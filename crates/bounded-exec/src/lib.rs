//! Bounded child-process execution imperative shell.
//!
//! Consumers supply explicit argv, environment behavior, input, deadlines,
//! capture limits, termination scope, and outcome policy. This crate owns only
//! process mechanics. It does not own sandboxing, executable authorization,
//! remote transport, output interpretation, evidence, or lifecycle policy.

#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fmt;
use std::io;
use std::io::Read;
use std::io::Write;
use std::path::PathBuf;
use std::process::Child;
use std::process::Command;
use std::process::ExitStatus;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use std::time::Instant;

use bounded_exec_core::CaptureAccumulator;
pub use bounded_exec_core::CapturePrepareError;
pub use bounded_exec_core::CapturedOutput;
pub use bounded_exec_core::Completion;
pub use bounded_exec_core::Disposition;
pub use bounded_exec_core::ExecutionLimits;
pub use bounded_exec_core::LimitError;
use bounded_exec_core::OutcomeFacts;
pub use bounded_exec_core::OutcomePolicy;
pub use bounded_exec_core::OverflowedStreams;
pub use bounded_exec_core::PolicyError;

const CAPTURE_BUFFER_BYTES: usize = 8_192;
const WORKER_CHANNEL_CAPACITY: usize = 1;
/// Extra time, beyond one poll interval, that a stopped worker gets to
/// observe its stop flag and return before it is reported as detached.
const WORKER_STOP_GRACE_MARGIN_MS: u64 = 100;
/// Largest stdin chunk written after readiness. POSIX guarantees that a pipe
/// write of at most `_POSIX_PIPE_BUF` bytes does not block once the pipe
/// reports writable.
#[cfg(unix)]
const PIPE_NONBLOCKING_WRITE_BYTES: usize = 512;

/// Whether the child inherits ambient environment variables.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EnvironmentMode {
    /// Remove the ambient environment before applying explicit entries.
    Clear,
    /// Inherit the ambient environment and apply explicit overrides.
    Inherit,
}

/// Standard-input source for the child.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Input {
    /// Connect standard input to the null device.
    Null,
    /// Write these exact bytes through a pipe, then close it.
    Bytes(Vec<u8>),
}

/// Processes owned by one execution request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminationScope {
    /// Terminate only the direct child.
    Child,
    /// Place the child in a fresh process group and terminate the whole group.
    ///
    /// Unix uses a real process group. Other platforms currently fall back to
    /// direct-child termination without claiming descendant teardown.
    ProcessGroup,
}

/// Fully explicit command description.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandSpec {
    /// Absolute executable path passed directly to the operating system.
    pub program: PathBuf,
    /// Exact argument vector excluding argv zero.
    pub args: Vec<OsString>,
    /// Absolute child working directory selected by the caller.
    pub current_dir: PathBuf,
    /// Ambient environment behavior.
    pub environment_mode: EnvironmentMode,
    /// Explicit environment entries or overrides.
    pub environment: Vec<(OsString, OsString)>,
    /// Standard-input source.
    pub input: Input,
}

/// Complete bounded execution request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunRequest {
    /// Command and process-context description.
    pub command: CommandSpec,
    /// Resource and timing bounds.
    pub limits: ExecutionLimits,
    /// Owned-process termination behavior.
    pub termination_scope: TerminationScope,
    /// Consumer-owned result classification policy.
    pub outcome_policy: OutcomePolicy,
}

/// Result of one completed bounded execution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutionOutput {
    /// Why execution stopped.
    pub completion: Completion,
    /// Portable exit code, if one was available.
    pub exit_code: Option<i32>,
    /// Unix termination signal, if one was available.
    pub signal: Option<i32>,
    /// Standard-input facts fixed by the request and child creation.
    pub input: InputDelivery,
    /// Bounded stdout capture.
    pub stdout: CapturedOutput,
    /// Bounded stderr capture.
    pub stderr: CapturedOutput,
    /// Product-neutral classification under the supplied outcome policy.
    pub disposition: Disposition,
}

/// Standard-input facts for one execution.
///
/// Every fact is fixed by the request and by whether a child was created, so
/// the same request and child behavior always produce the same facts. The
/// runner does not report how many bytes the stdin pipe accepted, nor whether
/// the child read any byte. When a child exits or is stopped without reading,
/// the accepted-byte count depends on writer scheduling, and consumption is
/// not observable from the writer side at all.
// r[impl bounded_exec.input_delivery.fact]
// r[impl bounded_exec.input_delivery.boundary]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InputDelivery {
    /// Bytes the caller supplied through [`Input::Bytes`]; zero for [`Input::Null`].
    pub offered_bytes: usize,
    /// How standard input was provided.
    pub status: InputStatus,
}

/// How standard input was provided.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputStatus {
    /// No child was created, so no byte was offered to a child.
    NotStarted,
    /// Standard input was the null device; no byte was offered.
    NullDevice,
    /// The offered bytes were offered through a stdin pipe. Writing stopped
    /// once every byte was written or the read end closed, and the runner
    /// closed the pipe. Which of the two happened is not reported: for a
    /// child that exits or is stopped without reading, it depends on
    /// scheduling. Neither proves that the child read any byte.
    Piped,
}

/// Worker associated with a runner error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Worker {
    /// Standard-input writer.
    Stdin,
    /// Standard-output reader.
    Stdout,
    /// Standard-error reader.
    Stderr,
}

/// Operating-system operation associated with a runner error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Operation {
    /// Spawn the child.
    Spawn,
    /// Poll child status.
    Wait,
    /// Terminate an owned child or process group.
    Kill,
    /// Reap the terminated direct child.
    Reap,
    /// Write standard input.
    WriteStdin,
    /// Read standard input. Reserved for future duplex execution support.
    ReadStdin,
    /// Read standard output.
    ReadStdout,
    /// Read standard error.
    ReadStderr,
}

/// Bounded runner failure.
#[derive(Debug)]
pub enum RunError {
    /// Invalid resource or timing limits.
    InvalidLimits(LimitError),
    /// The executable path is empty.
    EmptyProgram,
    /// The executable path is not absolute.
    ProgramNotAbsolute,
    /// The working directory is empty.
    EmptyCurrentDirectory,
    /// The working directory is not absolute.
    CurrentDirectoryNotAbsolute,
    /// An explicit environment variable appears more than once.
    DuplicateEnvironmentName,
    /// Supplied stdin exceeds its configured bound.
    InputLimitExceeded,
    /// Output retention capacity could not be prepared before spawn.
    CapturePreparation {
        /// Stream whose declared capacity could not be prepared.
        worker: Worker,
        /// Typed core preparation error.
        error: CapturePrepareError,
    },
    /// A configured pipe was unexpectedly unavailable.
    MissingPipe(Worker),
    /// Starting an I/O worker thread failed after spawning the child.
    WorkerStart {
        /// Worker whose thread could not be created.
        worker: Worker,
        /// Operating-system thread-creation error.
        error: io::Error,
        /// Cleanup failures observed while tearing down the child and any
        /// already-started workers under the shared teardown budget.
        cleanup_errors: Vec<RunError>,
    },
    /// An operating-system operation failed.
    Io {
        /// Failed operation.
        operation: Operation,
        /// Operating-system error.
        error: io::Error,
    },
    /// A stream byte counter overflowed.
    CaptureCountOverflow(Worker),
    /// A worker exited without delivering a result.
    WorkerDisconnected(Worker),
    /// A worker panicked.
    WorkerPanicked(Worker),
    /// A worker did not finish within the teardown deadline.
    WorkerTeardownTimedOut(Worker),
    /// The direct child did not reap within the teardown deadline.
    ChildTeardownTimedOut,
    /// The platform process identifier cannot be represented safely.
    ProcessIdentifierOverflow,
}

impl fmt::Display for RunError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLimits(error) => write!(formatter, "invalid execution limits: {error:?}"),
            Self::EmptyProgram => formatter.write_str("executable path is empty"),
            Self::ProgramNotAbsolute => formatter.write_str("executable path is not absolute"),
            Self::EmptyCurrentDirectory => formatter.write_str("working directory is empty"),
            Self::CurrentDirectoryNotAbsolute => {
                formatter.write_str("working directory is not absolute")
            }
            Self::DuplicateEnvironmentName => {
                formatter.write_str("explicit environment contains a duplicate name")
            }
            Self::InputLimitExceeded => {
                formatter.write_str("stdin exceeds its configured byte bound")
            }
            Self::CapturePreparation { worker, error } => write!(
                formatter,
                "{worker:?} capture capacity {} could not be prepared",
                error.requested_capacity()
            ),
            Self::MissingPipe(worker) => write!(formatter, "{worker:?} pipe is unavailable"),
            Self::WorkerStart {
                worker,
                error,
                cleanup_errors,
            } => {
                if cleanup_errors.is_empty() {
                    write!(formatter, "{worker:?} worker startup failed: {error}")
                } else {
                    write!(
                        formatter,
                        "{worker:?} worker startup failed: {error} (cleanup also recorded {} error(s))",
                        cleanup_errors.len()
                    )
                }
            }
            Self::Io { operation, error } => write!(formatter, "{operation:?} failed: {error}"),
            Self::CaptureCountOverflow(worker) => {
                write!(formatter, "{worker:?} byte count overflowed")
            }
            Self::WorkerDisconnected(worker) => write!(formatter, "{worker:?} worker disconnected"),
            Self::WorkerPanicked(worker) => write!(formatter, "{worker:?} worker panicked"),
            Self::WorkerTeardownTimedOut(worker) => {
                write!(formatter, "{worker:?} worker teardown timed out")
            }
            Self::ChildTeardownTimedOut => formatter.write_str("direct child teardown timed out"),
            Self::ProcessIdentifierOverflow => {
                formatter.write_str("child process identifier does not fit the platform PID type")
            }
        }
    }
}

impl std::error::Error for RunError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { error, .. } | Self::WorkerStart { error, .. } => Some(error),
            _ => None,
        }
    }
}

/// Execute a request without external cancellation.
///
/// # Errors
///
/// Returns an error when request validation, process operations, capture, or
/// teardown fails. Timeouts, output-limit dispositions, and a child that
/// closes stdin before reading all input are successful observations and are
/// returned in [`ExecutionOutput`].
pub fn run(request: RunRequest) -> Result<ExecutionOutput, RunError> {
    run_inner(request, None)
}

/// Execute a request while observing an explicit cancellation flag.
///
/// A flag already raised after validation and capture preparation produces
/// [`Completion::Cancelled`] without creating a child. A flag raised later
/// produces [`Completion::Cancelled`] after owned-process teardown.
///
/// # Errors
///
/// Returns the same infrastructure errors as [`run`]. Validation and capture
/// preparation errors take precedence over a raised flag.
pub fn run_with_cancellation(
    request: RunRequest,
    cancellation: &AtomicBool,
) -> Result<ExecutionOutput, RunError> {
    run_inner(request, Some(cancellation))
}

fn run_inner(
    request: RunRequest,
    cancellation: Option<&AtomicBool>,
) -> Result<ExecutionOutput, RunError> {
    run_with_spawner(request, cancellation, &mut OsWorkerSpawner)
}

fn run_with_spawner<S: WorkerSpawner>(
    request: RunRequest,
    cancellation: Option<&AtomicBool>,
    spawner: &mut S,
) -> Result<ExecutionOutput, RunError> {
    validate_request(&request)?;
    let RunRequest {
        command: command_spec,
        limits,
        termination_scope,
        outcome_policy,
    } = request;
    let prepared_captures = prepare_captures(limits)?;
    // r[impl bounded_exec.cancellation.pre_spawn]
    if cancellation_requested(cancellation) {
        let input = input_delivery(&command_spec.input, ChildCreation::NotStarted);
        return Ok(observation(
            Completion::Cancelled,
            None,
            input,
            prepared_captures.stdout.finish(),
            prepared_captures.stderr.finish(),
            &outcome_policy,
        ));
    }
    let mut command = prepare_command(&command_spec, termination_scope);
    let child = command.spawn().map_err(|error| RunError::Io {
        operation: Operation::Spawn,
        error,
    })?;

    // r[impl bounded_exec.worker_startup.fallible]
    // r[impl bounded_exec.worker_startup.cleanup]
    let mut startup = StartupGuard::new(child, termination_scope, limits);
    if matches!(&command_spec.input, Input::Bytes(_)) {
        startup.take_stdin()?;
    }
    startup.take_stdout()?;
    startup.take_stderr()?;

    let poll_interval = Duration::from_millis(limits.poll_interval_ms);
    let input = input_delivery(&command_spec.input, ChildCreation::Started);
    startup.start_stdin_worker(command_spec.input, poll_interval, spawner)?;
    startup.start_stdout_worker(prepared_captures.stdout, poll_interval, spawner)?;
    startup.start_stderr_worker(prepared_captures.stderr, poll_interval, spawner)?;

    let ActiveChild {
        mut child,
        stdin_worker,
        stdout_worker,
        stderr_worker,
    } = startup.into_active();

    // Remaining process-group members are terminated inside
    // `wait_for_completion`, before the direct child is reaped, so the group
    // identifier cannot be recycled between exit and `killpg`.
    let (completion, exit_status) =
        match wait_for_completion(&mut child, termination_scope, limits, cancellation) {
            Ok(result) => result,
            Err(error) => {
                let teardown_budget =
                    TeardownBudget::new(limits.teardown_timeout_ms, limits.poll_interval_ms);
                let _stdin_cleanup =
                    stdin_worker.map(|worker| finish_worker(worker, teardown_budget));
                let _stdout_cleanup = finish_worker(stdout_worker, teardown_budget);
                let _stderr_cleanup = finish_worker(stderr_worker, teardown_budget);
                return Err(error);
            }
        };

    let teardown_budget = TeardownBudget::new(limits.teardown_timeout_ms, limits.poll_interval_ms);
    let stdin_result = stdin_worker.map_or(Ok(()), |worker| finish_worker(worker, teardown_budget));
    let stdout_result = finish_worker(stdout_worker, teardown_budget);
    let stderr_result = finish_worker(stderr_worker, teardown_budget);
    stdin_result?;
    let stdout = stdout_result?;
    let stderr = stderr_result?;
    Ok(observation(
        completion,
        exit_status,
        input,
        stdout,
        stderr,
        &outcome_policy,
    ))
}

fn observation(
    completion: Completion,
    exit_status: Option<ExitStatus>,
    input: InputDelivery,
    stdout: CapturedOutput,
    stderr: CapturedOutput,
    outcome_policy: &OutcomePolicy,
) -> ExecutionOutput {
    let exit_code = exit_status.and_then(|status| status.code());
    let signal = exit_status.and_then(exit_signal);
    let facts = OutcomeFacts {
        completion,
        exit_code,
        stdout_truncated: stdout.truncated,
        stderr_truncated: stderr.truncated,
    };
    let disposition = bounded_exec_core::classify(facts, outcome_policy);
    ExecutionOutput {
        completion,
        exit_code,
        signal,
        input,
        stdout,
        stderr,
        disposition,
    }
}

fn cancellation_requested(cancellation: Option<&AtomicBool>) -> bool {
    cancellation.is_some_and(|flag| flag.load(Ordering::Acquire))
}

fn offered_input_bytes(input: &Input) -> usize {
    match input {
        Input::Null => 0,
        Input::Bytes(bytes) => bytes.len(),
    }
}

/// Whether a child process was created for the run.
#[derive(Clone, Copy)]
enum ChildCreation {
    NotStarted,
    Started,
}

/// Derive input facts from the request and child creation only.
///
/// No writer progress enters the result, so it cannot vary with scheduling.
// r[impl bounded_exec.input_delivery.fact]
fn input_delivery(input: &Input, creation: ChildCreation) -> InputDelivery {
    let status = match (creation, input) {
        (ChildCreation::NotStarted, _) => InputStatus::NotStarted,
        (ChildCreation::Started, Input::Null) => InputStatus::NullDevice,
        (ChildCreation::Started, Input::Bytes(_)) => InputStatus::Piped,
    };
    InputDelivery {
        offered_bytes: offered_input_bytes(input),
        status,
    }
}

struct PreparedCaptures {
    stdout: CaptureAccumulator,
    stderr: CaptureAccumulator,
}

// r[impl bounded_exec.capture_capacity.pre_spawn]
fn prepare_captures(limits: ExecutionLimits) -> Result<PreparedCaptures, RunError> {
    let stdout = CaptureAccumulator::prepare(limits.stdout_max_bytes).map_err(|error| {
        RunError::CapturePreparation {
            worker: Worker::Stdout,
            error,
        }
    })?;
    let stderr = CaptureAccumulator::prepare(limits.stderr_max_bytes).map_err(|error| {
        RunError::CapturePreparation {
            worker: Worker::Stderr,
            error,
        }
    })?;
    assert!(stdout.prepared_capacity() >= stdout.limit_bytes());
    assert!(stderr.prepared_capacity() >= stderr.limit_bytes());
    Ok(PreparedCaptures { stdout, stderr })
}

fn validate_request(request: &RunRequest) -> Result<(), RunError> {
    request.limits.validate().map_err(RunError::InvalidLimits)?;
    if request.command.program.as_os_str().is_empty() {
        return Err(RunError::EmptyProgram);
    }
    if !request.command.program.is_absolute() {
        return Err(RunError::ProgramNotAbsolute);
    }
    if request.command.current_dir.as_os_str().is_empty() {
        return Err(RunError::EmptyCurrentDirectory);
    }
    if !request.command.current_dir.is_absolute() {
        return Err(RunError::CurrentDirectoryNotAbsolute);
    }
    if offered_input_bytes(&request.command.input) > request.limits.stdin_max_bytes {
        return Err(RunError::InputLimitExceeded);
    }
    let mut names = BTreeSet::new();
    for (name, _) in &request.command.environment {
        if !names.insert(name.clone()) {
            return Err(RunError::DuplicateEnvironmentName);
        }
    }
    Ok(())
}

fn prepare_command(command_spec: &CommandSpec, termination_scope: TerminationScope) -> Command {
    let mut command = Command::new(&command_spec.program);
    command.args(&command_spec.args);
    command.current_dir(&command_spec.current_dir);
    if command_spec.environment_mode == EnvironmentMode::Clear {
        command.env_clear();
    }
    command.envs(command_spec.environment.iter().cloned());
    match &command_spec.input {
        Input::Null => {
            command.stdin(Stdio::null());
        }
        Input::Bytes(_) => {
            command.stdin(Stdio::piped());
        }
    }
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    configure_termination_scope(&mut command, termination_scope);
    command
}

#[cfg(unix)]
fn configure_termination_scope(command: &mut Command, scope: TerminationScope) {
    use std::os::unix::process::CommandExt;

    if scope == TerminationScope::ProcessGroup {
        command.process_group(0);
    }
}

#[cfg(not(unix))]
fn configure_termination_scope(_command: &mut Command, _scope: TerminationScope) {}

#[derive(Clone, Copy)]
struct TeardownBudget {
    started: Instant,
    timeout: Duration,
    stop_grace: Duration,
}

impl TeardownBudget {
    fn new(timeout_ms: u64, poll_interval_ms: u64) -> Self {
        Self {
            started: Instant::now(),
            timeout: Duration::from_millis(timeout_ms),
            stop_grace: Duration::from_millis(
                poll_interval_ms.saturating_add(WORKER_STOP_GRACE_MARGIN_MS),
            ),
        }
    }

    fn remaining(self) -> Duration {
        self.timeout.saturating_sub(self.started.elapsed())
    }
}

struct WorkerHandle<T> {
    worker: Worker,
    receiver: mpsc::Receiver<Result<T, RunError>>,
    thread: thread::JoinHandle<()>,
    stop: Arc<AtomicBool>,
}

/// Pipe end usable by an I/O worker.
///
/// On Unix, workers wait for readiness with a bounded `poll` so that a pipe
/// held open by an escaped descendant cannot pin the worker forever.
#[cfg(unix)]
trait WorkerPipe: std::os::fd::AsFd {}
#[cfg(unix)]
impl<T: std::os::fd::AsFd> WorkerPipe for T {}
#[cfg(not(unix))]
trait WorkerPipe {}
#[cfg(not(unix))]
impl<T> WorkerPipe for T {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Readiness {
    Ready,
    Stopped,
}

#[derive(Clone, Copy)]
enum PipeDirection {
    Read,
    Write,
}

/// Wait until the pipe is ready or the stop flag is raised.
///
/// Readiness includes hang-up and error conditions; the following read or
/// write observes them.
#[cfg(unix)]
fn wait_ready<P: WorkerPipe>(
    pipe: &P,
    direction: PipeDirection,
    stop: &AtomicBool,
    poll_interval: Duration,
) -> io::Result<Readiness> {
    use nix::errno::Errno;
    use nix::poll::PollFd;
    use nix::poll::PollFlags;
    use nix::poll::PollTimeout;
    use nix::poll::poll;

    let events = match direction {
        PipeDirection::Read => PollFlags::POLLIN,
        PipeDirection::Write => PollFlags::POLLOUT,
    };
    let timeout = PollTimeout::try_from(poll_interval).unwrap_or(PollTimeout::MAX);
    loop {
        if stop.load(Ordering::Acquire) {
            return Ok(Readiness::Stopped);
        }
        let mut fds = [PollFd::new(pipe.as_fd(), events)];
        match poll(&mut fds, timeout) {
            Ok(0) | Err(Errno::EINTR) => {}
            Ok(_) => return Ok(Readiness::Ready),
            Err(error) => return Err(io::Error::from_raw_os_error(error as i32)),
        }
    }
}

/// Non-Unix platforms keep blocking I/O and make no detachment claim.
#[cfg(not(unix))]
fn wait_ready<P: WorkerPipe>(
    _pipe: &P,
    _direction: PipeDirection,
    stop: &AtomicBool,
    _poll_interval: Duration,
) -> io::Result<Readiness> {
    if stop.load(Ordering::Acquire) {
        Ok(Readiness::Stopped)
    } else {
        Ok(Readiness::Ready)
    }
}

trait WorkerSpawner {
    fn spawn<F>(&mut self, worker: Worker, task: F) -> io::Result<thread::JoinHandle<()>>
    where
        F: FnOnce() + Send + 'static;
}

struct OsWorkerSpawner;

impl WorkerSpawner for OsWorkerSpawner {
    // r[impl bounded_exec.worker_startup.fallible]
    fn spawn<F>(&mut self, worker: Worker, task: F) -> io::Result<thread::JoinHandle<()>>
    where
        F: FnOnce() + Send + 'static,
    {
        thread::Builder::new()
            .name(worker_thread_name(worker).to_owned())
            .spawn(task)
    }
}

const fn worker_thread_name(worker: Worker) -> &'static str {
    match worker {
        Worker::Stdin => "bounded-exec-stdin",
        Worker::Stdout => "bounded-exec-stdout",
        Worker::Stderr => "bounded-exec-stderr",
    }
}

struct ActiveChild {
    child: Child,
    stdin_worker: Option<WorkerHandle<()>>,
    stdout_worker: WorkerHandle<CapturedOutput>,
    stderr_worker: WorkerHandle<CapturedOutput>,
}

/// Armed owner for a spawned child, its pipes, and its started I/O workers
/// during the startup window before [`wait_for_completion`].
///
/// Ownership is transferred and the guard disarmed only through
/// [`Self::into_active`]. On an ordinary fallible error, [`Self::teardown`]
/// runs explicitly and returns every cleanup error so the caller can preserve
/// both the startup failure and any uncertain teardown result. On unwinding,
/// [`Drop`] executes the same non-panicking teardown.
// r[impl bounded_exec.worker_startup.cleanup]
// r[impl bounded_exec.worker_startup.unwind]
struct StartupGuard {
    child: Option<Child>,
    scope: TerminationScope,
    limits: ExecutionLimits,
    stdin_pipe: Option<std::process::ChildStdin>,
    stdout_pipe: Option<std::process::ChildStdout>,
    stderr_pipe: Option<std::process::ChildStderr>,
    stdin_worker: Option<WorkerHandle<()>>,
    stdout_worker: Option<WorkerHandle<CapturedOutput>>,
    stderr_worker: Option<WorkerHandle<CapturedOutput>>,
}

impl StartupGuard {
    const fn new(child: Child, scope: TerminationScope, limits: ExecutionLimits) -> Self {
        Self {
            child: Some(child),
            scope,
            limits,
            stdin_pipe: None,
            stdout_pipe: None,
            stderr_pipe: None,
            stdin_worker: None,
            stdout_worker: None,
            stderr_worker: None,
        }
    }

    fn take_stdin(&mut self) -> Result<(), RunError> {
        let stdin = self.child.as_mut().and_then(|child| child.stdin.take());
        let Some(stdin) = stdin else {
            return Err(self.fail_missing_pipe(Worker::Stdin));
        };
        self.stdin_pipe = Some(stdin);
        Ok(())
    }

    fn take_stdout(&mut self) -> Result<(), RunError> {
        let stdout = self.child.as_mut().and_then(|child| child.stdout.take());
        let Some(stdout) = stdout else {
            return Err(self.fail_missing_pipe(Worker::Stdout));
        };
        self.stdout_pipe = Some(stdout);
        Ok(())
    }

    fn take_stderr(&mut self) -> Result<(), RunError> {
        let stderr = self.child.as_mut().and_then(|child| child.stderr.take());
        let Some(stderr) = stderr else {
            return Err(self.fail_missing_pipe(Worker::Stderr));
        };
        self.stderr_pipe = Some(stderr);
        Ok(())
    }

    fn start_stdin_worker<S: WorkerSpawner>(
        &mut self,
        input: Input,
        poll_interval: Duration,
        spawner: &mut S,
    ) -> Result<(), RunError> {
        let (Some(stdin), Input::Bytes(bytes)) = (self.stdin_pipe.take(), input) else {
            return Ok(());
        };
        match start_stdin_worker(stdin, bytes, poll_interval, spawner) {
            Ok(worker) => {
                self.stdin_worker = Some(worker);
                Ok(())
            }
            Err(error) => Err(self.fail_worker_start(Worker::Stdin, error)),
        }
    }

    fn start_stdout_worker<S: WorkerSpawner>(
        &mut self,
        capture: CaptureAccumulator,
        poll_interval: Duration,
        spawner: &mut S,
    ) -> Result<(), RunError> {
        let Some(stdout) = self.stdout_pipe.take() else {
            return Err(self.fail_missing_pipe(Worker::Stdout));
        };
        match start_capture_worker(stdout, capture, Worker::Stdout, poll_interval, spawner) {
            Ok(worker) => {
                self.stdout_worker = Some(worker);
                Ok(())
            }
            Err(error) => Err(self.fail_worker_start(Worker::Stdout, error)),
        }
    }

    fn start_stderr_worker<S: WorkerSpawner>(
        &mut self,
        capture: CaptureAccumulator,
        poll_interval: Duration,
        spawner: &mut S,
    ) -> Result<(), RunError> {
        let Some(stderr) = self.stderr_pipe.take() else {
            return Err(self.fail_missing_pipe(Worker::Stderr));
        };
        match start_capture_worker(stderr, capture, Worker::Stderr, poll_interval, spawner) {
            Ok(worker) => {
                self.stderr_worker = Some(worker);
                Ok(())
            }
            Err(error) => Err(self.fail_worker_start(Worker::Stderr, error)),
        }
    }

    fn into_active(mut self) -> ActiveChild {
        let child = self
            .child
            .take()
            .expect("active child must exist when disarming startup guard");
        let stdin_worker = self.stdin_worker.take();
        let stdout_worker = self
            .stdout_worker
            .take()
            .expect("stdout worker must exist when disarming startup guard");
        let stderr_worker = self
            .stderr_worker
            .take()
            .expect("stderr worker must exist when disarming startup guard");
        ActiveChild {
            child,
            stdin_worker,
            stdout_worker,
            stderr_worker,
        }
    }

    fn fail_missing_pipe(&mut self, worker: Worker) -> RunError {
        if let Some(cleanup_error) = self.teardown().into_iter().next() {
            return cleanup_error;
        }
        RunError::MissingPipe(worker)
    }

    fn fail_worker_start(&mut self, worker: Worker, error: io::Error) -> RunError {
        let cleanup_errors = self.teardown();
        RunError::WorkerStart {
            worker,
            error,
            cleanup_errors,
        }
    }

    fn teardown(&mut self) -> Vec<RunError> {
        // Close any unused pipes first so no parent descriptor keeps child I/O
        // open during termination.
        drop(self.stdin_pipe.take());
        drop(self.stdout_pipe.take());
        drop(self.stderr_pipe.take());
        if let Some(child) = self.child.as_mut() {
            drop(child.stdin.take());
            drop(child.stdout.take());
            drop(child.stderr.take());
        }

        let child = self.child.take();
        let stdin_worker = self.stdin_worker.take();
        let stdout_worker = self.stdout_worker.take();
        let stderr_worker = self.stderr_worker.take();
        if child.is_none()
            && stdin_worker.is_none()
            && stdout_worker.is_none()
            && stderr_worker.is_none()
        {
            return Vec::new();
        }

        let teardown_budget = TeardownBudget::new(
            self.limits.teardown_timeout_ms,
            self.limits.poll_interval_ms,
        );
        let mut cleanup_errors = Vec::new();

        // Terminate and reap the owned process(es) before waiting on workers so
        // blocked pipe I/O unblocks without deadlocking on the child.
        if let Some(mut child) = child {
            if let Err(error) = terminate_and_reap_with_budget(
                &mut child,
                self.scope,
                self.limits.poll_interval_ms,
                teardown_budget,
            ) {
                cleanup_errors.push(error);
            }
        }

        if let Some(worker) = stdin_worker {
            if let Err(error) = finish_worker(worker, teardown_budget) {
                cleanup_errors.push(error);
            }
        }
        if let Some(worker) = stdout_worker {
            if let Err(error) = finish_worker(worker, teardown_budget) {
                cleanup_errors.push(error);
            }
        }
        if let Some(worker) = stderr_worker {
            if let Err(error) = finish_worker(worker, teardown_budget) {
                cleanup_errors.push(error);
            }
        }

        cleanup_errors
    }
}

impl Drop for StartupGuard {
    fn drop(&mut self) {
        let _cleanup_errors = self.teardown();
    }
}

fn start_stdin_worker<S: WorkerSpawner>(
    stdin: std::process::ChildStdin,
    bytes: Vec<u8>,
    poll_interval: Duration,
    spawner: &mut S,
) -> io::Result<WorkerHandle<()>> {
    let (sender, receiver) = mpsc::sync_channel(WORKER_CHANNEL_CAPACITY);
    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = Arc::clone(&stop);
    let worker_thread = spawner.spawn(Worker::Stdin, move || {
        let result = write_stdin(stdin, &bytes, &worker_stop, poll_interval);
        let _send_result = sender.send(result);
    })?;
    Ok(WorkerHandle {
        worker: Worker::Stdin,
        receiver,
        thread: worker_thread,
        stop,
    })
}

// r[impl bounded_exec.input_delivery.reader_closed]
// r[impl bounded_exec.input_delivery.write_errors]
fn write_stdin<W>(
    mut stdin: W,
    bytes: &[u8],
    stop: &AtomicBool,
    poll_interval: Duration,
) -> Result<(), RunError>
where
    W: Write + WorkerPipe,
{
    let write_error = |error| RunError::Io {
        operation: Operation::WriteStdin,
        error,
    };
    let mut remaining = bytes;
    while !remaining.is_empty() {
        if wait_ready(&stdin, PipeDirection::Write, stop, poll_interval).map_err(write_error)?
            == Readiness::Stopped
        {
            return Err(RunError::WorkerTeardownTimedOut(Worker::Stdin));
        }
        let chunk = &remaining[..remaining.len().min(stdin_chunk_bytes())];
        match stdin.write(chunk) {
            Ok(0) => {
                return Err(write_error(io::Error::from(io::ErrorKind::WriteZero)));
            }
            Ok(written) => remaining = &remaining[written..],
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            // The read end closed because the child closed it, exited, or
            // was stopped. Whether a child that never reads its input closes
            // the pipe before or after the last byte is written depends on
            // scheduling, so closure ends writing in every completion and
            // leaves no trace in the observation.
            Err(error) if is_reader_closure(&error) => return Ok(()),
            Err(error) => return Err(write_error(error)),
        }
    }
    stdin.flush().map_err(write_error)
}

/// Whether a stdin write error means every read end of the pipe is closed.
fn is_reader_closure(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::BrokenPipe
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::ConnectionAborted
    )
}

#[cfg(unix)]
const fn stdin_chunk_bytes() -> usize {
    PIPE_NONBLOCKING_WRITE_BYTES
}

#[cfg(not(unix))]
const fn stdin_chunk_bytes() -> usize {
    CAPTURE_BUFFER_BYTES
}

// r[impl bounded_exec.capture_capacity.steady_state]
// r[impl bounded_exec.worker_startup.fallible]
fn start_capture_worker<R, S>(
    reader: R,
    capture: CaptureAccumulator,
    worker: Worker,
    poll_interval: Duration,
    spawner: &mut S,
) -> io::Result<WorkerHandle<CapturedOutput>>
where
    R: Read + WorkerPipe + Send + 'static,
    S: WorkerSpawner,
{
    let (sender, receiver) = mpsc::sync_channel(WORKER_CHANNEL_CAPACITY);
    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = Arc::clone(&stop);
    let worker_thread = spawner.spawn(worker, move || {
        let result = read_bounded(reader, capture, worker, &worker_stop, poll_interval);
        let _send_result = sender.send(result);
    })?;
    Ok(WorkerHandle {
        worker,
        receiver,
        thread: worker_thread,
        stop,
    })
}

fn read_bounded<R>(
    mut reader: R,
    mut capture: CaptureAccumulator,
    worker: Worker,
    stop: &AtomicBool,
    poll_interval: Duration,
) -> Result<CapturedOutput, RunError>
where
    R: Read + WorkerPipe,
{
    let read_error = |error| RunError::Io {
        operation: read_operation(worker),
        error,
    };
    let prepared_capacity = capture.prepared_capacity();
    let mut buffer = [0_u8; CAPTURE_BUFFER_BYTES];
    loop {
        if wait_ready(&reader, PipeDirection::Read, stop, poll_interval).map_err(read_error)?
            == Readiness::Stopped
        {
            return Err(RunError::WorkerTeardownTimedOut(worker));
        }
        let read_bytes = match reader.read(&mut buffer) {
            Ok(read_bytes) => read_bytes,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(read_error(error)),
        };
        if read_bytes == 0 {
            break;
        }
        capture
            .observe(&buffer[..read_bytes])
            .map_err(|_error| RunError::CaptureCountOverflow(worker))?;
        assert_eq!(capture.prepared_capacity(), prepared_capacity);
    }
    Ok(capture.finish())
}

const fn read_operation(worker: Worker) -> Operation {
    match worker {
        Worker::Stdout => Operation::ReadStdout,
        Worker::Stderr => Operation::ReadStderr,
        Worker::Stdin => Operation::ReadStdin,
    }
}

fn wait_for_completion(
    child: &mut Child,
    scope: TerminationScope,
    limits: ExecutionLimits,
    cancellation: Option<&AtomicBool>,
) -> Result<(Completion, Option<ExitStatus>), RunError> {
    let started = Instant::now();
    let timeout = Duration::from_millis(limits.timeout_ms);
    let poll_interval = Duration::from_millis(limits.poll_interval_ms);
    loop {
        // r[impl bounded_exec.cancellation.post_spawn]
        if cancellation_requested(cancellation) {
            let status = terminate_and_reap(child, scope, limits)?;
            return Ok((Completion::Cancelled, status));
        }
        let exited = match child_has_exited(child) {
            Ok(exited) => exited,
            Err(error) => {
                let wait_error = RunError::Io {
                    operation: Operation::Wait,
                    error,
                };
                terminate_and_reap(child, scope, limits)?;
                return Err(wait_error);
            }
        };
        if exited {
            return Ok((Completion::Exited, reap_exited(child, scope)?));
        }
        let elapsed = started.elapsed();
        if elapsed >= timeout {
            let status = terminate_and_reap(child, scope, limits)?;
            return Ok((Completion::TimedOut, status));
        }
        thread::sleep(poll_interval.min(timeout.saturating_sub(elapsed)));
    }
}

/// Report whether the direct child has exited.
///
/// Where `waitid(WNOWAIT)` is available, the exited child is left as an
/// unreaped zombie so its PID, and therefore its process-group identifier,
/// stays reserved until [`reap_exited`] has terminated remaining members.
#[cfg(any(
    target_os = "android",
    all(target_os = "linux", not(target_env = "uclibc"))
))]
fn child_has_exited(child: &mut Child) -> io::Result<bool> {
    use nix::errno::Errno;
    use nix::sys::wait::Id;
    use nix::sys::wait::WaitPidFlag;
    use nix::sys::wait::WaitStatus;
    use nix::sys::wait::waitid;
    use nix::unistd::Pid;

    let pid =
        i32::try_from(child.id()).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
    let flags = WaitPidFlag::WEXITED | WaitPidFlag::WNOHANG | WaitPidFlag::WNOWAIT;
    match waitid(Id::Pid(Pid::from_raw(pid)), flags) {
        Ok(WaitStatus::StillAlive) | Err(Errno::EINTR) => Ok(false),
        Ok(_) => Ok(true),
        Err(error) => Err(io::Error::from_raw_os_error(error as i32)),
    }
}

/// Without `waitid(WNOWAIT)`, exit detection reaps the child. A later
/// process-group termination can then race PID reuse; no stronger claim is
/// made on these platforms.
#[cfg(not(any(
    target_os = "android",
    all(target_os = "linux", not(target_env = "uclibc"))
)))]
fn child_has_exited(child: &mut Child) -> io::Result<bool> {
    child.try_wait().map(|status| status.is_some())
}

/// Terminate remaining owned group members, then reap the exited child.
fn reap_exited(child: &mut Child, scope: TerminationScope) -> Result<Option<ExitStatus>, RunError> {
    let group_result = if scope == TerminationScope::ProcessGroup {
        terminate_process_group(child.id())
    } else {
        Ok(())
    };
    // Reap even if group termination failed so no zombie is left behind.
    // The child has already exited, so this does not block.
    let status = child.wait().map_err(|error| RunError::Io {
        operation: Operation::Reap,
        error,
    })?;
    group_result?;
    Ok(Some(status))
}

fn terminate_and_reap(
    child: &mut Child,
    scope: TerminationScope,
    limits: ExecutionLimits,
) -> Result<Option<ExitStatus>, RunError> {
    let teardown_budget = TeardownBudget::new(limits.teardown_timeout_ms, limits.poll_interval_ms);
    terminate_and_reap_with_budget(child, scope, limits.poll_interval_ms, teardown_budget)
}

fn terminate_and_reap_with_budget(
    child: &mut Child,
    scope: TerminationScope,
    poll_interval_ms: u64,
    teardown_budget: TeardownBudget,
) -> Result<Option<ExitStatus>, RunError> {
    terminate_owned(child, scope)?;
    let poll_interval = Duration::from_millis(poll_interval_ms);
    loop {
        let status = child.try_wait().map_err(|error| RunError::Io {
            operation: Operation::Reap,
            error,
        })?;
        if status.is_some() {
            return Ok(status);
        }
        let remaining = teardown_budget.remaining();
        if remaining.is_zero() {
            return Err(RunError::ChildTeardownTimedOut);
        }
        thread::sleep(poll_interval.min(remaining));
    }
}

#[cfg(unix)]
fn terminate_owned(child: &mut Child, scope: TerminationScope) -> Result<(), RunError> {
    if scope == TerminationScope::ProcessGroup {
        return terminate_process_group(child.id());
    }
    terminate_direct_child(child)
}

#[cfg(not(unix))]
fn terminate_owned(child: &mut Child, _scope: TerminationScope) -> Result<(), RunError> {
    terminate_direct_child(child)
}

fn terminate_direct_child(child: &mut Child) -> Result<(), RunError> {
    let Err(kill_error) = child.kill() else {
        return Ok(());
    };
    match child.try_wait() {
        Ok(Some(_status)) => Ok(()),
        Ok(None) | Err(_) => Err(RunError::Io {
            operation: Operation::Kill,
            error: kill_error,
        }),
    }
}

#[cfg(unix)]
fn terminate_process_group(child_id: u32) -> Result<(), RunError> {
    use nix::errno::Errno;
    use nix::sys::signal::Signal;
    use nix::sys::signal::killpg;
    use nix::unistd::Pid;

    let process_group = i32::try_from(child_id).map_err(|_| RunError::ProcessIdentifierOverflow)?;
    match killpg(Pid::from_raw(process_group), Signal::SIGKILL) {
        Ok(()) | Err(Errno::ESRCH) => Ok(()),
        Err(error) => Err(RunError::Io {
            operation: Operation::Kill,
            error: io::Error::from_raw_os_error(error as i32),
        }),
    }
}

#[cfg(not(unix))]
fn terminate_process_group(_child_id: u32) -> Result<(), RunError> {
    Ok(())
}

fn finish_worker<T>(
    worker: WorkerHandle<T>,
    teardown_budget: TeardownBudget,
) -> Result<T, RunError> {
    let received = worker.receiver.recv_timeout(teardown_budget.remaining());
    match received {
        Ok(result) => {
            if worker.thread.join().is_err() {
                return Err(RunError::WorkerPanicked(worker.worker));
            }
            result
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            // A pipe is still held open, typically by a descendant that left
            // the owned process group. Stop the worker so its thread and
            // prepared buffer are released instead of leaking. On Unix the
            // worker observes the flag within one poll interval.
            worker.stop.store(true, Ordering::Release);
            match worker.receiver.recv_timeout(teardown_budget.stop_grace) {
                Ok(_) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                    let _join_result = worker.thread.join();
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            Err(RunError::WorkerTeardownTimedOut(worker.worker))
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            if worker.thread.join().is_err() {
                Err(RunError::WorkerPanicked(worker.worker))
            } else {
                Err(RunError::WorkerDisconnected(worker.worker))
            }
        }
    }
}

#[cfg(unix)]
fn exit_signal(status: ExitStatus) -> Option<i32> {
    use std::os::unix::process::ExitStatusExt;

    status.signal()
}

#[cfg(not(unix))]
const fn exit_signal(_status: ExitStatus) -> Option<i32> {
    None
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::sync::atomic::AtomicBool;
    use std::time::Duration;
    use std::time::Instant;

    use super::*;

    const SUCCESS_EXIT_CODE: i32 = 0;
    const TEST_TIMEOUT_MS: u64 = 1_000;
    const SHORT_TIMEOUT_MS: u64 = 100;
    const TEST_STDIN_MAX_BYTES: usize = 1_024;
    const TEST_OUTPUT_MAX_BYTES: usize = 1_024;
    const SMALL_OUTPUT_MAX_BYTES: usize = 4;
    const TEST_POLL_INTERVAL_MS: u64 = 10;
    const TEST_TEARDOWN_TIMEOUT_MS: u64 = 1_000;
    const TEST_EXECUTION_UPPER_BOUND_SECS: u64 = 2;
    const NON_TERMINATING_SHELL_LOOP: &str = "while :; do :; done";
    const EXPECTED_OUTPUT_BYTES: &[u8] = b"bounded";
    const LONG_TIMEOUT_MS: u64 = 10_000;
    const MARKER_SCRIPT: &str = "printf started > \"$1\"";
    const MARKER_WAIT_POLL_MS: u64 = 5;
    const PRE_SPAWN_PROOF_RUNS: usize = 256;
    const UNREAD_SMALL_INPUT: &[u8] = b"bounded\n";
    /// 4 MiB: four times Linux's default `pipe-max-size` and 64 times its
    /// default pipe buffer, so an unread write cannot complete and always
    /// ends in reader closure.
    const PIPE_EXCEEDING_INPUT_BYTES: usize = 4 << 20;
    const INPUT_FILL_BYTE: u8 = b'x';
    const READ_TO_END_LINE: &[u8] = b"bounded\n";
    /// 128 KiB of lines, twice the default Linux pipe buffer, so complete
    /// delivery requires the child to read.
    const READ_TO_END_LINES: usize = 16_384;
    const SEQUENTIAL_UNREAD_RUNS: usize = 64;
    /// More concurrent children than typical CI cores, so writers and
    /// exiting children contend for scheduling.
    const STRESS_WORKERS: usize = 16;
    const STRESS_RUNS_PER_WORKER: usize = 32;
    const FAULT_INPUT_BYTES: usize = 4_096;
    const FAULT_ACCEPTED_BYTES: usize = 1_024;
    const STOP_WRITER_AFTER_MS: u64 = 50;

    fn request(script: &str) -> RunRequest {
        RunRequest {
            command: CommandSpec {
                program: PathBuf::from("/bin/sh"),
                args: vec![OsString::from("-c"), OsString::from(script)],
                current_dir: PathBuf::from("/"),
                environment_mode: EnvironmentMode::Clear,
                environment: Vec::new(),
                input: Input::Null,
            },
            limits: ExecutionLimits {
                timeout_ms: TEST_TIMEOUT_MS,
                stdin_max_bytes: TEST_STDIN_MAX_BYTES,
                stdout_max_bytes: TEST_OUTPUT_MAX_BYTES,
                stderr_max_bytes: TEST_OUTPUT_MAX_BYTES,
                poll_interval_ms: TEST_POLL_INTERVAL_MS,
                teardown_timeout_ms: TEST_TEARDOWN_TIMEOUT_MS,
            },
            termination_scope: TerminationScope::ProcessGroup,
            outcome_policy: OutcomePolicy::new(vec![SUCCESS_EXIT_CODE], true, true)
                .expect("fixture outcome policy is valid"),
        }
    }

    fn marker_request(script: &str, marker: &std::path::Path) -> RunRequest {
        let mut request = request(script);
        request.command.args.push(OsString::from("sh"));
        request.command.args.push(marker.as_os_str().to_owned());
        request
    }

    fn scratch_directory(label: &str) -> PathBuf {
        let directory =
            std::env::temp_dir().join(format!("bounded-exec-{label}-{}", std::process::id()));
        let _stale = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("scratch directory must be created");
        directory
    }

    fn pipe_exceeding_input() -> Vec<u8> {
        vec![INPUT_FILL_BYTE; PIPE_EXCEEDING_INPUT_BYTES]
    }

    fn empty_capture() -> CapturedOutput {
        CapturedOutput {
            bytes: Vec::new(),
            observed_bytes: 0,
            truncated: false,
        }
    }

    fn piped(offered_bytes: usize) -> InputDelivery {
        InputDelivery {
            offered_bytes,
            status: InputStatus::Piped,
        }
    }

    fn immediate_exit_request(input: &[u8]) -> RunRequest {
        let mut request = request("exit 0");
        request.limits.stdin_max_bytes = PIPE_EXCEEDING_INPUT_BYTES;
        request.command.input = Input::Bytes(input.to_vec());
        request
    }

    /// The single observation every immediate-exit run must produce,
    /// whatever the stdin writer's scheduling.
    fn immediate_exit_outcome(offered_bytes: usize) -> ExecutionOutput {
        ExecutionOutput {
            completion: Completion::Exited,
            exit_code: Some(SUCCESS_EXIT_CODE),
            signal: None,
            input: piped(offered_bytes),
            stdout: empty_capture(),
            stderr: empty_capture(),
            disposition: Disposition::Succeeded,
        }
    }

    fn assert_immediate_exit_outcome(input: &[u8]) {
        let output = run(immediate_exit_request(input)).expect("unread input never fails a run");

        assert_eq!(output, immediate_exit_outcome(input.len()));
    }

    /// Whether one immediate-exit run leaked a stdin write error. Any other
    /// difference from the expected observation fails the test directly.
    fn immediate_exit_leaked_stdin_error(input: &[u8]) -> bool {
        match run(immediate_exit_request(input)) {
            Ok(output) => {
                assert_eq!(output, immediate_exit_outcome(input.len()));
                false
            }
            Err(RunError::Io {
                operation: Operation::WriteStdin,
                ..
            }) => true,
            Err(error) => panic!("unexpected run error {error:?}"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn explicit_environment_and_input_round_trip() {
        let mut request = request(
            "if [ \"${HOME+x}\" = x ]; then exit 1; fi; IFS= read -r value; printf '%s:%s' \"$FIXTURE\" \"$value\"",
        );
        request
            .command
            .environment
            .push((OsString::from("FIXTURE"), OsString::from("bounded")));
        request.command.input = Input::Bytes(b"input\n".to_vec());

        let output = run(request).expect("bounded command succeeds");

        assert_eq!(output.disposition, Disposition::Succeeded);
        assert_eq!(output.stdout.bytes, b"bounded:input");
        assert_eq!(output.stdout.observed_bytes, output.stdout.bytes.len());
        assert!(!output.stdout.truncated);
        assert_eq!(output.input, piped(b"input\n".len()));
    }

    #[cfg(unix)]
    #[test]
    fn truncation_is_bounded_and_policy_visible() {
        let mut strict_request = request("printf 'bounded-output'");
        strict_request.limits.stdout_max_bytes = SMALL_OUTPUT_MAX_BYTES;
        let mut truncating_request = strict_request.clone();
        truncating_request.outcome_policy =
            OutcomePolicy::new(vec![SUCCESS_EXIT_CODE], false, false)
                .expect("truncating fixture policy is valid");

        let strict =
            run(strict_request).expect("strict output flood is captured as an observation");
        let truncating =
            run(truncating_request).expect("permitted truncation is captured as an observation");

        assert_eq!(strict.stdout.bytes.len(), SMALL_OUTPUT_MAX_BYTES);
        assert!(strict.stdout.observed_bytes > strict.stdout.bytes.len());
        assert!(strict.stdout.truncated);
        assert_eq!(
            strict.disposition,
            Disposition::OutputLimitExceeded(OverflowedStreams::Stdout)
        );
        assert_eq!(truncating.stdout, strict.stdout);
        assert_eq!(truncating.disposition, Disposition::Succeeded);
    }

    // r[verify bounded_exec.cancellation.pre_spawn]
    #[cfg(unix)]
    #[test]
    fn timeout_tears_down_and_preexisting_cancellation_stops_before_spawn() {
        let mut timeout_request = request(NON_TERMINATING_SHELL_LOOP);
        timeout_request.limits.timeout_ms = SHORT_TIMEOUT_MS;
        let timeout_started = Instant::now();
        let timed_out = run(timeout_request).expect("timeout is a bounded observation");
        let cancellation = AtomicBool::new(true);
        let cancelled = run_with_cancellation(request(NON_TERMINATING_SHELL_LOOP), &cancellation)
            .expect("cancellation is a bounded observation");

        assert_eq!(timed_out.completion, Completion::TimedOut);
        assert_eq!(timed_out.disposition, Disposition::TimedOut);
        assert!(timeout_started.elapsed() < Duration::from_secs(TEST_EXECUTION_UPPER_BOUND_SECS));
        assert_eq!(cancelled.completion, Completion::Cancelled);
        assert_eq!(cancelled.disposition, Disposition::Cancelled);
        assert_eq!(cancelled.exit_code, None);
        assert_eq!(cancelled.signal, None);
        assert_eq!(cancelled.input.status, InputStatus::NotStarted);
    }

    #[cfg(unix)]
    #[test]
    fn process_group_cleanup_closes_descendant_held_pipes() {
        let script = format!(
            "({NON_TERMINATING_SHELL_LOOP}) & printf '{}'",
            String::from_utf8_lossy(EXPECTED_OUTPUT_BYTES)
        );
        let started = Instant::now();

        let output =
            run(request(&script)).expect("descendant is terminated after the direct child exits");

        assert_eq!(output.stdout.bytes, EXPECTED_OUTPUT_BYTES);
        assert_eq!(output.disposition, Disposition::Succeeded);
        assert!(started.elapsed() < Duration::from_secs(TEST_EXECUTION_UPPER_BOUND_SECS));
    }

    // r[verify bounded_exec.capture_capacity.pre_spawn]
    // r[verify bounded_exec.capture_capacity.verification]
    #[test]
    fn capture_preparation_failure_precedes_spawn_for_each_stream() {
        let mut stdout_failure = request("exit 0");
        stdout_failure.command.program = PathBuf::from("/missing/bounded-exec-fixture");
        stdout_failure.limits.stdout_max_bytes = usize::MAX;
        let stdout_error = run(stdout_failure).expect_err("stdout preparation must fail");

        let mut stderr_failure = request("exit 0");
        stderr_failure.command.program = PathBuf::from("/missing/bounded-exec-fixture");
        stderr_failure.limits.stderr_max_bytes = usize::MAX;
        let stderr_error = run(stderr_failure).expect_err("stderr preparation must fail");

        assert!(matches!(
            stdout_error,
            RunError::CapturePreparation {
                worker: Worker::Stdout,
                ..
            }
        ));
        assert!(matches!(
            stderr_error,
            RunError::CapturePreparation {
                worker: Worker::Stderr,
                ..
            }
        ));
    }

    #[test]
    fn malformed_requests_fail_before_spawn() {
        let mut empty_program = request("exit 0");
        empty_program.command.program = PathBuf::new();
        let mut relative_program = request("exit 0");
        relative_program.command.program = PathBuf::from("sh");
        let mut empty_current_directory = request("exit 0");
        empty_current_directory.command.current_dir = PathBuf::new();
        let mut relative_current_directory = request("exit 0");
        relative_current_directory.command.current_dir = PathBuf::from("relative");
        let mut duplicate_environment = request("exit 0");
        duplicate_environment.command.environment = vec![
            (OsString::from("DUPLICATE"), OsString::from("first")),
            (OsString::from("DUPLICATE"), OsString::from("second")),
        ];
        let mut oversized_input = request("exit 0");
        oversized_input.limits.stdin_max_bytes = SMALL_OUTPUT_MAX_BYTES;
        oversized_input.command.input = Input::Bytes(vec![0_u8; SMALL_OUTPUT_MAX_BYTES + 1]);

        assert!(matches!(run(empty_program), Err(RunError::EmptyProgram)));
        assert!(matches!(
            run(relative_program),
            Err(RunError::ProgramNotAbsolute)
        ));
        assert!(matches!(
            run(empty_current_directory),
            Err(RunError::EmptyCurrentDirectory)
        ));
        assert!(matches!(
            run(relative_current_directory),
            Err(RunError::CurrentDirectoryNotAbsolute)
        ));
        assert!(matches!(
            run(duplicate_environment),
            Err(RunError::DuplicateEnvironmentName)
        ));
        assert!(matches!(
            run(oversized_input),
            Err(RunError::InputLimitExceeded)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn stopped_capture_worker_returns_while_pipe_is_still_held_open() {
        use std::os::unix::net::UnixStream;

        const STOP_AFTER_MS: u64 = 50;
        const WORKER_RETURN_UPPER_BOUND_MS: u64 = 1_000;
        const EXHAUSTED_TEARDOWN_MS: u64 = 1;

        // The held peer models a descendant that escaped the owned process
        // group and keeps the write end of the capture pipe open.
        let (reader, _held_peer) = UnixStream::pair().expect("socket pair must open");
        let capture = CaptureAccumulator::prepare(TEST_OUTPUT_MAX_BYTES)
            .expect("fixture capacity must prepare");
        let worker = start_capture_worker(
            reader,
            capture,
            Worker::Stdout,
            Duration::from_millis(TEST_POLL_INTERVAL_MS),
            &mut OsWorkerSpawner,
        )
        .expect("capture worker thread must start");
        thread::sleep(Duration::from_millis(STOP_AFTER_MS));
        let started = Instant::now();

        let result = finish_worker(
            worker,
            TeardownBudget::new(EXHAUSTED_TEARDOWN_MS, TEST_POLL_INTERVAL_MS),
        );

        assert!(matches!(
            result,
            Err(RunError::WorkerTeardownTimedOut(Worker::Stdout))
        ));
        assert!(started.elapsed() < Duration::from_millis(WORKER_RETURN_UPPER_BOUND_MS));
    }

    #[cfg(unix)]
    #[test]
    fn process_group_exit_reports_the_reaped_exit_status() {
        const EXPECTED_EXIT_CODE: i32 = 7;

        let mut request = request(&format!("exit {EXPECTED_EXIT_CODE}"));
        request.outcome_policy = OutcomePolicy::new(vec![EXPECTED_EXIT_CODE], true, true)
            .expect("fixture outcome policy is valid");

        let output = run(request).expect("exited child is observed");

        assert_eq!(output.completion, Completion::Exited);
        assert_eq!(output.exit_code, Some(EXPECTED_EXIT_CODE));
        assert_eq!(output.signal, None);
        assert_eq!(output.disposition, Disposition::Succeeded);
        assert_eq!(
            output.input,
            InputDelivery {
                offered_bytes: 0,
                status: InputStatus::NullDevice,
            }
        );
    }

    // r[verify bounded_exec.cancellation.pre_spawn]
    // r[verify bounded_exec.cancellation.verification]
    #[cfg(unix)]
    #[test]
    fn preexisting_cancellation_never_spawns_while_the_control_does() {
        let directory = scratch_directory("pre-spawn");
        let cancellation = AtomicBool::new(true);
        let expected = ExecutionOutput {
            completion: Completion::Cancelled,
            exit_code: None,
            signal: None,
            input: InputDelivery {
                offered_bytes: UNREAD_SMALL_INPUT.len(),
                status: InputStatus::NotStarted,
            },
            stdout: empty_capture(),
            stderr: empty_capture(),
            disposition: Disposition::Cancelled,
        };

        for attempt in 0..PRE_SPAWN_PROOF_RUNS {
            let marker = directory.join(format!("cancelled-{attempt}"));
            let mut request = marker_request(MARKER_SCRIPT, &marker);
            request.command.input = Input::Bytes(UNREAD_SMALL_INPUT.to_vec());

            let output = run_with_cancellation(request, &cancellation)
                .expect("pre-spawn cancellation is a bounded observation");

            assert!(!marker.exists(), "attempt {attempt} started a child");
            assert_eq!(output, expected);
        }
        let control_marker = directory.join("control");
        let control = run_with_cancellation(
            marker_request(MARKER_SCRIPT, &control_marker),
            &AtomicBool::new(false),
        )
        .expect("control command runs");

        assert!(control_marker.exists(), "control must start a child");
        assert_eq!(control.disposition, Disposition::Succeeded);
        std::fs::remove_dir_all(&directory).expect("scratch directory must be removed");
    }

    // r[verify bounded_exec.cancellation.pre_spawn]
    #[test]
    fn invalid_requests_keep_their_errors_when_cancellation_is_raised() {
        let cancellation = AtomicBool::new(true);
        let mut empty_program = request("exit 0");
        empty_program.command.program = PathBuf::new();
        let mut oversized_input = request("exit 0");
        oversized_input.limits.stdin_max_bytes = SMALL_OUTPUT_MAX_BYTES;
        oversized_input.command.input = Input::Bytes(vec![0_u8; SMALL_OUTPUT_MAX_BYTES + 1]);
        let mut unpreparable_capture = request("exit 0");
        unpreparable_capture.limits.stdout_max_bytes = usize::MAX;

        assert!(matches!(
            run_with_cancellation(empty_program, &cancellation),
            Err(RunError::EmptyProgram)
        ));
        assert!(matches!(
            run_with_cancellation(oversized_input, &cancellation),
            Err(RunError::InputLimitExceeded)
        ));
        assert!(matches!(
            run_with_cancellation(unpreparable_capture, &cancellation),
            Err(RunError::CapturePreparation {
                worker: Worker::Stdout,
                ..
            })
        ));
    }

    // r[verify bounded_exec.cancellation.post_spawn]
    // r[verify bounded_exec.cancellation.verification]
    #[cfg(unix)]
    #[test]
    fn cancellation_after_spawn_tears_down_and_reports_the_reaped_signal() {
        let directory = scratch_directory("post-spawn");
        let marker = directory.join("started");
        let mut request = marker_request(
            &format!("{MARKER_SCRIPT}; {NON_TERMINATING_SHELL_LOOP}"),
            &marker,
        );
        request.limits.timeout_ms = LONG_TIMEOUT_MS;
        request.limits.stdin_max_bytes = PIPE_EXCEEDING_INPUT_BYTES;
        request.command.input = Input::Bytes(pipe_exceeding_input());
        let cancellation = AtomicBool::new(false);

        let output = thread::scope(|scope| {
            scope.spawn(|| {
                let started = Instant::now();
                while !marker.exists() && started.elapsed() < Duration::from_millis(LONG_TIMEOUT_MS)
                {
                    thread::sleep(Duration::from_millis(MARKER_WAIT_POLL_MS));
                }
                cancellation.store(true, Ordering::Release);
            });
            run_with_cancellation(request, &cancellation)
        })
        .expect("cancellation after spawn is a bounded observation");

        assert!(marker.exists(), "the child must have started");
        assert_eq!(output.completion, Completion::Cancelled);
        assert_eq!(output.disposition, Disposition::Cancelled);
        assert_eq!(output.exit_code, None);
        assert_eq!(
            output.signal,
            Some(nix::sys::signal::Signal::SIGKILL as i32)
        );
        assert_eq!(output.input, piped(PIPE_EXCEEDING_INPUT_BYTES));
        std::fs::remove_dir_all(&directory).expect("scratch directory must be removed");
    }

    // r[verify bounded_exec.input_delivery.reader_closed]
    // r[verify bounded_exec.input_delivery.verification]
    #[cfg(unix)]
    #[test]
    fn immediate_exit_with_pipe_exceeding_input_succeeds() {
        assert_immediate_exit_outcome(&pipe_exceeding_input());
    }

    // r[verify bounded_exec.input_delivery.reader_closed]
    // r[verify bounded_exec.input_delivery.verification]
    #[cfg(unix)]
    #[test]
    fn immediate_exit_with_small_input_matches_the_large_input_outcome() {
        let large = run(immediate_exit_request(&pipe_exceeding_input()))
            .expect("unread large input never fails a run");
        let small = run(immediate_exit_request(UNREAD_SMALL_INPUT))
            .expect("unread small input never fails a run");

        assert_eq!(small, immediate_exit_outcome(UNREAD_SMALL_INPUT.len()));
        assert_eq!(
            ExecutionOutput {
                input: small.input,
                ..large
            },
            small,
            "only the declared input size may differ"
        );
    }

    // r[verify bounded_exec.input_delivery.reader_closed]
    // r[verify bounded_exec.input_delivery.verification]
    #[cfg(unix)]
    #[test]
    fn immediate_exit_outcome_is_identical_in_sequence() {
        let large_input = pipe_exceeding_input();

        for _ in 0..SEQUENTIAL_UNREAD_RUNS {
            assert_immediate_exit_outcome(UNREAD_SMALL_INPUT);
            assert_immediate_exit_outcome(&large_input);
        }
    }

    // r[verify bounded_exec.input_delivery.reader_closed]
    // r[verify bounded_exec.input_delivery.verification]
    #[cfg(unix)]
    #[test]
    fn immediate_exit_outcome_is_identical_under_parallel_load() {
        let large_input = pipe_exceeding_input();

        let (small_leaks, large_leaks) = thread::scope(|scope| {
            let workers: Vec<_> = (0..STRESS_WORKERS)
                .map(|_| {
                    scope.spawn(|| {
                        let mut leaks = (0_usize, 0_usize);
                        for _ in 0..STRESS_RUNS_PER_WORKER {
                            leaks.0 +=
                                usize::from(immediate_exit_leaked_stdin_error(UNREAD_SMALL_INPUT));
                            leaks.1 += usize::from(immediate_exit_leaked_stdin_error(&large_input));
                        }
                        leaks
                    })
                })
                .collect();
            workers
                .into_iter()
                .map(|worker| worker.join().expect("stress worker must not panic"))
                .fold((0, 0), |total, leaks| {
                    (total.0 + leaks.0, total.1 + leaks.1)
                })
        });

        assert_eq!(
            (small_leaks, large_leaks),
            (0, 0),
            "leaked WriteStdin errors (small, large) over {} runs each",
            STRESS_WORKERS * STRESS_RUNS_PER_WORKER
        );
    }

    // r[verify bounded_exec.input_delivery.fact]
    // r[verify bounded_exec.input_delivery.verification]
    #[cfg(unix)]
    #[test]
    fn child_that_reads_all_input_reports_the_piped_fact() {
        let input = READ_TO_END_LINE.repeat(READ_TO_END_LINES);
        let mut request = request("while IFS= read -r line; do :; done; printf drained");
        request.limits.timeout_ms = LONG_TIMEOUT_MS;
        request.limits.stdin_max_bytes = input.len();
        request.command.input = Input::Bytes(input.clone());

        let output = run(request).expect("read-to-end command succeeds");

        assert_eq!(output.disposition, Disposition::Succeeded);
        assert_eq!(output.stdout.bytes, b"drained");
        assert_eq!(output.input, piped(input.len()));
    }

    /// Pollable writer that accepts a prefix, then returns a chosen fault.
    /// `None` models a zero-byte write.
    #[cfg(unix)]
    struct FaultyPipe {
        stream: std::os::unix::net::UnixStream,
        accept_before_fault: usize,
        accepted: usize,
        fault: Option<io::ErrorKind>,
    }

    #[cfg(unix)]
    impl std::os::fd::AsFd for FaultyPipe {
        fn as_fd(&self) -> std::os::fd::BorrowedFd<'_> {
            self.stream.as_fd()
        }
    }

    #[cfg(unix)]
    impl Write for FaultyPipe {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            let open = self.accept_before_fault - self.accepted;
            if open > 0 {
                let accepted = bytes.len().min(open);
                self.accepted += accepted;
                return Ok(accepted);
            }
            self.fault.map_or(Ok(0), |kind| Err(io::Error::from(kind)))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[cfg(unix)]
    fn write_through_fault(fault: Option<io::ErrorKind>) -> Result<(), RunError> {
        let (stream, _peer) =
            std::os::unix::net::UnixStream::pair().expect("socket pair must open");
        let pipe = FaultyPipe {
            stream,
            accept_before_fault: FAULT_ACCEPTED_BYTES,
            accepted: 0,
            fault,
        };
        write_stdin(
            pipe,
            &[INPUT_FILL_BYTE; FAULT_INPUT_BYTES],
            &AtomicBool::new(false),
            Duration::from_millis(TEST_POLL_INTERVAL_MS),
        )
    }

    // r[verify bounded_exec.input_delivery.reader_closed]
    // r[verify bounded_exec.input_delivery.write_errors]
    #[cfg(unix)]
    #[test]
    fn only_reader_closure_errors_end_writing_without_failing() {
        const READER_CLOSURE: [io::ErrorKind; 3] = [
            io::ErrorKind::BrokenPipe,
            io::ErrorKind::ConnectionReset,
            io::ErrorKind::ConnectionAborted,
        ];
        const OTHER_FAULTS: [io::ErrorKind; 4] = [
            io::ErrorKind::PermissionDenied,
            io::ErrorKind::InvalidInput,
            io::ErrorKind::OutOfMemory,
            io::ErrorKind::Other,
        ];

        for kind in READER_CLOSURE {
            assert!(
                matches!(write_through_fault(Some(kind)), Ok(())),
                "{kind:?} must end writing without failing"
            );
        }
        for kind in OTHER_FAULTS {
            let result = write_through_fault(Some(kind));
            assert!(
                matches!(
                    &result,
                    Err(RunError::Io {
                        operation: Operation::WriteStdin,
                        error,
                    }) if error.kind() == kind
                ),
                "{kind:?} must fail the writer, got {result:?}"
            );
        }
        assert!(matches!(
            write_through_fault(None),
            Err(RunError::Io {
                operation: Operation::WriteStdin,
                error,
            }) if error.kind() == io::ErrorKind::WriteZero
        ));
    }

    // r[verify bounded_exec.input_delivery.write_errors]
    #[cfg(unix)]
    #[test]
    fn stopped_stdin_writer_still_fails_while_the_reader_stays_open() {
        let (writer, _held_reader) =
            std::os::unix::net::UnixStream::pair().expect("socket pair must open");
        let input = pipe_exceeding_input();
        let stop = AtomicBool::new(false);

        let result = thread::scope(|scope| {
            let handle = scope.spawn(|| {
                write_stdin(
                    writer,
                    &input,
                    &stop,
                    Duration::from_millis(TEST_POLL_INTERVAL_MS),
                )
            });
            thread::sleep(Duration::from_millis(STOP_WRITER_AFTER_MS));
            stop.store(true, Ordering::Release);
            handle.join().expect("writer must not panic")
        });

        assert!(matches!(
            result,
            Err(RunError::WorkerTeardownTimedOut(Worker::Stdin))
        ));
    }

    // r[verify bounded_exec.input_delivery.verification]
    #[cfg(unix)]
    #[test]
    fn timeout_with_unread_input_reports_the_piped_fact() {
        let mut request = request(NON_TERMINATING_SHELL_LOOP);
        request.limits.timeout_ms = SHORT_TIMEOUT_MS;
        request.limits.stdin_max_bytes = PIPE_EXCEEDING_INPUT_BYTES;
        request.command.input = Input::Bytes(pipe_exceeding_input());

        let output = run(request).expect("timeout with unread input is an observation");

        assert_eq!(output.completion, Completion::TimedOut);
        assert_eq!(output.disposition, Disposition::TimedOut);
        assert_eq!(output.input, piped(PIPE_EXCEEDING_INPUT_BYTES));
    }

    // r[verify bounded_exec.input_delivery.verification]
    #[cfg(unix)]
    #[test]
    fn output_limit_disposition_is_independent_of_input_delivery() {
        let mut unread = request("printf 'bounded-output'");
        unread.limits.stdout_max_bytes = SMALL_OUTPUT_MAX_BYTES;
        unread.limits.stdin_max_bytes = PIPE_EXCEEDING_INPUT_BYTES;
        unread.command.input = Input::Bytes(pipe_exceeding_input());
        let mut echoing = request("while IFS= read -r line; do printf '%s\\n' \"$line\"; done");
        echoing.limits.stdout_max_bytes = SMALL_OUTPUT_MAX_BYTES;
        echoing.command.input = Input::Bytes(READ_TO_END_LINE.to_vec());

        let unread = run(unread).expect("unread output flood is an observation");
        let echoing = run(echoing).expect("echoed output flood is an observation");

        assert_eq!(
            unread.disposition,
            Disposition::OutputLimitExceeded(OverflowedStreams::Stdout)
        );
        assert_eq!(unread.input, piped(PIPE_EXCEEDING_INPUT_BYTES));
        assert_eq!(
            echoing.disposition,
            Disposition::OutputLimitExceeded(OverflowedStreams::Stdout)
        );
        assert_eq!(echoing.input, piped(READ_TO_END_LINE.len()));
    }

    #[cfg(unix)]
    mod worker_startup_tests {
        use std::error::Error;
        use std::path::Path;
        use std::sync::atomic::AtomicUsize;

        use nix::errno::Errno;
        use nix::sys::signal::Signal;
        use nix::sys::signal::kill;
        use nix::unistd::Pid;

        use super::*;

        const SHORT_TEARDOWN_TIMEOUT_MS: u64 = 100;
        const WORKER_SETTLE_MS: u64 = 25;
        const PID_WAIT_TIMEOUT_MS: u64 = 2_000;

        enum FaultBehavior {
            IoError(io::ErrorKind),
            Panic,
        }

        struct CountingFailSpawner {
            fail_on: Worker,
            behavior: FaultBehavior,
            alive_workers: Arc<AtomicUsize>,
            before_fail: Option<Box<dyn FnMut()>>,
        }

        impl CountingFailSpawner {
            fn new(
                fail_on: Worker,
                behavior: FaultBehavior,
                alive_workers: Arc<AtomicUsize>,
                before_fail: Option<Box<dyn FnMut()>>,
            ) -> Self {
                Self {
                    fail_on,
                    behavior,
                    alive_workers,
                    before_fail,
                }
            }
        }

        impl WorkerSpawner for CountingFailSpawner {
            fn spawn<F>(&mut self, worker: Worker, task: F) -> io::Result<thread::JoinHandle<()>>
            where
                F: FnOnce() + Send + 'static,
            {
                if worker == self.fail_on {
                    if let Some(hook) = self.before_fail.as_mut() {
                        hook();
                    }
                    return match self.behavior {
                        FaultBehavior::IoError(kind) => {
                            Err(io::Error::new(kind, "injected worker startup failure"))
                        }
                        FaultBehavior::Panic => panic!("injected worker startup unwind"),
                    };
                }
                let alive = Arc::clone(&self.alive_workers);
                alive.fetch_add(1, Ordering::AcqRel);
                let result = OsWorkerSpawner.spawn(worker, move || {
                    struct LiveWorkerGuard(Arc<AtomicUsize>);
                    impl Drop for LiveWorkerGuard {
                        fn drop(&mut self) {
                            self.0.fetch_sub(1, Ordering::AcqRel);
                        }
                    }
                    let _live = LiveWorkerGuard(alive);
                    task();
                });
                if result.is_err() {
                    self.alive_workers.fetch_sub(1, Ordering::AcqRel);
                }
                result
            }
        }

        struct ProcessCleanupGuard(Option<i32>);

        impl Drop for ProcessCleanupGuard {
            fn drop(&mut self) {
                if let Some(pid) = self.0.take() {
                    let _ = kill(Pid::from_raw(pid), Signal::SIGKILL);
                }
            }
        }

        fn wait_for_pid_file(path: &Path) -> i32 {
            let started = Instant::now();
            let timeout = Duration::from_millis(PID_WAIT_TIMEOUT_MS);
            loop {
                if let Ok(content) = std::fs::read_to_string(path) {
                    if content.ends_with('\n') {
                        if let Ok(pid) = content.trim().parse::<i32>() {
                            return pid;
                        }
                    }
                }
                assert!(
                    started.elapsed() < timeout,
                    "timed out waiting for PID file {}",
                    path.display()
                );
                thread::sleep(Duration::from_millis(MARKER_WAIT_POLL_MS));
            }
        }

        fn assert_reaped_immediately(pid: i32) {
            let status = kill(Pid::from_raw(pid), None);
            assert_eq!(
                status,
                Err(Errno::ESRCH),
                "expected direct child PID {pid} to be reaped immediately, got {status:?}"
            );
        }

        fn assert_terminated_within_budget(pid: i32) {
            let started = Instant::now();
            let timeout = Duration::from_millis(TEST_TEARDOWN_TIMEOUT_MS);
            loop {
                if kill(Pid::from_raw(pid), None) == Err(Errno::ESRCH) {
                    return;
                }
                assert!(
                    started.elapsed() < timeout,
                    "descendant PID {pid} remained alive past teardown budget"
                );
                thread::sleep(Duration::from_millis(MARKER_WAIT_POLL_MS));
            }
        }

        // r[verify bounded_exec.worker_startup.fallible]
        // r[verify bounded_exec.worker_startup.cleanup]
        // r[verify bounded_exec.worker_startup.verification]
        #[test]
        fn worker_start_failure_at_each_position_reaps_child_and_returns_typed_error() {
            const POSITIONS: [Worker; 3] = [Worker::Stdin, Worker::Stdout, Worker::Stderr];

            let directory = scratch_directory("worker-start-positions");
            for (index, position) in POSITIONS.into_iter().enumerate() {
                let pid_file = directory.join(format!("child-{index}.pid"));
                let mut req = marker_request(
                    &format!("printf '%s\\n' \"$$\" > \"$1\"; {NON_TERMINATING_SHELL_LOOP}"),
                    &pid_file,
                );
                req.limits.timeout_ms = LONG_TIMEOUT_MS;
                req.limits.stdin_max_bytes = TEST_STDIN_MAX_BYTES;
                req.command.input = Input::Bytes(UNREAD_SMALL_INPUT.to_vec());

                let alive_workers = Arc::new(AtomicUsize::new(0));
                let pid_wait_path = pid_file.clone();
                let mut spawner = CountingFailSpawner::new(
                    position,
                    FaultBehavior::IoError(io::ErrorKind::WouldBlock),
                    Arc::clone(&alive_workers),
                    Some(Box::new(move || {
                        let _pid = wait_for_pid_file(&pid_wait_path);
                    })),
                );

                let started = Instant::now();
                let error = run_with_spawner(req, None, &mut spawner)
                    .expect_err("injected worker startup failure must fail the run");
                let elapsed = started.elapsed();

                assert!(
                    elapsed < Duration::from_secs(TEST_EXECUTION_UPPER_BOUND_SECS),
                    "startup cleanup exceeded bound for {position:?}: {elapsed:?}"
                );
                assert!(
                    error.source().is_some(),
                    "WorkerStart must preserve the underlying OS error as Error::source()"
                );
                match error {
                    RunError::WorkerStart {
                        worker,
                        error: os_error,
                        cleanup_errors,
                    } => {
                        assert_eq!(worker, position);
                        assert_eq!(os_error.kind(), io::ErrorKind::WouldBlock);
                        assert!(
                            cleanup_errors.is_empty(),
                            "clean process-group teardown for {position:?} had cleanup errors: {cleanup_errors:?}"
                        );
                    }
                    other => panic!("expected WorkerStart for {position:?}, got {other:?}"),
                }
                let child_pid = wait_for_pid_file(&pid_file);
                assert_reaped_immediately(child_pid);
                assert_eq!(
                    alive_workers.load(Ordering::Acquire),
                    0,
                    "all started workers must finish before returning for {position:?}"
                );
            }
            std::fs::remove_dir_all(&directory).expect("scratch directory must be removed");
        }

        // r[verify bounded_exec.worker_startup.cleanup]
        // r[verify bounded_exec.worker_startup.verification]
        #[test]
        fn worker_start_failure_in_process_group_tears_down_descendants_holding_pipes() {
            let directory = scratch_directory("worker-start-pgroup");
            let child_pid_file = directory.join("child.pid");
            let descendant_pid_file = directory.join("descendant.pid");
            let mut req = request(&format!(
                "exec 3<&0; ({NON_TERMINATING_SHELL_LOOP}) <&3 & printf '%s\\n' \"$!\" > \"$2\"; printf '%s\\n' \"$$\" > \"$1\"; {NON_TERMINATING_SHELL_LOOP}"
            ));
            req.command.args.push(OsString::from("sh"));
            req.command.args.push(child_pid_file.as_os_str().to_owned());
            req.command
                .args
                .push(descendant_pid_file.as_os_str().to_owned());
            req.limits.timeout_ms = LONG_TIMEOUT_MS;
            req.limits.stdin_max_bytes = PIPE_EXCEEDING_INPUT_BYTES;
            req.command.input = Input::Bytes(pipe_exceeding_input());
            req.termination_scope = TerminationScope::ProcessGroup;

            let alive_workers = Arc::new(AtomicUsize::new(0));
            let wait_child = child_pid_file.clone();
            let wait_descendant = descendant_pid_file.clone();
            let mut spawner = CountingFailSpawner::new(
                Worker::Stderr,
                FaultBehavior::IoError(io::ErrorKind::OutOfMemory),
                Arc::clone(&alive_workers),
                Some(Box::new(move || {
                    let _descendant = wait_for_pid_file(&wait_descendant);
                    let _child = wait_for_pid_file(&wait_child);
                    thread::sleep(Duration::from_millis(WORKER_SETTLE_MS));
                })),
            );

            let started = Instant::now();
            let error = run_with_spawner(req, None, &mut spawner)
                .expect_err("injected stderr worker startup failure must fail the run");

            assert!(started.elapsed() < Duration::from_secs(TEST_EXECUTION_UPPER_BOUND_SECS));
            match error {
                RunError::WorkerStart {
                    worker,
                    error: os_error,
                    cleanup_errors,
                } => {
                    assert_eq!(worker, Worker::Stderr);
                    assert_eq!(os_error.kind(), io::ErrorKind::OutOfMemory);
                    assert!(
                        cleanup_errors.is_empty(),
                        "process-group teardown must close descendant pipes cleanly: {cleanup_errors:?}"
                    );
                }
                other => panic!("expected WorkerStart(Stderr), got {other:?}"),
            }
            let child_pid = wait_for_pid_file(&child_pid_file);
            let descendant_pid = wait_for_pid_file(&descendant_pid_file);
            assert_reaped_immediately(child_pid);
            assert_terminated_within_budget(descendant_pid);
            assert_eq!(alive_workers.load(Ordering::Acquire), 0);
            std::fs::remove_dir_all(&directory).expect("scratch directory must be removed");
        }

        // r[verify bounded_exec.worker_startup.cleanup]
        // r[verify bounded_exec.worker_startup.boundary]
        // r[verify bounded_exec.worker_startup.verification]
        #[test]
        fn worker_start_failure_records_cleanup_uncertainty_when_descendant_retains_pipes() {
            let directory = scratch_directory("worker-start-uncertainty");
            let child_pid_file = directory.join("child.pid");
            let descendant_pid_file = directory.join("descendant.pid");
            let mut req = request(&format!(
                "exec 3<&0; ({NON_TERMINATING_SHELL_LOOP}) <&3 & printf '%s\\n' \"$!\" > \"$2\"; printf '%s\\n' \"$$\" > \"$1\"; {NON_TERMINATING_SHELL_LOOP}"
            ));
            req.command.args.push(OsString::from("sh"));
            req.command.args.push(child_pid_file.as_os_str().to_owned());
            req.command
                .args
                .push(descendant_pid_file.as_os_str().to_owned());
            req.limits.timeout_ms = LONG_TIMEOUT_MS;
            req.limits.teardown_timeout_ms = SHORT_TEARDOWN_TIMEOUT_MS;
            req.limits.stdin_max_bytes = PIPE_EXCEEDING_INPUT_BYTES;
            req.command.input = Input::Bytes(pipe_exceeding_input());
            // Direct-child scope leaves the descendant alive holding stdin and
            // stdout open, modeling a scope/platform unable to terminate
            // descendants completely.
            req.termination_scope = TerminationScope::Child;

            let alive_workers = Arc::new(AtomicUsize::new(0));
            let wait_child = child_pid_file.clone();
            let wait_descendant = descendant_pid_file.clone();
            let mut spawner = CountingFailSpawner::new(
                Worker::Stderr,
                FaultBehavior::IoError(io::ErrorKind::OutOfMemory),
                Arc::clone(&alive_workers),
                Some(Box::new(move || {
                    let _descendant = wait_for_pid_file(&wait_descendant);
                    let _child = wait_for_pid_file(&wait_child);
                    thread::sleep(Duration::from_millis(WORKER_SETTLE_MS));
                })),
            );

            let started = Instant::now();
            let error = run_with_spawner(req, None, &mut spawner)
                .expect_err("injected stderr worker startup failure must fail the run");
            let elapsed = started.elapsed();

            let descendant_pid = wait_for_pid_file(&descendant_pid_file);
            let cleanup_descendant = ProcessCleanupGuard(Some(descendant_pid));
            let child_pid = wait_for_pid_file(&child_pid_file);

            assert!(elapsed < Duration::from_secs(TEST_EXECUTION_UPPER_BOUND_SECS));
            assert_reaped_immediately(child_pid);
            assert_eq!(
                alive_workers.load(Ordering::Acquire),
                0,
                "timed-out workers must still stop and join instead of detaching"
            );
            match error {
                RunError::WorkerStart {
                    worker,
                    error: os_error,
                    cleanup_errors,
                } => {
                    assert_eq!(worker, Worker::Stderr);
                    assert_eq!(os_error.kind(), io::ErrorKind::OutOfMemory);
                    assert!(
                        cleanup_errors.iter().any(|item| matches!(
                            item,
                            RunError::WorkerTeardownTimedOut(Worker::Stdin)
                        )),
                        "expected stdin teardown timeout in cleanup_errors, got {cleanup_errors:?}"
                    );
                    assert!(
                        cleanup_errors.iter().any(|item| matches!(
                            item,
                            RunError::WorkerTeardownTimedOut(Worker::Stdout)
                        )),
                        "expected stdout teardown timeout in cleanup_errors, got {cleanup_errors:?}"
                    );
                }
                other => panic!("expected WorkerStart(Stderr), got {other:?}"),
            }
            drop(cleanup_descendant);
            std::fs::remove_dir_all(&directory).expect("scratch directory must be removed");
        }

        // r[verify bounded_exec.worker_startup.cleanup]
        // r[verify bounded_exec.worker_startup.verification]
        #[test]
        fn worker_start_failure_does_not_deadlock_on_blocked_stdin_or_continuous_output() {
            const POSITIONS: [Worker; 2] = [Worker::Stdout, Worker::Stderr];

            let directory = scratch_directory("worker-start-flood");
            for (index, position) in POSITIONS.into_iter().enumerate() {
                let pid_file = directory.join(format!("flood-{index}.pid"));
                let mut req = marker_request(
                    "printf '%s\\n' \"$$\" > \"$1\"; while :; do printf 'stdout-flood\\n'; printf 'stderr-flood\\n' >&2; done",
                    &pid_file,
                );
                req.limits.timeout_ms = LONG_TIMEOUT_MS;
                req.limits.stdin_max_bytes = PIPE_EXCEEDING_INPUT_BYTES;
                req.limits.stdout_max_bytes = SMALL_OUTPUT_MAX_BYTES;
                req.limits.stderr_max_bytes = SMALL_OUTPUT_MAX_BYTES;
                req.command.input = Input::Bytes(pipe_exceeding_input());

                let alive_workers = Arc::new(AtomicUsize::new(0));
                let wait_path = pid_file.clone();
                let mut spawner = CountingFailSpawner::new(
                    position,
                    FaultBehavior::IoError(io::ErrorKind::WouldBlock),
                    Arc::clone(&alive_workers),
                    Some(Box::new(move || {
                        let _pid = wait_for_pid_file(&wait_path);
                        thread::sleep(Duration::from_millis(WORKER_SETTLE_MS));
                    })),
                );

                let started = Instant::now();
                let error = run_with_spawner(req, None, &mut spawner)
                    .expect_err("injected worker startup failure must fail the run");

                assert!(started.elapsed() < Duration::from_secs(TEST_EXECUTION_UPPER_BOUND_SECS));
                match error {
                    RunError::WorkerStart {
                        worker,
                        cleanup_errors,
                        ..
                    } => {
                        assert_eq!(worker, position);
                        assert!(
                            cleanup_errors.is_empty(),
                            "cleanup under blocked stdin / flooded output recorded errors: {cleanup_errors:?}"
                        );
                    }
                    other => panic!("expected WorkerStart({position:?}), got {other:?}"),
                }
                let child_pid = wait_for_pid_file(&pid_file);
                assert_reaped_immediately(child_pid);
                assert_eq!(alive_workers.load(Ordering::Acquire), 0);
            }
            std::fs::remove_dir_all(&directory).expect("scratch directory must be removed");
        }

        // r[verify bounded_exec.worker_startup.unwind]
        // r[verify bounded_exec.worker_startup.verification]
        #[test]
        fn worker_start_unwind_reaps_child_and_joins_started_workers() {
            let directory = scratch_directory("worker-start-unwind");
            let pid_file = directory.join("unwind.pid");
            let mut req = marker_request(
                &format!("printf '%s\\n' \"$$\" > \"$1\"; {NON_TERMINATING_SHELL_LOOP}"),
                &pid_file,
            );
            req.limits.timeout_ms = LONG_TIMEOUT_MS;
            req.limits.stdin_max_bytes = PIPE_EXCEEDING_INPUT_BYTES;
            req.command.input = Input::Bytes(pipe_exceeding_input());

            let alive_workers = Arc::new(AtomicUsize::new(0));
            let wait_path = pid_file.clone();
            let mut spawner = CountingFailSpawner::new(
                Worker::Stderr,
                FaultBehavior::Panic,
                Arc::clone(&alive_workers),
                Some(Box::new(move || {
                    let _pid = wait_for_pid_file(&wait_path);
                    thread::sleep(Duration::from_millis(WORKER_SETTLE_MS));
                })),
            );

            let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                run_with_spawner(req, None, &mut spawner)
            }));

            assert!(caught.is_err(), "injected unwind must propagate cleanly");
            let child_pid = wait_for_pid_file(&pid_file);
            assert_reaped_immediately(child_pid);
            assert_eq!(
                alive_workers.load(Ordering::Acquire),
                0,
                "unwind guard must finish started workers instead of detaching them"
            );
            std::fs::remove_dir_all(&directory).expect("scratch directory must be removed");
        }

        // r[verify bounded_exec.worker_startup.cleanup]
        #[test]
        fn missing_pipe_reaps_child_and_reports_missing_pipe() {
            let directory = scratch_directory("missing-pipe-reap");
            let pid_file = directory.join("missing-pipe.pid");
            let req = marker_request(
                &format!("printf '%s\\n' \"$$\" > \"$1\"; {NON_TERMINATING_SHELL_LOOP}"),
                &pid_file,
            );
            let mut command = prepare_command(&req.command, req.termination_scope);
            let mut child = command.spawn().expect("fixture child must spawn");
            let _pid = wait_for_pid_file(&pid_file);
            // Simulate a missing stderr pipe before handing child to StartupGuard.
            drop(child.stderr.take());

            let mut guard = StartupGuard::new(child, req.termination_scope, req.limits);
            guard.take_stdout().expect("stdout pipe is present");
            let error = guard
                .take_stderr()
                .expect_err("missing stderr pipe must fail");

            assert!(matches!(error, RunError::MissingPipe(Worker::Stderr)));
            let child_pid = wait_for_pid_file(&pid_file);
            assert_reaped_immediately(child_pid);
            std::fs::remove_dir_all(&directory).expect("scratch directory must be removed");
        }
    }
}
