# Radicle publication

## Canonical identity

Bounded Exec's canonical project identity is `rad:zG25DRmacxYQcXzZmLKj8tmgYbEG`.
Its single delegate is
`did:key:z6MksnXbFoE8zkCkGWhHc8zuxpnEUhrJHv2KECRV4GSv9gkx` with threshold 1.
Canonical `refs/tags/*` follow the same delegate at threshold 1. More delegates
are to be added once their keys have backups. The repository was initialized
from the existing Git history without rewriting any commit.

The intended read-only HTTPS adapter is
`https://git.onix.computer/zG25DRmacxYQcXzZmLKj8tmgYbEG.git`. Seed replication
and adapter admission for this RID are pending in onix-core and the OnixOS
catalog. Until a publication receipt for this RID is accepted, no revision of
it is an admitted consumer source.

## Frozen predecessor

The previous identity, `rad:z2CpqLFpdP36fZXYUK5ZNWxMibpCo`, is frozen. It
requires two of three delegate signatures, and two of those three delegate
keys are lost. Its canonical `main` stays at
`10e2ac43ca27f1dcadbe67a654cd185f60c0c3d3`, and its identity can no longer
change. It remains served read-only for consumers pinned to the reviewed pilot
source `29dac88ecded94457572db3fdfaaaab95fa91525`, which that canonical `main`
contains:

```sh
git clone https://git.onix.computer/z2CpqLFpdP36fZXYUK5ZNWxMibpCo.git bounded-exec
git -C bounded-exec checkout 29dac88ecded94457572db3fdfaaaab95fa91525
```

Later revisions are not admitted from the frozen identity. A revision that only
one delegate signed never becomes canonical there, and Cargo fetches only
branch heads from non-GitHub hosts.

The accepted pilot publication receipt for the frozen identity is generated
from [`evidence/radicle/publication-v1.ncl`](../evidence/radicle/publication-v1.ncl).
Its deterministic JSON digest is
`30112693cadd92ba749be5b02ae148cb4560994de282e7342274b2f88e3ac894`.
The receipt links the accepted Onix Core bootstrap and replica evidence, OnixOS
catalog admission, Lattice cutover receipt `a5a1c7dd…`, and OnixOS consumer
acceptance revision `4395bca`. It covers only the frozen identity and
`29dac88ecded94457572db3fdfaaaab95fa91525`.

## Exact-revision consumption

Cargo consumers pin the admitted HTTPS URL and an exact `rev` without a GitHub
fallback. Nix consumers must name the same URL and revision and regenerate
their lock with Nix. The RID is the repository identity; the HTTPS URL is only
an acquisition adapter.

The pilot's independent selective seeds were:

- `z6MkfpHAyrqSqhpiSGayy6AjB6L5UWkKLvsZvLh5hYD7XSu8@100.100.103.95:8776`
- `z6MkkQCj5EczNiVzDzCkX9ewHNJ7NDEXSKbuRiS1x7o72yeG@100.110.43.11:8776`

The OnixOS catalog for the canonical identity decides its admitted seeds.

## Pilot acquisition and outage evidence

This section records the frozen identity's accepted pilot evidence.

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
both owning lockfiles together to the previously reviewed source declaration,
including its RID and adapter URL. Do not add an automatic GitHub fallback: a
Radicle acquisition failure must be visible and fail closed.

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
