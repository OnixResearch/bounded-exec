## Goals

- Expose an explicit resource-watermark admission fact.
- Make a deterministic admit or defer decision from supplied values.
- Keep every execution bound explicit.
- Leave process execution, sandboxing, and resource enforcement with the caller.

## Admission fact

The core accepts these values:

1. An explicit resource observation, for example a resident count.
2. An explicit high watermark.
3. An explicit low watermark.

Every bound is explicit. No implicit default limit exists.

## Decision rules

The pure core returns one of two admissions:

- `Admit` — the observation is at or below the high watermark.
- `Defer` — the observation is above the high watermark, and admission stays deferred until the low watermark is reached.

The core accepts the observation and both watermarks as values. It reads no clock, memory, or scheduler state.

## Output

The decision contains public, bounded fields:

- the observation and both watermarks;
- the disposition;
- implementation and policy identities;
- required non-claims.

The decision does not run a process, prove execution, or prove sandboxing.

## Functional core and shell

The pure core computes the admission from supplied values.

The shell owns process setup, bounded I/O, deadlines, cancellation, and teardown. The caller owns scheduling and resource enforcement.

## Reference

The Celld watermark-shedding design is a bounded reference input. It is a comparison source, not a Bounded Exec requirement, parity claim, or equivalence claim.

## Verification

Positive coverage includes admit at or below high and defer above high until low.

Negative coverage includes a request without explicit bounds and reversed or missing watermarks.

Boundary coverage rejects an implicit default limit, clock or memory reads, process-run coupling, and execution-promotion claims.
