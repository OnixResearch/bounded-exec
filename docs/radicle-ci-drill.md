# Radicle CI Drill

This documentation-only change exercises the production Radicle patch CI path
for Bounded Exec.

Acceptance requires the runner to:

- resolve this exact patch revision from its local Radicle object store;
- preserve the reviewed `Cargo.toml`, `Cargo.lock`, `flake.nix`, and
  `flake.lock` identities;
- execute the accepted Nix check without network access or credentials;
- retain bounded logs and a typed result outside the source object; and
- publish status only as the non-delegate CI bot.

A successful drill is evidence for this bounded path. It does not prove source
correctness, artifact durability, deployment authority, release readiness, or
protocol-enforced mandatory CI.

## Canonical admission is separate

A passing CI status is an input to canonical admission, not canonical authority.
The operator guard must preview against live Radicle state and reject admission
unless all of these facts agree:

- the candidate descends the current canonical branch head;
- CI status names the exact candidate, source locks, policy, and bounded result;
- the configured threshold of delegates gave exact native approvals;
- those same delegates published matching signed refs at feature level `parent`;
- the offline admission receipt verifies; and
- the canonical branch still equals the guard's expected-old object.

Preview does not mutate the canonical branch. Mutation requires a separate,
explicit operator action after reviewing the fresh preview. Imported review
text, bot comments, seed replication, and a green CI result never substitute
for delegate approval or grant canonical-ref authority.
