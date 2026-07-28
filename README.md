# bounded-exec

`bounded-exec` runs child processes with explicit limits. It provides product-neutral process mechanics for Rust systems code.

The repository separates pure policy from operating-system effects:

- `bounded-exec-core` is a `no_std` and `alloc` functional core. It checks limits, retains bounded output prefixes, and classifies execution facts.
- `bounded-exec` is the standard-library shell. It starts processes, manages bounded I/O, applies deadlines and cancellation, and tears down owned processes.

Consumers supply the exit-code and truncation policy. They must also supply every execution limit.

On Unix, process-group scope starts the child in a new group. It terminates remaining descendants before it joins the capture workers.

Other platforms terminate only the direct child. This repository makes no descendant-teardown claim for those platforms.

## Boundary

This repository owns only process mechanics. Consumers own these functions:

- executable discovery, identity checks, and authorization
- sandbox, capability, seccomp, namespace, and network policy
- remote execution and transport protocols
- output parsing, redaction, and product diagnostics
- receipts, evidence roles, release readiness, and lifecycle decisions

A successful result records an observation from the supplied request and host implementation. It does not prove any of these properties:

- sandboxing or hermeticity
- child correctness
- executable authorization
- platform equivalence
- release eligibility

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

## Source publication

The canonical Radicle project is:

```text
rad:z2CpqLFpdP36fZXYUK5ZNWxMibpCo
```

The reviewed pilot source is Git object `29dac88ecded94457572db3fdfaaaab95fa91525`. The source is available from this read-only URL:

```text
https://git.onix.computer/z2CpqLFpdP36fZXYUK5ZNWxMibpCo.git
```

GitHub is a transition mirror. It is not an admitted dependency fallback.

See [`docs/radicle-publication.md`](docs/radicle-publication.md) for clone, revision, outage, rollback, evidence, and non-claim details.

## Checks

Run all commands from the repository root:

```sh
nix develop -c cargo fmt --all -- --check
nix develop -c cargo test --workspace
nix develop -c cargo clippy --workspace --all-targets -- -D warnings
nix develop -c cargo check -p bounded-exec-core --target wasm32-unknown-unknown
nix flake check -L
```

Positive tests cover explicit environment behavior, input round trips, and accepted exits. Negative tests cover these cases:

- malformed requests
- output floods
- deadlines and cancellation
- descendant-held pipe cleanup

For more information, read these documents:

- [`PILOT.md`](PILOT.md): disposable two-consumer migration evidence
- [`ADOPTION.md`](ADOPTION.md): the authoritative consumer cutover gate
- [`docs/radicle-publication.md`](docs/radicle-publication.md): the accepted Radicle publication receipt

## License

This repository uses `AGPL-3.0-or-later`. See [`LICENSE`](LICENSE).

## References

- [Mantle](https://github.com/OnixResearch/mantle), bounded external-batch process adapter at pilot base revision `26c6880c08cd730c9e9420f92ea91a87212e492d`.
- [Octet](https://github.com/OnixResearch/octet), bounded Aeneas process adapter at pilot base revision `28ffbe8cfb01879a241cad248008b75252a8b7f5`.
