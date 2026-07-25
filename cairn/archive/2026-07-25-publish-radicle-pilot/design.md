## Context

The current reviewed consumer revision is `29dac88ecded94457572db3fdfaaaab95fa91525`. Lattice names that exact commit in both Cargo and Nix configuration, so the pilot can change transport and repository identity without combining the result with a dependency upgrade.

Bounded Exec owns only its Git history, repository metadata, publication receipt, and existing process-mechanism verification. `onix-core` owns concrete node, HTTP, machine, monitoring, backup, and restore observations. OnixOS owns forge catalog admission, independent-seed acceptance, CI policy, and cross-repository pilot acceptance. Lattice owns Cargo/Nix source alignment and consumer behavior.

Completion begins only after the `onix-core` bootstrap-node receipt and OnixOS catalog admission are accepted. It ends when the existing commit is available under one recorded Radicle RID through the admitted service paths and Lattice consumes it without a GitHub fallback. Merely running `rad init`, adding a second remote, or publishing a mutable branch is false completion.

## Decisions

### Decision: Require concrete node evidence before publication

**Choice:** Reject publication when the `onix-core` bootstrap-node receipt or its OnixOS catalog admission is absent, stale, malformed, policy-incompatible, or unable to acquire the exact probe object through the declared HTTPS path.

**Rationale:** Repository identity and publication evidence must bind to an operating node rather than a planned hostname. The bootstrap receipt does not satisfy the later independent-seed outage gate.

### Decision: Preserve Git object identity

**Choice:** Initialize Radicle from the existing repository and publish the complete reachable history needed by maintained consumers. Do not rewrite commits, re-sign historical commits, squash, or recreate tags.

**Rationale:** Existing Cargo, Nix, documentation, and evidence name exact Git object IDs. Preserving those IDs allows transport migration to remain independent from source change.

### Decision: Record one canonical RID and explicit endpoints

**Choice:** Record the canonical RID, delegates, canonical default branch and release-tag rules, minimum signed-reference feature, seed identities, and HTTPS clone endpoints in a BLAKE3-bound publication receipt. Repository metadata is updated from that reviewed receipt rather than from an unverified operator string.

**Rationale:** RIDs are the durable Radicle project identity; HTTPS endpoints are transport adapters for ordinary Git clients. Conflating them would make an endpoint rename appear to change source authority.

### Decision: Test exact acquisition, not only browsing

**Choice:** Verify the selected object from each seed and through the Lattice Cargo/Nix paths. Compare Git object ID and source content before running focused consumer checks.

**Rationale:** A web explorer proving that a branch is visible does not prove Cargo or Nix can acquire the exact dependency source.

### Decision: No implicit fallback

**Choice:** The pilot consumer configuration contains only the Radicle-backed source for Bounded Exec. GitHub may remain a read-only mirror, but failure of Radicle acquisition fails closed and does not retry GitHub.

**Rationale:** Automatic fallback would hide an availability failure and preserve the dependency the pilot is intended to remove.

### Decision: Publication adds no runtime authority

**Choice:** Do not add Radicle node, identity, network, CI, receipt, or synchronization behavior to the `bounded-exec` crates. Publication automation remains an operator shell outside product APIs.

**Rationale:** The repository's functional core and process shell are product-neutral mechanisms. Forge operation is not child-process execution policy.

## Evidence Shape

The publication evidence records:

- canonical RID and exact reviewed Git object ID;
- delegate and canonical-ref policy identities;
- signed-reference feature policy;
- accepted `onix-core` bootstrap/deployment and OnixOS catalog receipt identities;
- seed and HTTPS endpoint identities without credentials;
- exact clone/fetch/object verification commands and bounded outcomes;
- one-seed-outage observations;
- Lattice consumer receipt identity;
- explicit non-claims.

The receipt uses BLAKE3 for Bounded Exec-owned identities. Git object IDs retain Git's required algorithm because interoperability mandates it.

## Positive and Negative Validation

Positive validation acquires the same exact commit from every admitted endpoint, verifies source identity, and runs the existing Bounded Exec and focused Lattice checks. Negative validation covers unknown RID, unavailable object ID, altered source identity, mismatched Cargo/Nix revisions, unavailable endpoint, attempted GitHub fallback, and incomplete publication evidence.

## Risks / Trade-offs

- An HTTPS gateway adds a DNS/TLS/service dependency even though the RID remains canonical.
- A seed can possess public repository bytes without being a delegate; replication is not authority.
- Radicle availability does not prove GitHub-independent third-party Nix or Cargo acquisition.
- Passing publication tests does not prove process correctness, sandboxing, consumer release readiness, or whole-stack migration.
