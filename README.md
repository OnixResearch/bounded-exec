# bounded-exec

Product-neutral bounded child-process execution for Rust systems code.

The repository separates deterministic policy from operating-system effects:

- `bounded-exec-core` is `no_std` plus `alloc`. It validates explicit limits, owns deterministic bounded-prefix retention, and classifies execution facts under consumer-owned exit and truncation policy.
- `bounded-exec` is the standard-library shell. It spawns argv directly, applies explicit environment behavior, writes bounded input, drains stdout and stderr concurrently with bounded retention, enforces deadlines and cancellation, and tears down owned processes.

On Unix, process-group scope places the child in a fresh group and terminates remaining descendants before capture workers are joined. Other platforms currently fall back to direct-child termination and make no descendant-teardown claim.

## Boundary

This repository owns process mechanics only. Consumers retain:

- executable discovery, identity verification, and authorization;
- sandbox, capability, seccomp, namespace, and network policy;
- remote execution and transport protocols;
- output parsing, redaction, and product diagnostics;
- receipts, evidence roles, release readiness, and lifecycle decisions.

A successful `bounded-exec` result proves only the returned observation under the supplied request and host implementation. It does not prove sandboxing, hermeticity, child correctness, authorization, platform equivalence, or release eligibility.

## Example

```rust
use std::ffi::OsString;
use std::path::PathBuf;

use bounded_exec::{
    CommandSpec, EnvironmentMode, ExecutionLimits, Input, OutcomePolicy,
    RunRequest, TerminationScope, run,
};

const TIMEOUT_MS: u64 = 1_000;
const STDIN_MAX_BYTES: usize = 1_024;
const OUTPUT_MAX_BYTES: usize = 4_096;
const POLL_INTERVAL_MS: u64 = 10;
const TEARDOWN_TIMEOUT_MS: u64 = 500;
const SUCCESS_EXIT_CODE: i32 = 0;

let request = RunRequest {
    command: CommandSpec {
        program: PathBuf::from("/usr/bin/printf"),
        args: vec![OsString::from("bounded\n")],
        current_dir: PathBuf::from("/"),
        environment_mode: EnvironmentMode::Clear,
        environment: Vec::new(),
        input: Input::Null,
    },
    limits: ExecutionLimits {
        timeout_ms: TIMEOUT_MS,
        stdin_max_bytes: STDIN_MAX_BYTES,
        stdout_max_bytes: OUTPUT_MAX_BYTES,
        stderr_max_bytes: OUTPUT_MAX_BYTES,
        poll_interval_ms: POLL_INTERVAL_MS,
        teardown_timeout_ms: TEARDOWN_TIMEOUT_MS,
    },
    termination_scope: TerminationScope::ProcessGroup,
    outcome_policy: OutcomePolicy::new(vec![SUCCESS_EXIT_CODE], true, true)?,
};
let observation = run(request)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Resource-watermark admission

<!-- r[impl bounded_exec.watermark.policy] -->
<!-- r[impl bounded_exec.watermark.boundary] -->

`bounded-exec-core` computes an admission decision from an explicit observation, low watermark, high watermark, and hysteresis state.

Admission becomes deferred above the high watermark. It remains deferred until the observation reaches the low watermark.

The caller retains these functions:

- resource observation
- scheduling and resource enforcement
- process setup and execution
- sandbox policy and enforcement
- evidence and release decisions

The Celld pressure-shedding mechanism is a bounded design reference only. Bounded Exec does not claim parity or implementation equivalence.

## Prepared output capture

<!-- r[impl bounded_exec.capture_capacity.preparation] -->
<!-- r[impl bounded_exec.capture_capacity.boundary] -->

The shell reserves complete stdout and stderr retention capacity after request validation. It does this before child creation. If either reservation fails, the request returns a stream-specific error and starts no child. The capture workers receive the prepared buffers by value. They retain the declared prefix without later vector growth.

Successful preparation proves only that the selected output-retention capacity was available at that time. It does not prove child execution, sandboxing, fixed latency, later host-memory stability, or process-wide zero allocation. Callers still own retention limits and execution policy.

## Cancellation

<!-- r[impl bounded_exec.cancellation.pre_spawn] -->
<!-- r[impl bounded_exec.cancellation.post_spawn] -->

`run_with_cancellation` first reads the flag after request validation and capture preparation, before child creation. It then reads the flag on every poll. Invalid requests and failed capture preparation keep their errors. If the flag is already raised, no child is created. The result is `Completion::Cancelled` and `Disposition::Cancelled`, with no exit code or signal, empty captures, and input status `NotStarted`.

If the flag is raised after spawn, the poll loop tears down the owned processes, reaps the direct child, and reports the reaped termination facts. A flag raised between the pre-spawn read and spawn is observed by the poll loop and handled the same way, so a child can start in that window.

## Input delivery

<!-- r[impl bounded_exec.input_delivery.fact] -->
<!-- r[impl bounded_exec.input_delivery.reader_closed] -->
<!-- r[impl bounded_exec.input_delivery.boundary] -->

`ExecutionOutput::input` reports `offered_bytes` and an `InputStatus`:

- `NotStarted`: no child was created.
- `NullDevice`: input was `Input::Null`.
- `Piped`: the offered bytes were offered through a stdin pipe, which the runner closed.

Both facts are fixed by the request and by whether a child was created. The same request and child behavior always produce the same observation, however the stdin writer is scheduled.

A child may exit, close stdin, or be stopped without reading its input. The writer may then see broken pipe, connection reset, or connection aborted. Whether it does depends on scheduling: small input can fit in the pipe buffer before the child exits, while input larger than the buffer always meets the closed pipe. Reader closure therefore ends writing without failing the run in every completion, including a normal exit, and leaves no trace in the observation. The run is classified only by completion, exit code, and output policy. Other stdin write errors, zero-byte writes, and writers stopped by teardown still fail with their typed errors.

Bounded Exec does not observe stdin consumption. It does not report how many bytes the pipe accepted, because that count depends on scheduling, and even fully written input may sit unread in the pipe buffer. `Piped` does not prove that the child read, parsed, or acted on any byte. A consumer that needs proof of consumption must obtain a child-side acknowledgement, such as a byte count or digest in the child's own output or protocol.

## Fallible worker startup and owned teardown

<!-- r[impl bounded_exec.worker_startup.fallible] -->
<!-- r[impl bounded_exec.worker_startup.cleanup] -->
<!-- r[impl bounded_exec.worker_startup.unwind] -->
<!-- r[impl bounded_exec.worker_startup.boundary] -->

Immediately after `Command::spawn()` succeeds, the shell takes armed ownership of the child, its termination scope, its pipes, and every started I/O worker. Stdin, stdout, and stderr workers are created through fallible `std::thread::Builder::spawn` calls rather than panicking `std::thread::spawn`.

If worker creation fails at any position, the shell closes unused pipes, terminates and reaps the child according to its termination scope before waiting on already-started workers, and finishes those workers under one shared teardown budget. It returns `RunError::WorkerStart`, identifying the failed worker, preserving the original OS `io::Error` (and exposing it through `std::error::Error::source()`), and recording any child or worker teardown failures in `cleanup_errors` when cleanup cannot establish complete teardown (for example, when a descendant outside the terminated scope retains pipes). A non-panicking `Drop` guard provides the same cleanup on unexpected unwinding before ownership transfers to active execution. Scope-exit cleanup cannot run under process abort, `SIGKILL`, or a failing destructor.

## Source publication

The canonical Radicle project is `rad:zG25DRmacxYQcXzZmLKj8tmgYbEG`. Its
read-only HTTPS adapter, `https://git.onix.computer/zG25DRmacxYQcXzZmLKj8tmgYbEG.git`,
and seed admission are pending. The previous identity,
`rad:z2CpqLFpdP36fZXYUK5ZNWxMibpCo`, is frozen at canonical `main`
`10e2ac43ca27f1dcadbe67a654cd185f60c0c3d3` because two of its three delegate
keys are lost. It stays served read-only at
`https://git.onix.computer/z2CpqLFpdP36fZXYUK5ZNWxMibpCo.git` for consumers
pinned to the reviewed pilot source `29dac88ecded94457572db3fdfaaaab95fa91525`.
GitHub is a transition mirror, not an admitted dependency fallback.

See [`docs/radicle-publication.md`](docs/radicle-publication.md) for clone,
exact-revision consumption, outage, rollback, evidence, and non-claim details.

## Verification

```sh
nix develop -c cargo fmt --all -- --check
nix develop -c cargo test --workspace
nix develop -c cargo clippy --workspace --all-targets -- -D warnings
nix develop -c cargo check -p bounded-exec-core --target wasm32-unknown-unknown
nix flake check -L
```

Positive coverage includes explicit environment/input round trips, accepted exits, read-to-end input delivery, and a spawn control. Negative coverage includes malformed requests, bounded output floods, deadlines, pre-spawn cancellation proven by marker files over repeated runs, cancellation after spawn, unread input repeated sequentially and under parallel load, injected non-closure stdin errors, stopped writers, descendant-held pipe cleanup, and injected I/O worker-startup failures and unwinds across stdin, stdout, and stderr positions.

See [`PILOT.md`](PILOT.md) for the disposable two-consumer migration evidence,
[`ADOPTION.md`](ADOPTION.md) for the authoritative consumer cutover gate,
[`docs/radicle-publication.md`](docs/radicle-publication.md) for the accepted
Radicle publication receipt, and
[`docs/radicle-ci-drill.md`](docs/radicle-ci-drill.md) for the boundary between
patch CI and canonical admission.

## License

AGPL-3.0-or-later. See [`LICENSE`](LICENSE).

## References

- [Mantle](https://github.com/OnixResearch/mantle), bounded external-batch process adapter at pilot base revision `26c6880c08cd730c9e9420f92ea91a87212e492d`.
- [Octet](https://github.com/OnixResearch/octet), bounded Aeneas process adapter at pilot base revision `28ffbe8cfb01879a241cad248008b75252a8b7f5`.
