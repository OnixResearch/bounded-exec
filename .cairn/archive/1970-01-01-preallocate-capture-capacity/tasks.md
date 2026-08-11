## Baseline and core

- [x] [serial] `[capture-baseline]` Run current core and shell capture tests before changing capture construction. r[bounded_exec.capture_capacity.verification]
- [x] [depends:capture-baseline] `[capture-prepare-core]` Add fallible prepared-capture construction with complete limit reservation and typed allocation failure. r[bounded_exec.capture_capacity.preparation]
- [x] [parallel] `[capture-core-tests]` Add positive exact-limit and truncation tests plus negative allocation and counter-overflow tests. r[bounded_exec.capture_capacity.verification]

## Shell sequencing

- [x] [depends:capture-prepare-core] `[capture-pre-spawn-shell]` Prepare both output accumulators before child creation and return stream-specific errors without spawning on failure. r[bounded_exec.capture_capacity.pre_spawn]
- [x] [depends:capture-pre-spawn-shell] `[capture-worker-transfer]` Move prepared accumulators into capture workers and prevent capacity growth during observation. r[bounded_exec.capture_capacity.steady_state]
- [x] [parallel] `[capture-shell-tests]` Add spawn-denial, partial-preparation cleanup, output-flood, cancellation, and teardown tests. r[bounded_exec.capture_capacity.verification]

## Compatibility and closeout

- [x] [depends:capture-worker-transfer] `[capture-api-migration]` Migrate production callers and document any compatibility constructor without hidden fallback behavior. r[bounded_exec.capture_capacity.boundary]
- [x] [parallel] `[capture-nonclaims]` Document that preparation does not prove execution, sandboxing, latency, or global allocation behavior. r[bounded_exec.capture_capacity.boundary]
- [x] [serial] `[capture-validation]` Run formatting, workspace tests, Clippy, the core Wasm check, Cairn validation, and relevant Nix checks. r[bounded_exec.capture_capacity.verification]
