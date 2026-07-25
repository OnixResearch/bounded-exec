# Consumer adoption status

## Proven boundary

The standalone test matrix covers the two first-wave consumer policies:

- **Mantle-equivalent strict capture:** clean environment, piped stdin, accepted zero exit, timeout, process-group teardown, and rejection when either retained-output bound is exceeded.
- **Octet-equivalent truncating capture:** bounded retained prefixes, exact observed-byte counts, timeout classification, and successful exit classification when consumer policy permits truncation.

A disposable migration against exact Mantle and Octet base revisions also passed both focused test suites and both consumer-focused Clippy rails. See [`PILOT.md`](PILOT.md) for commands, line reductions, retained consumer policy, and bounded caveats. These are pilot facts, not consumer release evidence.

## Cutover gate

The reviewed commit `29dac88ecded94457572db3fdfaaaab95fa91525` is now
published as canonical Radicle project
`rad:z2CpqLFpdP36fZXYUK5ZNWxMibpCo` through the read-only HTTPS adapter
`https://git.onix.computer/z2CpqLFpdP36fZXYUK5ZNWxMibpCo.git`. The accepted
publication receipt is documented in
[`docs/radicle-publication.md`](docs/radicle-publication.md). Maintained
consumers must not use an ambient `../bounded-exec` path or configure an
automatic GitHub fallback.

After consumer-owned cutover authorization:

1. consume the reviewed immutable commit without changing the AGPL license;
2. pin `bounded-exec` and `bounded-exec-core` by exact Git revision in both consumers;
3. align Cargo and Nix source identities in each consumer;
4. add a reviewed explicit working-directory field to Mantle's Nickel-backed dispatcher command profile instead of preserving ambient current-directory behavior;
5. replace Mantle's process mechanics in `src/external_batch_dispatch.rs` while retaining executable BLAKE3 verification, Slurm projection, protocol parsing, redaction, and product error codes in Mantle;
6. replace Octet's process mechanics in `cargo-octet/src/aeneas_execution.rs` while retaining provider-path construction, environment allowlisting, stage ordering, log rendering, report validation, and proof/evidence claims in Octet;
7. review the deliberate change from direct-child kill to process-group teardown in both consumers;
8. run standalone tests plus the focused pre-cutover consumer commands;
9. remove duplicate pipe, polling, kill, reap, and capture helpers only after parity passes.

Focused consumer commands:

```sh
# Mantle
nix develop -c cargo test -p mantle --bin mantle external_batch_dispatch::tests::

# Octet
nix develop -c cargo test -p cargo-octet --lib aeneas_execution::tests::
```

Cutover success means both focused suites retain their public behavior and product diagnostics while the generic mechanics come from the same exact published revision. It does not transfer Mantle build authority, Octet proof authority, Valence evidence authority, or Cairn release-readiness authority into this repository.
