# Cairn policy provenance

The local lifecycle policy was initialized from the reviewed Cairn source revision `70db8fe114910feb5d162fea5db8ad4ffdb6f176` and narrowed to Bounded Exec's repository-owned requirement and evidence roots.

Nickel source in `contracts.ncl` and `default.ncl` is authoritative. Runtime validation consumes `generated/cairn-policy.json`; refresh it only with `cairn policy export` from the pinned or explicitly reviewed Cairn provider, then run policy freshness, lifecycle validation, gates, and traceability checks.
