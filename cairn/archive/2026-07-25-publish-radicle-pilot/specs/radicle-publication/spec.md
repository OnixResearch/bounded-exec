# Radicle Publication Delta

## ADDED Requirements

### Requirement: Git history identity preservation
r[bounded_exec.radicle_publication.history] Bounded Exec MUST publish the reviewed source through Radicle without changing the Git object identities required by maintained consumers.

#### Scenario: Reviewed consumer revision is fetched
r[bounded_exec.radicle_publication.history.scenario.exact-revision]
- GIVEN a maintained consumer names the reviewed pre-pilot Git object ID
- WHEN that object is fetched from an admitted Radicle-backed endpoint
- THEN the fetched Git object and source content MUST match the existing reviewed identity exactly

#### Scenario: Publication would rewrite history
r[bounded_exec.radicle_publication.history.scenario.rewrite-rejected]
- GIVEN publication would squash, recreate, or otherwise change a required commit or tag identity
- WHEN publication admission runs
- THEN the pilot MUST reject publication before presenting the Radicle repository as canonical

### Requirement: Canonical Radicle repository identity
r[bounded_exec.radicle_publication.identity] Bounded Exec MUST record one canonical Radicle repository ID with explicit delegate, canonical-ref, and signed-reference policy before repository metadata names Radicle as canonical.

#### Scenario: Canonical identity is accepted
r[bounded_exec.radicle_publication.identity.scenario.accepted]
- GIVEN the published repository has the reviewed RID, delegate set, canonical default branch, canonical release-tag rules, and minimum signed-reference feature
- WHEN the publication receipt is verified
- THEN repository metadata MAY name that RID as the canonical project identity

#### Scenario: Endpoint is mistaken for identity
r[bounded_exec.radicle_publication.identity.scenario.endpoint-only]
- GIVEN an HTTPS mirror exposes repository bytes but no reviewed RID and authority policy are recorded
- WHEN canonical identity is evaluated
- THEN Bounded Exec MUST reject the endpoint as sufficient publication evidence

### Requirement: Independent Radicle source availability
r[bounded_exec.radicle_publication.availability] Bounded Exec MUST be replicated through the OnixOS catalog-declared independent seed paths and MUST remain exactly fetchable when any single admitted seed is unavailable.

#### Scenario: One seed is stopped
r[bounded_exec.radicle_publication.availability.scenario.seed-outage]
- GIVEN the reviewed object has replicated to every admitted seed
- WHEN one seed is unavailable and the surviving path is selected
- THEN the exact object MUST remain fetchable without consulting GitHub for Bounded Exec

#### Scenario: No admitted Radicle path is available
r[bounded_exec.radicle_publication.availability.scenario.no-fallback]
- GIVEN every admitted Radicle path is unavailable
- WHEN a pilot consumer requests Bounded Exec
- THEN acquisition MUST fail closed and MUST NOT silently fall back to GitHub

### Requirement: Aligned consumer source identity
r[bounded_exec.radicle_publication.consumer] Bounded Exec MUST provide a Radicle-backed Git source that allows maintained consumers to align Cargo and Nix on the same exact reviewed revision.

#### Scenario: Lattice consumes the pilot source
r[bounded_exec.radicle_publication.consumer.scenario.lattice]
- GIVEN Lattice names the canonical Bounded Exec RID through its admitted HTTPS Git endpoint
- WHEN Cargo and Nix acquire the dependency
- THEN both source paths MUST resolve the same exact Git object and existing focused consumer checks MUST retain their results

#### Scenario: Consumer source revisions differ
r[bounded_exec.radicle_publication.consumer.scenario.mismatch]
- GIVEN Cargo and Nix name different RIDs, revisions, or source content
- WHEN source identity validation runs
- THEN the consumer pilot MUST fail before a successful build or migration claim is accepted

### Requirement: Bounded publication evidence
r[bounded_exec.radicle_publication.evidence] Bounded Exec MUST publish BLAKE3-bound evidence for repository identity, authority policy, exact source acquisition, outage observations, consumer receipt linkage, and explicit non-claims.

#### Scenario: Complete publication evidence verifies
r[bounded_exec.radicle_publication.evidence.scenario.valid]
- GIVEN a receipt contains the required identities, bounded observations, consumer link, and non-claims
- WHEN publication verification runs
- THEN the receipt MUST validate deterministically against the current reviewed inputs

#### Scenario: Evidence is missing or tampered
r[bounded_exec.radicle_publication.evidence.scenario.invalid]
- GIVEN a receipt omits an identity or non-claim, names unsafe credential material, or contains a mismatched BLAKE3 value
- WHEN publication verification runs
- THEN verification MUST fail without treating Radicle transport as process correctness or release evidence
