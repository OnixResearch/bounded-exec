# Worker Startup Specification

## Purpose

Defines the `worker-startup` capability for fallible I/O worker creation and owned teardown when startup fails after child creation.

## Requirements

### Requirement: I/O worker startup is fallible and typed

r[bounded_exec.worker_startup.fallible] The shell MUST create stdin, stdout, and stderr I/O worker threads using fallible thread creation (`std::thread::Builder::spawn`) rather than panicking `std::thread::spawn`. When thread creation fails, the shell MUST return a typed `RunError::WorkerStart` identifying the failed worker (`Stdin`, `Stdout`, or `Stderr`), preserving the original OS `io::Error` (including as `Error::source()`), and recording any cleanup errors.

#### Scenario: Worker thread creation fails at any position

- GIVEN a spawned long-running child and an OS thread-creation failure at the stdin, stdout, or stderr worker position
- WHEN execution startup attempts to create that worker
- THEN execution MUST NOT panic
- AND it MUST return `RunError::WorkerStart` identifying that worker and preserving the original OS error

### Requirement: Partial startup failure tears down owned processes and started workers under a shared budget

r[bounded_exec.worker_startup.cleanup] Immediately after `Command::spawn()` succeeds, the shell MUST take armed ownership of the child, its termination scope, remaining pipes, and started workers, disarming ownership only when all resources are handed to the active execution loop. On partial startup failure, the shell MUST close unused pipes, terminate and reap the child according to its termination scope before waiting on workers, and finish every already-started worker under one shared teardown budget. Any child or worker teardown failure MUST be recorded explicitly in `RunError::WorkerStart::cleanup_errors` rather than dropped or masked.

#### Scenario: Process-group descendants hold pipes when startup fails

- GIVEN a spawned child in `TerminationScope::ProcessGroup` with a descendant retaining pipes, and a worker-start failure after earlier workers have started
- WHEN partial startup cleanup runs
- THEN the child and process-group descendant MUST be terminated and the direct child reaped within the teardown policy
- AND all already-started workers MUST finish and join cleanly with no cleanup errors

#### Scenario: An escaped descendant retains pipes when startup fails

- GIVEN a spawned child whose descendant remains outside the terminated scope and retains stdin and stdout pipes, and a stderr worker-start failure
- WHEN partial startup cleanup runs
- THEN the direct child MUST be terminated and reaped
- AND the already-started workers MUST observe teardown stop and join under the shared teardown budget without detaching
- AND `RunError::WorkerStart::cleanup_errors` MUST explicitly record the bounded worker teardown timeouts

#### Scenario: Blocked stdin or continuous output does not deadlock startup cleanup

- GIVEN a spawned child that ignores pipe-exceeding stdin and floods stdout and stderr continuously, and a stdout or stderr worker-start failure
- WHEN partial startup cleanup runs
- THEN cleanup MUST terminate and reap the child before waiting on started workers
- AND cleanup MUST complete promptly without deadlocking

### Requirement: Unwinding during startup triggers non-panicking RAII cleanup

r[bounded_exec.worker_startup.unwind] The startup guard MUST implement a non-panicking `Drop` fallback so that an unexpected unwind between `Command::spawn()` and handoff to active execution still closes unused pipes, terminates and reaps the owned child, and finishes already-started workers.

#### Scenario: Startup unwinds after earlier workers have started

- GIVEN a spawned child with started stdin and stdout workers and a panic during stderr worker startup
- WHEN unwinding drops the armed startup guard
- THEN `Drop` MUST NOT double-panic
- AND the direct child MUST be reaped and already-started workers joined

### Requirement: Startup cleanup boundaries remain explicit

r[bounded_exec.worker_startup.boundary] Scope-exit cleanup MUST NOT claim durability across process abort, `SIGKILL`, or failing destructors, and MUST keep capture preparation before spawn with the existing `no_std` core boundary unchanged. Where a scope or platform cannot terminate descendants retaining pipes, cleanup uncertainty MUST be reported explicitly via teardown errors.

#### Scenario: Consumer inspects a worker-start failure

- GIVEN a `RunError::WorkerStart` returned by `run` or `run_with_cancellation`
- WHEN the consumer inspects `cleanup_errors`
- THEN an empty list MUST reflect clean child reaping and worker joining under the configured scope
- AND a non-empty list MUST expose every observed child or worker teardown failure

### Requirement: Worker startup failure paths are verified deterministically

r[bounded_exec.worker_startup.verification] Verification MUST inject worker-creation failures through a deterministic internal seam at stdin, stdout, and stderr positions against real long-running children, covering process-group descendants holding pipes, retained-pipe cleanup uncertainty, blocked stdin with continuously flooded output, startup unwinding, and missing-pipe teardown, asserting immediate child reaping and zero detached worker threads.

#### Scenario: Deterministic worker-startup tests run

- GIVEN the shell test suite with injected worker-start faults
- WHEN tests execute
- THEN every injected failure MUST return its typed error with immediate child reaping and zero remaining live worker threads
