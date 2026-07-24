## Phase 1: Repository identity and publication

- [ ] [serial] Confirm the reviewed publication object, reachable history, existing tags, license, and clean source state before creating the canonical Radicle repository identity. r[bounded_exec.radicle_publication.history]
- [ ] [serial] Initialize and publish the repository without rewriting commits or tags, then record the RID, delegates, canonical refs, and minimum signed-reference policy. r[bounded_exec.radicle_publication.identity]
- [ ] [serial] Seed the repository through the independently operated OnixOS paths and record stable HTTPS Git endpoints for ordinary clients. r[bounded_exec.radicle_publication.availability]

## Phase 2: Exact-source evidence

- [ ] [parallel] Add a BLAKE3-bound publication receipt over the RID, reviewed Git object, authority policy, endpoint identities, acquisition results, and explicit non-claims. r[bounded_exec.radicle_publication.evidence]
- [ ] [parallel] Add positive checks that every admitted endpoint resolves the same reviewed Git object and source content. r[bounded_exec.radicle_publication.history]
- [ ] [parallel] Add negative checks for unknown RID, missing revision, altered source identity, mismatched authority policy, incomplete receipt, unavailable endpoint, and attempted GitHub fallback. r[bounded_exec.radicle_publication.evidence]

## Phase 3: Consumer and outage pilot

- [ ] [serial] Coordinate the independent Lattice Cargo/Nix source cutover while retaining the exact pre-pilot Bounded Exec revision. r[bounded_exec.radicle_publication.consumer]
- [ ] [serial] Stop each seed independently and prove the surviving configured path can provide the exact dependency without consulting GitHub for Bounded Exec. r[bounded_exec.radicle_publication.availability]
- [ ] [parallel] Run existing Bounded Exec verification and the focused Lattice dependency/source checks against the Radicle-backed source. r[bounded_exec.radicle_publication.consumer]

## Phase 4: Metadata and closeout

- [ ] [serial] Update Cargo repository metadata, `README.md`, and `ADOPTION.md` from the accepted RID and endpoint receipt while labeling GitHub as a transition mirror. r[bounded_exec.radicle_publication.identity]
- [ ] [parallel] Document clone, exact-revision consumption, outage behavior, rollback, and publication non-claims without adding Radicle runtime behavior to either crate. r[bounded_exec.radicle_publication.evidence]
- [ ] [serial] Run Cargo, Clippy, formatting, core WASM, Cairn, and Nix checks; sync and archive only after the OnixOS and Lattice evidence inputs are accepted. r[bounded_exec.radicle_publication.consumer]
