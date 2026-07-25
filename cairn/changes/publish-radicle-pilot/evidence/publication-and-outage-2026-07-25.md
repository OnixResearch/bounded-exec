# Bounded Exec publication and outage evidence

Observed: 2026-07-25

## Identity and prerequisites

- Canonical RID: `rad:z2CpqLFpdP36fZXYUK5ZNWxMibpCo`
- Reviewed object: `29dac88ecded94457572db3fdfaaaab95fa91525`
- Object type: `commit`
- Source archive BLAKE3: `4fbbf8f0749262469f00748e04c775180488dba800303f139172656d25931927`
- Delegate policy: three delegates; two approvals for identity, canonical default branch, and release tags
- Signed-reference feature: `parent`
- Aspen bootstrap receipt BLAKE3: `6113f11993c56ed1086ec61dfc905cbd07f77cc85027f45c6066e7662e5a7c52`
- Desktop replica receipt BLAKE3: `c53a296c5bd277ba032ffa35574634690c9a9debc76eaa68fa51cdd77a527e24`
- OnixOS catalog BLAKE3: `f132c01a9d0edb4141201146c1dd5dc1eacfe77a2b080e6f99b2c4181d856655`
- OnixOS acceptance revisions: `b8425aa` and `39aba12`

All three delegates signed `main` at the reviewed object before public
announcement. The isolated publication checkout retained the existing commit
identity, reachable history, tags, and AGPL license.

## Acquisition

The public read-only endpoint
`https://git.onix.computer/z2CpqLFpdP36fZXYUK5ZNWxMibpCo.git`, Aspen1 native
seed, and desktop native replica each provided the reviewed commit and source
archive hash. The endpoint exposed no receive-pack route.

Unknown HTTPS RID, receive-pack query and POST, API route, and root route
returned `404`. The missing object
`1111111111111111111111111111111111111111` returned no object and failed with
HTTP `500`; this is rejection evidence only, not a healthy endpoint claim.

## Independent outage drills

The accepted desktop-only drill stopped Aspen1 and the transient operator node.
A fresh dynamic-user client was restricted to `100.110.43.11/32`, blocked from
seed storage, secrets, homes, public egress, and Aspen fallback, and acquired the
exact commit and source hash from desktop node
`z6MkkQCj5EczNiVzDzCkX9ewHNJ7NDEXSKbuRiS1x7o72yeG`. The undeclared Heartwood
RID and a missing object were rejected. Aspen1 was restored before acceptance.

The final Aspen-only drill stopped the persistent desktop node. A fresh
dynamic-user client with NID
`z6MkuAaWGg75i7kmb97mdsL6AKQqSwvfRdRhGpToiDKWmH3y` was restricted to
`100.100.103.95/32`, blocked from seed storage, secrets, homes, and public
egress, and acquired the same exact commit and source hash from Aspen1 node
`z6MkfpHAyrqSqhpiSGayy6AjB6L5UWkKLvsZvLh5hYD7XSu8`. The undeclared Heartwood
RID was rejected. The desktop node and policy timer were active after the trap
restored service.

## Consumer closeout

Lattice changed Cargo, Nix, Crane, and generated lock identities to the exact
Radicle-backed HTTPS source at the reviewed commit in revision `d69ec49`, then
archived its accepted change at `ad17c7d`. Its BLAKE3-bound cutover receipt is
`a5a1c7dd49729ef8d131b1bea57f3557c6dc5ed665fc04d44caf9a67a2bf04f9`.
OnixOS accepted that receipt at `4395bca`. The final publication receipt has
status `accepted` and BLAKE3
`30112693cadd92ba749be5b02ae148cb4560994de282e7342274b2f88e3ac894`.
No release is claimed by this evidence.

## Final validation

- `cargo test --workspace`: passed
- `cargo clippy --workspace --all-targets -- -D warnings`: passed
- `cargo fmt --all -- --check`: passed
- `cargo check -p bounded-exec-core --target wasm32-unknown-unknown`: passed
- publication receipt positive/negative Nickel check: passed
- Cairn proposal/design/tasks gates: passed
- `nix flake check -L`: passed on `x86_64-linux`; `aarch64-linux` was omitted as an incompatible local system
- Lattice focused source validator: 6 passed
- Lattice focused bounded-process matrix: 21 passed

## Non-claims

This evidence does not prove process correctness, consumer behavior, CI
correctness, release readiness, private confidentiality, automatic HTTPS
failover, geographic independence, protocol-level mandatory CI, or complete
third-party GitHub independence. Replication grants neither delegate nor
operational authority.
