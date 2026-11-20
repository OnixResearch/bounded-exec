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
    /// Bounded standard-input delivery facts.
    pub input: InputDelivery,
    /// Bounded stdout capture.
    pub stdout: CapturedOutput,
    /// Bounded stderr capture.
    pub stderr: CapturedOutput,
    /// Product-neutral classification under the supplied outcome policy.
    pub disposition: Disposition,
}

/// Standard-input delivery facts for one execution.
///
/// Written bytes were accepted by the stdin pipe. They are not proof that the
/// child read, parsed, or acted on any byte.
// r[impl bounded_exec.input_delivery.fact]
// r[impl bounded_exec.input_delivery.boundary]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InputDelivery {
    /// Bytes the caller supplied through [`Input::Bytes`]; zero for [`Input::Null`].
    pub offered_bytes: usize,
    /// Bytes written into the stdin pipe, never more than `offered_bytes`.
    pub written_bytes: usize,
    /// How delivery ended.
    pub status: InputStatus,
}

/// How standard-input delivery ended.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputStatus {
    /// No child was created, so no byte was written.
    NotStarted,
    /// Standard input was the null device; no byte was offered.
    NullDevice,
    /// Every offered byte was written, then the runner closed the pipe.
    AllWritten,
    /// The read end closed before every offered byte was written, because
    /// the child closed it, exited, or was terminated. This is a delivery
    /// fact, not a run failure.
    ReaderClosed,
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
            Self::Io { error, .. } => Some(error),
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
        let input = InputDelivery {
            offered_bytes: offered_input_bytes(&command_spec.input),
            written_bytes: 0,
            status: InputStatus::NotStarted,
        };
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
    let mut child = command.spawn().map_err(|error| RunError::Io {
        operation: Operation::Spawn,
        error,
    })?;

    let stdin = if matches!(&command_spec.input, Input::Bytes(_)) {
        Some(take_required_stdin(&mut child, termination_scope, limits)?)
    } else {
        None
    };
    let stdout = take_required_stdout(&mut child, termination_scope, limits)?;
    let stderr = take_required_stderr(&mut child, termination_scope, limits)?;
    let poll_interval = Duration::from_millis(limits.poll_interval_ms);
    let stdin_worker = start_stdin_worker(stdin, command_spec.input, poll_interval);
    let stdout_worker = start_capture_worker(
        stdout,
        prepared_captures.stdout,
        Worker::Stdout,
        poll_interval,
    );
    let stderr_worker = start_capture_worker(
        stderr,
        prepared_captures.stderr,
        Worker::Stderr,
        poll_interval,
    );

    // Remaining process-group members are terminated inside
    // `wait_for_completion`, before the direct child is reaped, so the group
    // identifier cannot be recycled between exit and `killpg`.
    let (completion, exit_status) =
        wait_for_completion(&mut child, termination_scope, limits, cancellation)?;

    let teardown_budget = TeardownBudget::new(limits.teardown_timeout_ms, limits.poll_interval_ms);
    let input_result = match stdin_worker {
        Some(worker) => finish_worker(worker, teardown_budget),
        None => Ok(InputDelivery {
            offered_bytes: 0,
            written_bytes: 0,
            status: InputStatus::NullDevice,
        }),
    };
    let stdout_result = finish_worker(stdout_worker, teardown_budget);
    let stderr_result = finish_worker(stderr_worker, teardown_budget);
    let input = input_result?;
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

fn take_required_stdin(
    child: &mut Child,
    scope: TerminationScope,
    limits: ExecutionLimits,
) -> Result<std::process::ChildStdin, RunError> {
    let Some(stdin) = child.stdin.take() else {
        terminate_and_reap(child, scope, limits)?;
        return Err(RunError::MissingPipe(Worker::Stdin));
    };
    Ok(stdin)
}

fn take_required_stdout(
    child: &mut Child,
    scope: TerminationScope,
    limits: ExecutionLimits,
) -> Result<std::process::ChildStdout, RunError> {
    let Some(stdout) = child.stdout.take() else {
        terminate_and_reap(child, scope, limits)?;
        return Err(RunError::MissingPipe(Worker::Stdout));
    };
    Ok(stdout)
}

fn take_required_stderr(
    child: &mut Child,
    scope: TerminationScope,
    limits: ExecutionLimits,
) -> Result<std::process::ChildStderr, RunError> {
    let Some(stderr) = child.stderr.take() else {
        terminate_and_reap(child, scope, limits)?;
        return Err(RunError::MissingPipe(Worker::Stderr));
    };
    Ok(stderr)
}

fn start_stdin_worker(
    stdin: Option<std::process::ChildStdin>,
    input: Input,
    poll_interval: Duration,
) -> Option<WorkerHandle<InputDelivery>> {
    let (Some(stdin), Input::Bytes(bytes)) = (stdin, input) else {
        return None;
    };
    let (sender, receiver) = mpsc::sync_channel(WORKER_CHANNEL_CAPACITY);
    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = Arc::clone(&stop);
    let worker_thread = thread::spawn(move || {
        let result = write_stdin(stdin, &bytes, &worker_stop, poll_interval);
        let _send_result = sender.send(result);
    });
    Some(WorkerHandle {
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
) -> Result<InputDelivery, RunError>
where
    W: Write + WorkerPipe,
{
    let write_error = |error| RunError::Io {
        operation: Operation::WriteStdin,
        error,
    };
    let delivery = |remaining: &[u8], status| InputDelivery {
        offered_bytes: bytes.len(),
        written_bytes: bytes.len() - remaining.len(),
        status,
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
            Err(error) if is_reader_closure(&error) => {
                return Ok(delivery(remaining, InputStatus::ReaderClosed));
            }
            Err(error) => return Err(write_error(error)),
        }
    }
    stdin.flush().map_err(write_error)?;
    Ok(delivery(remaining, InputStatus::AllWritten))
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
fn start_capture_worker<R>(
    reader: R,
    capture: CaptureAccumulator,
    worker: Worker,
    poll_interval: Duration,
) -> WorkerHandle<CapturedOutput>
where
    R: Read + WorkerPipe + Send + 'static,
{
    let (sender, receiver) = mpsc::sync_channel(WORKER_CHANNEL_CAPACITY);
    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = Arc::clone(&stop);
    let worker_thread = thread::spawn(move || {
        let result = read_bounded(reader, capture, worker, &worker_stop, poll_interval);
        let _send_result = sender.send(result);
    });
    WorkerHandle {
        worker,
        receiver,
        thread: worker_thread,
        stop,
    }
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
    terminate_owned(child, scope)?;
    let started = Instant::now();
    let timeout = Duration::from_millis(limits.teardown_timeout_ms);
    let poll_interval = Duration::from_millis(limits.poll_interval_ms);
    loop {
        let status = child.try_wait().map_err(|error| RunError::Io {
            operation: Operation::Reap,
            error,
        })?;
        if status.is_some() {
            return Ok(status);
        }
        let elapsed = started.elapsed();
        if elapsed >= timeout {
            return Err(RunError::ChildTeardownTimedOut);
        }
        thread::sleep(poll_interval.min(timeout.saturating_sub(elapsed)));
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
    /// Larger than any default pipe or socket buffer, so an unread write
    /// cannot complete.
    const PIPE_EXCEEDING_INPUT_BYTES: usize = 1 << 20;
    const INPUT_FILL_BYTE: u8 = b'x';
    const READ_TO_END_LINE: &[u8] = b"bounded\n";
    /// 128 KiB of lines, twice the default Linux pipe buffer, so complete
    /// delivery requires the child to read.
    const READ_TO_END_LINES: usize = 16_384;
    const SEQUENTIAL_UNREAD_RUNS: usize = 64;
    const PARALLEL_UNREAD_WORKERS: usize = 8;
    const PARALLEL_UNREAD_RUNS_PER_WORKER: usize = 16;
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

    fn assert_piped_delivery_consistent(delivery: InputDelivery, offered_bytes: usize) {
        assert_eq!(delivery.offered_bytes, offered_bytes);
        match delivery.status {
            InputStatus::AllWritten => assert_eq!(delivery.written_bytes, offered_bytes),
            InputStatus::ReaderClosed => assert!(delivery.written_bytes < offered_bytes),
            other => panic!("unexpected piped delivery status {other:?}"),
        }
    }

    fn assert_unread_input_succeeds(input: &[u8]) -> InputDelivery {
        let mut request = request("exit 0");
        request.limits.stdin_max_bytes = PIPE_EXCEEDING_INPUT_BYTES;
        request.command.input = Input::Bytes(input.to_vec());

        let output = run(request).expect("unread input never fails a run");

        assert_eq!(output.completion, Completion::Exited);
        assert_eq!(output.exit_code, Some(SUCCESS_EXIT_CODE));
        assert_eq!(output.disposition, Disposition::Succeeded);
        assert_piped_delivery_consistent(output.input, input.len());
        output.input
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
        assert_eq!(
            output.input,
            InputDelivery {
                offered_bytes: b"input\n".len(),
                written_bytes: b"input\n".len(),
                status: InputStatus::AllWritten,
            }
        );
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
        );
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
                written_bytes: 0,
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
                written_bytes: 0,
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
        assert_eq!(output.input.status, InputStatus::ReaderClosed);
        assert_piped_delivery_consistent(output.input, PIPE_EXCEEDING_INPUT_BYTES);
        std::fs::remove_dir_all(&directory).expect("scratch directory must be removed");
    }

    // r[verify bounded_exec.input_delivery.reader_closed]
    // r[verify bounded_exec.input_delivery.verification]
    #[cfg(unix)]
    #[test]
    fn unread_input_never_fails_an_exited_run_in_sequence() {
        let large_input = pipe_exceeding_input();

        for _ in 0..SEQUENTIAL_UNREAD_RUNS {
            assert_unread_input_succeeds(UNREAD_SMALL_INPUT);
            let large = assert_unread_input_succeeds(&large_input);
            assert_eq!(large.status, InputStatus::ReaderClosed);
        }
    }

    // r[verify bounded_exec.input_delivery.reader_closed]
    // r[verify bounded_exec.input_delivery.verification]
    #[cfg(unix)]
    #[test]
    fn unread_input_never_fails_an_exited_run_under_parallel_load() {
        let large_input = pipe_exceeding_input();

        thread::scope(|scope| {
            for _ in 0..PARALLEL_UNREAD_WORKERS {
                scope.spawn(|| {
                    for _ in 0..PARALLEL_UNREAD_RUNS_PER_WORKER {
                        assert_unread_input_succeeds(UNREAD_SMALL_INPUT);
                        let large = assert_unread_input_succeeds(&large_input);
                        assert_eq!(large.status, InputStatus::ReaderClosed);
                    }
                });
            }
        });
    }

    // r[verify bounded_exec.input_delivery.fact]
    // r[verify bounded_exec.input_delivery.verification]
    #[cfg(unix)]
    #[test]
    fn child_that_reads_all_input_reports_every_byte_written() {
        let input = READ_TO_END_LINE.repeat(READ_TO_END_LINES);
        let mut request = request("while IFS= read -r line; do :; done; printf drained");
        request.limits.timeout_ms = LONG_TIMEOUT_MS;
        request.limits.stdin_max_bytes = input.len();
        request.command.input = Input::Bytes(input.clone());

        let output = run(request).expect("read-to-end command succeeds");

        assert_eq!(output.disposition, Disposition::Succeeded);
        assert_eq!(output.stdout.bytes, b"drained");
        assert_eq!(
            output.input,
            InputDelivery {
                offered_bytes: input.len(),
                written_bytes: input.len(),
                status: InputStatus::AllWritten,
            }
        );
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
    fn write_through_fault(fault: Option<io::ErrorKind>) -> Result<InputDelivery, RunError> {
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
    fn only_reader_closure_errors_end_delivery_without_failing() {
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
            assert_eq!(
                write_through_fault(Some(kind)).expect("reader closure is a delivery fact"),
                InputDelivery {
                    offered_bytes: FAULT_INPUT_BYTES,
                    written_bytes: FAULT_ACCEPTED_BYTES,
                    status: InputStatus::ReaderClosed,
                }
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
    fn timeout_with_unread_input_still_reports_reader_closure() {
        let mut request = request(NON_TERMINATING_SHELL_LOOP);
        request.limits.timeout_ms = SHORT_TIMEOUT_MS;
        request.limits.stdin_max_bytes = PIPE_EXCEEDING_INPUT_BYTES;
        request.command.input = Input::Bytes(pipe_exceeding_input());

        let output = run(request).expect("timeout with unread input is an observation");

        assert_eq!(output.completion, Completion::TimedOut);
        assert_eq!(output.disposition, Disposition::TimedOut);
        assert_eq!(output.input.status, InputStatus::ReaderClosed);
        assert_piped_delivery_consistent(output.input, PIPE_EXCEEDING_INPUT_BYTES);
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
        assert_eq!(unread.input.status, InputStatus::ReaderClosed);
        assert_piped_delivery_consistent(unread.input, PIPE_EXCEEDING_INPUT_BYTES);
        assert_eq!(
            echoing.disposition,
            Disposition::OutputLimitExceeded(OverflowedStreams::Stdout)
        );
        assert_eq!(
            echoing.input,
            InputDelivery {
                offered_bytes: READ_TO_END_LINE.len(),
                written_bytes: READ_TO_END_LINE.len(),
                status: InputStatus::AllWritten,
            }
        );
    }
}
