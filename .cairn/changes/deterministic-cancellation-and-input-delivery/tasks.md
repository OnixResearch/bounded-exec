## Baseline

- [x] [serial] `[baseline-repro]` Reproduce both defects against `8f1ed852` before changing the shell: count pre-raised-flag marker writes and unread-input run failures. Observed: 34 of 200 pre-raised-flag runs wrote the marker; 50 of 50 unread 1 MiB runs and 14 of 50 unread 8-byte runs failed. r[bounded_exec.cancellation.verification] r[bounded_exec.input_delivery.verification]

## Cancellation

- [x] [depends:baseline-repro] `[pre-spawn-cancel]` Read the cancellation flag after validation and capture preparation and before spawn. Return the cancelled observation with no exit code, no signal, empty captures, and `NotStarted` input. r[bounded_exec.cancellation.pre_spawn]
- [x] [parallel] `[cancel-tests]` Add the repeated marker proof with a positive control, an invalid-request-with-raised-flag test, and a cancellation-after-spawn test that observes the reaped signal. r[bounded_exec.cancellation.verification] r[bounded_exec.cancellation.post_spawn]

## Input delivery

- [x] [depends:baseline-repro] `[delivery-fact]` Add `InputDelivery` and `InputStatus`, and report them in `ExecutionOutput` for every path. r[bounded_exec.input_delivery.fact]
- [x] [depends:delivery-fact] `[reader-closed]` End delivery as `ReaderClosed` on broken-pipe, connection-reset, and connection-aborted errors in every completion. Remove the completion-dependent filter. r[bounded_exec.input_delivery.reader_closed]
- [x] [depends:reader-closed] `[write-errors]` Keep other write errors, zero-byte writes, and stopped writers as failures. Make the writer generic over its pipe so tests can inject errors. r[bounded_exec.input_delivery.write_errors]
- [x] [parallel] `[delivery-tests]` Add sequential and parallel unread-input loops, read-to-end, null-device, injected-error, stopped-writer, timeout, and output-limit tests. r[bounded_exec.input_delivery.verification]

## Documentation and closeout

- [x] [depends:write-errors] `[delivery-docs]` Document the cancellation and input semantics, the API change, and the consumption non-claim in `README.md` and `ADOPTION.md`. r[bounded_exec.input_delivery.boundary]
- [x] [serial] `[validation]` Run formatting, workspace tests, Clippy, the core wasm check, Cairn validation and gates, and `nix flake check`. r[bounded_exec.cancellation.verification] r[bounded_exec.input_delivery.verification]
