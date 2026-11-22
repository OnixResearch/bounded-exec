## Why

Issue #2 (`OnixResearch/bounded-exec#2`) reports that a child which exits normally without reading its piped stdin fails or succeeds depending on writer scheduling. Lattice vendors a revision that still forgives a stdin broken pipe only when completion is not `Exited`. Under load, a shell node that ignored its stdin and exited 0 became `AdapterRejected` in 1 of 16 parallel runs.

Revision `c33f1bd` already stopped that error from failing the run. It moved the race into the observation instead: `ExecutionOutput::input` reports `written_bytes` and either `AllWritten` or `ReaderClosed`. For a child that exits without reading small input, both depend on whether the writer ran before the child exited. Two runs of the same request and child behavior can still return different observations. A consumer that keys a receipt, a replay check, or a policy on those facts inherits the nondeterminism.

The issue proposes recording `stdin_unconsumed` and the bytes written. That repeats the same mistake. Whether stdin bytes entered the pipe before a non-reading child exits is itself scheduling-dependent. Even fully written input may sit unread in the pipe buffer, so the runner cannot observe consumption at all.

## What Changes

- Derive every input fact from the request and from whether a child was created. Remove `InputDelivery::written_bytes`.
- Replace `InputStatus::AllWritten` and `InputStatus::ReaderClosed` with one `InputStatus::Piped`. `NotStarted` and `NullDevice` stay.
- Keep reader closure (broken pipe, connection reset, connection aborted) as the end of writing, never a run failure, in every completion including a normal exit. It leaves no trace in the observation.
- Keep every other stdin write error, zero-byte writes, and stopped writers as run failures.
- State the non-claim plainly: Bounded Exec does not observe stdin consumption. A consumer that needs proof of consumption must use a child-side acknowledgement.
- Add immediate-exit tests with 4 MiB and small input that require the identical observation, and a parallel stress test that counts leaked stdin errors. Prove that the stress and large-input tests fail when the old completion guard is restored.

## Impact

- **Files**: `crates/bounded-exec/src/lib.rs`, `README.md`, `ADOPTION.md`, `.cairn/specs/input-delivery/spec.md`, and this change's artifacts.
- **Public API**: `InputDelivery` loses `written_bytes`. `InputStatus` loses `AllWritten` and `ReaderClosed` and gains `Piped`. Code that reads those members must update. No published or adopted revision carries them: they first appeared in `c33f1bd`, which no consumer pins.
- **Behavior**: the run outcome is unchanged from `c33f1bd`. The observation for a child that does not read its input is now identical across schedulings.
- **Testing**: workspace tests, Clippy, formatting, the core wasm check, Cairn validation and gates, `nix flake check`, and a recorded mutation run.
- **Consumers**: Lattice can drop its `cat > /dev/null` workaround after adopting a published revision that includes this change. That adoption is Lattice-owned.

## Dependencies

- `deterministic-cancellation-and-input-delivery`, which introduced `InputDelivery` and reader-closure tolerance and has already synced its requirements into `.cairn/specs/input-delivery/spec.md`. This change modifies those synced requirements before either change is published. It is not a recorded `depends_on`: Cairn blocks sync until a dependency is archived, and that change stays active until a consumer adopts a published revision. Archiving that change only moves it, so it does not conflict with this change's synced text.

## Non-Goals

- Do not change behavior for children that are killed, timed out, or cancelled. Their runs already tolerate reader closure, and their observations keep the same shape.
- Do not add any consumption, partial-delivery, or bytes-written fact. None can be deterministic from the writer side.
- Do not add a strict input-consumption mode.
- Do not publish, tag, or push a revision as part of this change.
