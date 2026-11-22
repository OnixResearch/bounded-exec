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

`ExecutionOutput::input` reports `offered_bytes`, `written_bytes`, and an `InputStatus`:

- `NotStarted`: no child was created.
- `NullDevice`: input was `Input::Null`.
- `AllWritten`: every offered byte was written, then the runner closed the pipe.
- `ReaderClosed`: the read end closed before every offered byte was written.

A child may exit, close stdin, or be stopped without reading its input. The writer then sees broken pipe, connection reset, or connection aborted, and records `ReaderClosed`. That is never a run failure in any completion. The run is classified only by completion, exit code, and output policy. Other stdin write errors, zero-byte writes, and writers stopped by teardown still fail with their typed errors.

Written bytes were accepted by the pipe. They do not prove that the child read, parsed, or acted on them. With small input and a child that exits without reading, either `AllWritten` or `ReaderClosed` can occur, depending on scheduling. Consumers that need proof of consumption must obtain it from the child's own output or protocol.

## Source publication

The canonical Radicle project is `rad:z2CpqLFpdP36fZXYUK5ZNWxMibpCo`. The
reviewed pilot source is the exact Git object
`29dac88ecded94457572db3fdfaaaab95fa91525`, available read-only at
`https://git.onix.computer/z2CpqLFpdP36fZXYUK5ZNWxMibpCo.git`. GitHub is a
transition mirror, not an admitted dependency fallback.

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

Positive coverage includes explicit environment/input round trips, accepted exits, read-to-end input delivery, and a spawn control. Negative coverage includes malformed requests, bounded output floods, deadlines, pre-spawn cancellation proven by marker files over repeated runs, cancellation after spawn, unread input repeated sequentially and under parallel load, injected non-closure stdin errors, stopped writers, and descendant-held pipe cleanup.

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
