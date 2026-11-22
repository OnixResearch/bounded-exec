## Baseline

- [x] [serial] `[baseline]` Confirm that the base `c33f1bd` reports scheduling-dependent `written_bytes` and `AllWritten` or `ReaderClosed` for a child that exits without reading, and that issue #2's proposed `stdin_unconsumed` and bytes-written facts would be scheduling-dependent too. Evidence: `evidence/verification.md`. r[bounded_exec.input_delivery.fact]

## Implementation

- [x] [depends:baseline] `[deterministic-facts]` Derive `InputDelivery { offered_bytes, status }` from the request and child creation only. Remove `written_bytes`, and replace `AllWritten` and `ReaderClosed` with `Piped`. r[bounded_exec.input_delivery.fact]
- [x] [depends:deterministic-facts] `[closure-silent]` End writing on broken pipe, reset, and abort without failing the run or changing the observation in every completion. r[bounded_exec.input_delivery.reader_closed]
- [x] [depends:closure-silent] `[other-errors]` Keep other write errors, zero-byte writes, and stopped writers as failures, so that only reader closure ends writing silently. The existing write-error requirement is unchanged. r[bounded_exec.input_delivery.reader_closed]
- [x] [parallel] `[non-claim-docs]` State in `README.md` and `ADOPTION.md` that Bounded Exec does not observe stdin consumption and that consumers needing it must use a child-side acknowledgement. Record the removal of the `c33f1bd` members. r[bounded_exec.input_delivery.boundary]

## Verification

- [x] [parallel] `[immediate-exit-tests]` Add 4 MiB and small-input immediate-exit tests that require the identical `Exited` success observation, a sequential loop, and a 16-worker parallel stress loop that counts leaked `WriteStdin` errors. Keep read-to-end, timeout, cancellation, output-limit, injected-error, and stopped-writer tests. r[bounded_exec.input_delivery.verification]
- [x] [depends:immediate-exit-tests] `[mutation]` Restore the old `completion != Completion::Exited` guard on a scratch copy and record that the 4 MiB, sequential, and stress tests fail. Evidence: `evidence/verification.md`. r[bounded_exec.input_delivery.verification]
- [x] [serial] `[gates]` Run formatting, workspace tests, Clippy, the core wasm check, Cairn validation and gates, and `nix flake check`. Evidence: `evidence/verification.md`. r[bounded_exec.input_delivery.verification]
