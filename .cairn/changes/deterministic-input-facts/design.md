## Context

The stdin writer runs on its own thread. It waits for pipe readiness, writes bounded chunks, and stops at the end of input, at reader closure, or at a teardown stop. At `c33f1bd` it returned `InputDelivery { offered_bytes, written_bytes, status }`, with `AllWritten` or `ReaderClosed` chosen by how writing ended.

When the child reads all of its input, writing always ends with every byte written. When the child exits or is stopped without reading, the outcome depends on input size and scheduling:

- Input larger than the pipe buffer always meets a closed pipe, so it ends in `ReaderClosed` with a partial count.
- Small input either fits in the buffer before the child exits (`AllWritten`) or meets the closed pipe first (`ReaderClosed` with zero or a partial count).

So `written_bytes` and the two statuses vary between runs of the same request and child. The run outcome was already deterministic; the observation was not.

## Decisions

### Decision: Input facts come only from the request and child creation

**Choice:** A pure function, `input_delivery(&Input, ChildCreation)`, computes `InputDelivery`. `offered_bytes` is the declared input length. `status` is `NotStarted` when no child was created, `NullDevice` for `Input::Null`, and `Piped` for `Input::Bytes`. The writer returns `Result<(), RunError>` and contributes no observation.

**Rationale:** The observation must be identical for the same input and child behavior, whatever the writer's scheduling. Only facts fixed before the writer starts meet that bar. The declared size is the one stdin fact a consumer can rely on.

**Alternative rejected:** Keep `written_bytes` or a `stdin_unconsumed` flag, as issue #2 proposes. Both are scheduling-dependent for small input, so they move the race from the error path into the observation.

### Decision: Reader closure ends writing silently in every completion

**Choice:** Broken pipe, connection reset, and connection aborted end the writer with `Ok(())`, whether the child exited, timed out, or was cancelled. The completion-dependent guard from earlier revisions does not return.

**Rationale:** The read end can close only because the child closed it, exited, or was stopped. None of these is a runner failure, and whether the writer observes it is scheduling-dependent.

### Decision: Every other writer failure still fails the run

**Choice:** Other write errors return `RunError::Io { operation: WriteStdin, error }`. A zero-byte write returns `WriteZero`. A writer stopped by teardown returns `WorkerTeardownTimedOut(Worker::Stdin)`. The writer stays generic over its pipe, so tests inject errors that a real pipe cannot produce on demand.

**Rationale:** Only reader closure has a benign, scheduling-dependent cause.

### Decision: State the consumption non-claim plainly

**Choice:** Documentation says that Bounded Exec does not observe stdin consumption, and that a consumer needing proof of consumption must obtain a child-side acknowledgement, such as a count or digest in the child's own output or protocol.

**Rationale:** Even fully written input may sit unread in the pipe buffer. No writer-side fact can stand in for consumption.

## Functional Core and Imperative Shell

The `no_std + alloc` core is unchanged. It classifies completion, exit code, and truncation. In the shell, `input_delivery` and `is_reader_closure` are pure decisions; the writer thread, pipe readiness, and process lifecycle stay imperative.

## Compatibility

`InputDelivery::written_bytes`, `InputStatus::AllWritten`, and `InputStatus::ReaderClosed` are removed, and `InputStatus::Piped` is added. They exist only at `c33f1bd`, which no consumer pins. Consumers on `8f1ed852` or earlier see the same upgrade notes as before, with the narrower input type.

## Risks / Trade-offs

- A consumer can no longer see whether a large input was cut short. That fact was only reliable for input larger than the pipe buffer. A consumer that needs it must get it from the child.

## Validation

Positive tests: immediate exit with 4 MiB input and with small input give the identical `Exited` success observation; a sequential loop and a 16-worker stress loop see no leaked stdin error and no differing observation; a read-to-end child, null-device input, and pre-spawn cancellation keep their facts. Negative tests: injected non-closure errors and zero-byte writes still fail; a stopped writer still fails; timeout and cancellation after spawn keep their completions and the `Piped` fact. A recorded mutation restores the old `completion != Completion::Exited` guard on a scratch copy and shows the large-input, small-input, sequential, and stress tests failing.
