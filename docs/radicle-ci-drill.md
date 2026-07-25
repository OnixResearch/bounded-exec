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
