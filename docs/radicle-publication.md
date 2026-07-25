# Radicle publication

Bounded Exec's canonical project identity is
`rad:z2CpqLFpdP36fZXYUK5ZNWxMibpCo`. The reviewed pilot source remains the exact
Git object `29dac88ecded94457572db3fdfaaaab95fa91525`; publication did not rewrite
that commit or its reachable history.

The accepted publication receipt is generated from
[`evidence/radicle/publication-v1.ncl`](../evidence/radicle/publication-v1.ncl).
Its deterministic JSON digest is
`30112693cadd92ba749be5b02ae148cb4560994de282e7342274b2f88e3ac894`.
The receipt links the accepted Onix Core bootstrap and replica evidence, OnixOS
catalog admission, Lattice cutover receipt `a5a1c7dd…`, and OnixOS consumer
acceptance revision `4395bca`.

## Clone and exact-revision consumption

Ordinary Git, Cargo, and Nix clients use the admitted read-only HTTPS adapter:

```sh
git clone https://git.onix.computer/z2CpqLFpdP36fZXYUK5ZNWxMibpCo.git bounded-exec
git -C bounded-exec checkout 29dac88ecded94457572db3fdfaaaab95fa91525
```

Cargo consumers pin the same URL and `rev` without a GitHub fallback. Nix
consumers must name the same URL and revision and regenerate their lock with
Nix. The RID is the repository identity; the HTTPS URL is only an acquisition
adapter.

Native Radicle replication is provided by these independent selective seeds:

- `z6MkfpHAyrqSqhpiSGayy6AjB6L5UWkKLvsZvLh5hYD7XSu8@100.100.103.95:8776`
- `z6MkkQCj5EczNiVzDzCkX9ewHNJ7NDEXSKbuRiS1x7o72yeG@100.110.43.11:8776`

## Acquisition and outage evidence

HTTPS, Aspen1 native acquisition, and desktop-replica native acquisition all
resolved `main` to the reviewed commit and produced source-archive BLAKE3
`4fbbf8f0749262469f00748e04c775180488dba800303f139172656d25931927`.

The outage drills stopped one persistent seed at a time. Each fresh client was
restricted to the surviving seed's `/32`, denied access to seed storage,
secrets, homes, and public fallback, and required to acquire the exact commit
and source hash. The undeclared Heartwood RID was rejected in both isolation
boundaries. The desktop-only drill additionally rejected a missing object.
Both stopped services were restored before acceptance.

HTTPS rejects unknown RIDs, receive-pack, API, and root routes with `404`. A
missing object was rejected with `500` and returned no object; that result is
accepted only as fail-closed rejection, not as a healthy endpoint response.

## Rollback

Before Lattice cutover, rollback is simply to leave Lattice's existing exact
source unchanged. After cutover, rollback means reverting Lattice's source and
both owning lockfiles together to the previously reviewed source declaration.
Do not add an automatic GitHub fallback: a Radicle acquisition failure must be
visible and fail closed.

GitHub may remain a transition mirror while migration proceeds. It is not an
admitted fallback for Bounded Exec dependency acquisition.

## Non-claims

Publication and replication do not prove Bounded Exec process correctness,
consumer behavior, CI correctness, patch-review correctness, release
readiness, private-repository confidentiality, automatic HTTPS failover,
geographic independence, protocol-level mandatory CI, or whole-stack GitHub
independence. A seed stores public repository bytes but receives no delegate,
canonical-ref, release, deployment, CI, cache-write, backup, Cloudflare, or
artifact-administration authority from replication.
