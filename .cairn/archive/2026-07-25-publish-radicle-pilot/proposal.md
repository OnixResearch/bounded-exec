## Why

Bounded Exec is already pinned by Lattice at an exact Git revision, but that source is acquired from GitHub. It is the smallest dependency-bearing repository that can prove whether Radicle publication preserves Git object identity, works through ordinary Cargo and Nix Git acquisition, and survives a seed outage without changing process behavior. Publication cannot begin until `onix-core` supplies an accepted persistent node and seed-backed HTTPS endpoint.

Publishing a mirror is not sufficient. The pilot must record one canonical Radicle repository identity, preserve existing history, expose the exact reviewed revision through independently operated seeds, and provide evidence a consumer can verify without treating transport success as a correctness or release claim.

## What Changes

- Verify the accepted `onix-core` bootstrap-node receipt and OnixOS catalog admission before creating the Radicle repository identity.
- Publish the existing Git history and reviewed revision under one Radicle repository identity without rewriting commits or tags.
- Record delegates, canonical refs, signed-reference policy, seed endpoints, HTTPS clone endpoints, and BLAKE3-bound publication evidence.
- Prove the current Lattice revision is byte-identical through Radicle and remains consumable when either seed is unavailable.
- Update repository metadata and operator documentation only after the Radicle identity and endpoints pass the acceptance drills.
- Retain GitHub as a read-only transition mirror during the pilot, with no fallback from the Radicle consumer path.

## Impact

- **Files**: Cairn lifecycle artifacts, repository metadata, `ADOPTION.md`, `README.md`, and publication evidence; process core and shell behavior remain unchanged.
- **Cross-repo input**: the `onix-core` bootstrap/deployment receipts provide concrete node and endpoint facts, OnixOS admits the catalog and multi-seed availability evidence, and Lattice owns the consumer cutover.
- **Testing**: exact-object identity checks, independent-seed acquisition, negative RID/revision/fallback tests, existing Cargo/Nix verification, and Cairn gates.
