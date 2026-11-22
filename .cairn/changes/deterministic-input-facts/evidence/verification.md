# Deterministic input facts verification

Observed: 2026-11-22. Base `c33f1bd64999d0bb8b4a1d73399782efa847f424`; implementation commit `10c61fafe1da10209b7662dc4d1d81e3a177a008`. Host: x86_64-linux, Linux 6.18.

## Baseline

At the base, a run whose child exits without reading its input already succeeds, but `ExecutionOutput::input` carries `written_bytes` and either `AllWritten` or `ReaderClosed`. For small input those depend on whether the writer ran before the child exited. The base's own design notes this: "Small unread input still yields either `AllWritten` or `ReaderClosed`, depending on scheduling." Issue #2 was filed against `83d9cb2`, an earlier revision that still forgave reader closure only when completion was not `Exited`. Lattice vendors such a revision.

## Mutation: old completion guard restored

A scratch copy of the implementation commit was changed in two places and nothing else:

- The reader-closure arm in `write_stdin` was removed, so broken pipe, reset, and abort return `RunError::Io { operation: WriteStdin, .. }`.
- `run_inner` again called the old `finish_stdin_worker`, which forgives those errors only when `completion != Completion::Exited`.

`cargo test -p bounded-exec --no-fail-fast` on the mutant gave 14 passed and 6 failed:

| Test | Mutant result |
| --- | --- |
| `immediate_exit_with_pipe_exceeding_input_succeeds` | FAILED: `Io { operation: WriteStdin, error: BrokenPipe (32) }` |
| `immediate_exit_with_small_input_matches_the_large_input_outcome` | FAILED: same error on the 4 MiB run |
| `immediate_exit_outcome_is_identical_in_sequence` | FAILED: same error |
| `immediate_exit_outcome_is_identical_under_parallel_load` | FAILED: leaked `(small, large) = (13, 512)` of 512 runs each |
| `output_limit_disposition_is_independent_of_input_delivery` | FAILED: unread 4 MiB flood returned `WriteStdin` `BrokenPipe` |
| `only_reader_closure_errors_end_writing_without_failing` | FAILED: closure kinds now fail the writer |

Three more runs of the stress test on the mutant leaked `(27, 512)`, `(29, 512)`, and `(28, 512)`. Large input leaks in every run. Small input leaks in a scheduling-dependent minority, which is the defect issue #2 reports. Timeout, cancellation after spawn, read-to-end, null-device, and pre-spawn cancellation tests passed on the mutant, as they should, because the old guard already forgave stopped children.

On the implementation commit, three repeated runs of the four `immediate_exit` tests passed: 4 passed, 0 failed each.

## Gates

All commands ran with `CARGO_TARGET_DIR` and `TMPDIR` under `/var/tmp`, and Nix with `builders = ''` and `min-free = 0`.

| Gate | Exit | Result |
| --- | --- | --- |
| `nix develop -c cargo fmt --all -- --check` | 0 | clean |
| `nix develop -c cargo test --workspace` | 0 | `bounded-exec` 20 passed; `bounded-exec-core` 13 passed; doc tests 0 |
| `nix develop -c cargo clippy --workspace --all-targets -- -D warnings` | 0 | no warnings |
| `nix develop -c cargo check -p bounded-exec-core --target wasm32-unknown-unknown` | 0 | builds |
| `nix flake check -L --option builders '' --option min-free 0` on `10c61faf` | 0 | `cargo-test` (20 + 13 passed), `cargo-clippy`, `core-no-std-wasm`, `cargo-format`, `radicle-publication-receipt` |
| `cairn validate --root . --strict` | 0 | valid, 0 errors, 0 warnings |
| `cairn gate proposal|design|tasks deterministic-input-facts --root .` | 0 | no issues |

## Non-claims

- These results do not show that any child read its input. Bounded Exec does not observe consumption.
- The stress counts are host- and load-specific. Only the 4 MiB failure under the mutant is deterministic.
- No consumer has adopted this revision. Lattice adoption and removal of its `cat > /dev/null` workaround are Lattice-owned.
