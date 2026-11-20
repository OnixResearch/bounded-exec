## Why

Two process-runner behaviors depend on scheduling, so consumers see failures that appear only under load. Lattice vendors revision `29dac88ecded94457572db3fdfaaaab95fa91525` and observed both defects.

First, `run_with_cancellation` spawns the child before it reads the cancellation flag. The poll loop reads the flag only after spawn. A request whose flag is already raised can therefore start a child that runs briefly before teardown. Lattice's pre-execution cancellation test failed 2–6 of 48 runs under load, and Lattice now guards this in its own shell.

Second, a child can exit normally without reading its piped input. The stdin writer can then lose the race and see a broken pipe. Execution currently tolerates that error only when the runner stopped the child. When the child exited on its own, the same error becomes a run failure. The same command succeeds whenever the write lands first.

## What Changes

- Check the cancellation flag after request validation and capture preparation, and before child creation. A raised flag returns the cancelled observation without spawning.
- Keep validation and capture-preparation errors ahead of the cancellation check.
- Keep cancellation after spawn unchanged: owned-process teardown, reap, and a cancelled observation.
- Treat broken-pipe, connection-reset, and connection-aborted stdin write errors as reader closure in every completion, not as run failures.
- Report bounded input-delivery facts in `ExecutionOutput`: offered bytes, bytes written into the pipe, and how delivery ended.
- Keep every other stdin write error, and the stopped-writer teardown error, as run failures.
- Add paired positive and negative tests, including repeated and parallel runs, and document the semantics and their claim boundary.

## Impact

- **Files**: `crates/bounded-exec/src/lib.rs`, `README.md`, `ADOPTION.md`, and this change's Cairn artifacts.
- **Public API**: `ExecutionOutput` gains an `input` field. New public types `InputDelivery` and `InputStatus` are added. Consumers that build `ExecutionOutput` with a struct literal or destructure it without `..` must update. Field-access consumers need no change.
- **Behavior**: a pre-raised flag no longer starts a child. The pre-spawn observation has no exit code and no signal. Reader closure on stdin no longer fails a run whose child exited on its own.
- **Testing**: workspace tests, Clippy, formatting, the core wasm check, Cairn validation and gates, and `nix flake check`.
- **Consumers**: Lattice's own pre-spawn cancellation guard becomes redundant after it adopts the published revision. Removing that guard is Lattice-owned work.

## Dependencies

None. The core crate's `no_std` boundary does not change.

## Non-Goals

- Do not claim that the child read, parsed, or consumed any input byte. Bytes written into a pipe are only buffered.
- Do not add a strict input-consumption mode. The runner cannot observe consumption deterministically.
- Do not change timeout, output-limit, teardown, or descendant-cleanup behavior.
- Do not close the window in which a flag is raised after the pre-spawn check and before spawn. The poll loop handles that case as cancellation after spawn.
- Do not publish, tag, or push a revision as part of this change.
