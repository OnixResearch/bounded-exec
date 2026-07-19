# Two-consumer adoption pilot

Date: 2026-07-18

This pilot replaced duplicated process mechanics in disposable Mantle and Octet worktrees with the local `bounded-exec` crates. It validates the proposed dependency boundary without placing a sibling path dependency in either authoritative consumer repository.

## Inputs

| Consumer | Base revision | Migrated surface | Capture policy |
|---|---|---|---|
| Mantle | `26c6880c08cd730c9e9420f92ea91a87212e492d` | `src/external_batch_dispatch.rs` | Reject either truncated stream |
| Octet | `28ffbe8cfb01879a241cad248008b75252a8b7f5` | `cargo-octet/src/aeneas_execution.rs` | Retain prefixes and let Octet reject truncation in its report policy |

Both migrations used bounded-exec mechanism revision `6d38e4f0919b30c547ffbc6c298c4f9e6c3c91db`.

## Results

### Mantle

The pilot retained executable BLAKE3 verification, environment-handle resolution, protocol parsing, Slurm projection, redaction boundaries, and existing product error strings in Mantle. `bounded-exec` replaced stdin writing, concurrent pipe draining, output retention, polling, timeout handling, kill, and reap mechanics.

The focused suite and consumer-focused Clippy rail passed:

```text
5 passed; 0 failed; 0 ignored
cargo clippy -p mantle --bin mantle --no-deps -- -D warnings: passed
```

Command:

```sh
nix develop -c cargo test -p mantle --bin mantle external_batch_dispatch::tests::
nix develop -c cargo clippy -p mantle --bin mantle --no-deps -- -D warnings
```

The process-adapter source diff contained 59 inserted and 123 deleted lines. A broad Clippy invocation reached pre-existing vendored `fuse-backend-rs` diagnostics, while the consumer-focused `--no-deps` invocation passed.

### Octet

The pilot retained bubblewrap argv construction, provider-path derivation, environment allowlisting, stage order, lossy log rendering, diagnostics, report validation, artifact collection, and proof claims in Octet. `bounded-exec` replaced command setup, concurrent pipe draining, bounded retention, polling, timeout handling, kill, and reap mechanics.

The focused suite and focused Clippy rail passed:

```text
10 passed; 0 failed; 3 provider-dependent tests ignored
cargo clippy -p cargo-octet --lib -- -D warnings: passed
```

Commands:

```sh
nix --option builders '' --option secret-key-files '' develop path:/home/brittonr/git/OnixResearch/octet \
  -c cargo test -p cargo-octet --lib aeneas_execution::tests::
nix --option builders '' --option secret-key-files '' develop path:/home/brittonr/git/OnixResearch/octet \
  -c cargo clippy -p cargo-octet --lib -- -D warnings
```

The Aeneas runner source diff contained 61 inserted and 120 deleted lines.

## Boundary findings

1. **The mechanism boundary held for both consumers.** Neither migration moved executable authorization, sandbox construction, parsing, diagnostics, evidence, or lifecycle policy into `bounded-exec`.
2. **Mantle lacks an explicit command working-directory field.** The pilot snapshotted the caller's current directory to preserve existing behavior. Production cutover should add a reviewed working-directory policy to Mantle's Nickel-backed dispatcher profile rather than leave this ambient.
3. **Capture classification must remain consumer-owned.** Mantle maps truncation to its existing hard error. Octet retains bounded prefixes and reports truncation through Aeneas diagnostics before stopping later stages.
4. **Process-group teardown is a deliberate strengthening.** Both pilots selected `TerminationScope::ProcessGroup`; this needs to remain explicit in consumer review because the previous helpers directly killed only the immediate child.
5. **Infrastructure errors stay non-claims.** Runner failures are translated into each consumer's diagnostics and do not become build, proof, evidence, or readiness claims.

## Cutover decision

The pilot validates the shared API and reduces duplicated mechanism in two distinct consumers. Authoritative cutover remains blocked only by publication and consumer-owned policy work:

1. publish `bounded-exec` at an immutable Git revision after explicit authorization;
2. add a reviewed explicit working-directory policy to Mantle;
3. replace the disposable path dependencies with exact Git-revision pins and aligned Nix source identities;
4. rerun each consumer's focused and repository-local release rails;
5. commit the migrations in the independent consumer repositories.

The disposable worktrees and local path dependencies are test scaffolding, not release evidence or durable product inputs.
