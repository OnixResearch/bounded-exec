## Context

`run_inner` validates the request, prepares both output captures, builds the command, and spawns the child. `wait_for_completion` reads the cancellation flag for the first time inside its poll loop, after spawn. A flag raised before the call therefore still creates a child. The child runs until the first loop iteration terminates its process group.

The stdin worker writes input in bounded chunks after pipe readiness. Any write error ends the worker with `RunError::Io { operation: WriteStdin, .. }`. `finish_stdin_worker` ignores broken-pipe, connection-reset, and connection-aborted errors only when completion is not `Exited`. A child that exits without reading its input closes the read end. Whether the writer finishes before that closure depends on scheduling. Large input always loses the race, because no pipe buffer can hold it. Small input loses only sometimes.

## Decisions

### Decision: Check cancellation once more, immediately before spawn

**Choice:** After `validate_request` and `prepare_captures`, and before `prepare_command` and `Command::spawn`, read the flag with `Acquire` ordering. If the flag is raised, return `ExecutionOutput` with completion `Cancelled` and no exit code or signal. Both captures come from the prepared accumulators finished with zero observed bytes. Input delivery is `NotStarted`, and the disposition comes from the core classifier.

**Rationale:** Validation and preparation errors keep priority. The returned shape matches cancellation after spawn, except for the termination facts that only a reaped child can supply. Using the core classifier keeps one disposition authority.

**Boundary:** A flag raised after this check but before spawn is observed by the existing poll loop as cancellation after spawn. That window is inherent to an unsynchronized flag, and this change claims nothing about it.

### Decision: Reader closure ends delivery as a fact

**Choice:** In the writer, broken-pipe, connection-reset, and connection-aborted errors end delivery with `InputStatus::ReaderClosed` and the bytes written so far. The run continues to completion classification. The completion-dependent `is_expected_stdin_shutdown` filter is removed because no path needs it any more.

**Rationale:** The read end can close for only two reasons: the child closed or exited, or teardown killed it. Neither reason is an infrastructure failure of the runner. Making the error a fact in every completion removes the scheduling dependence completely, rather than only reducing it.

### Decision: Typed, bounded delivery facts in `ExecutionOutput`

**Choice:** Add `ExecutionOutput::input: InputDelivery { offered_bytes, written_bytes, status }`. `InputStatus` has these variants:

- `NotStarted`: no child was created; zero bytes were written.
- `NullDevice`: input was `Input::Null`; zero bytes were offered.
- `AllWritten`: every offered byte was written and the runner closed the pipe.
- `ReaderClosed`: the read end closed before every offered byte was written.

Invariants: `written_bytes <= offered_bytes`; `AllWritten` implies equality; `ReaderClosed` implies `written_bytes < offered_bytes`.

**Rationale:** A consumer can see partial delivery without the run failing. Counts are bounded by the already-bounded input length. The types stay in the standard-library shell because only the shell observes pipe writes. The core classifier does not use them.

**Boundary:** Written bytes were accepted by the pipe, not read by the child. No stricter consumption claim is possible from the writer side, so no opt-in strictness flag is added. A consumer that needs proof of consumption must obtain it from the child's own protocol or output.

### Decision: Keep every other writer failure

**Choice:** Other write errors still return `RunError::Io { operation: WriteStdin, error }`. A zero-byte write still returns `WriteZero`. A writer stopped by teardown still returns `WorkerTeardownTimedOut(Worker::Stdin)`. The writer becomes generic over its pipe type, so tests can inject errors that a real pipe cannot produce on demand.

**Rationale:** Only reader closure has a benign, scheduling-dependent cause. Every other error still points to runner or host trouble.

## Functional Core and Imperative Shell

The `no_std + alloc` core is unchanged. It still classifies completion, exit code, and truncation facts. The shell owns the pre-spawn flag read, the writer, and delivery accounting.

## Compatibility

`ExecutionOutput` gains a public field. Consumers that build it with a struct literal, or destructure it exhaustively, must add `input`. Field-access consumers are unaffected. A pre-raised flag now returns `exit_code: None` and `signal: None`, where it previously reported the facts of a killed child, such as `SIGKILL`.

## Risks / Trade-offs

- A consumer that relied on a stdin `BrokenPipe` error after a normal exit to detect unread input must now inspect `input.status`.
- Small unread input still yields either `AllWritten` or `ReaderClosed`, depending on scheduling. The run outcome is deterministic; the delivery fact honestly reports which interleaving occurred.

## Validation

Positive tests cover read-to-end input, unread small and large input in sequential and parallel loops, null-device input, the pre-spawn positive control, and cancellation after spawn. Negative tests cover the repeated pre-raised-flag marker proof, injected non-closure write errors, zero-byte writes, a stopped writer, timeout with unread input, and output-limit dispositions with read and unread input.
