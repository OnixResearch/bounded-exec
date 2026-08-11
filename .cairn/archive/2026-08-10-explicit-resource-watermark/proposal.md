## Why

Bounded Exec owns the bounded child-process mechanism. It already requires every execution bound to be explicit in `RunRequest`. It does not yet expose a resource-watermark admission fact that a caller can enforce before it admits a run.

Celld sheds pressure with explicit high and low watermarks: above the high watermark it parks idle cells and refuses to reacquire released cells until the low watermark is reached. That discipline transfers to child-process admission: an admission decision must use explicit watermarks over an explicit observation, and it must never fall back to an implicit default.

Bounded Exec should own the deterministic admission-watermark decision. The caller owns process execution, sandboxing, and resource enforcement.

## What Changes

- Add an optional, explicit resource-watermark admission fact to the core.
- Make a deterministic admit or defer decision from a supplied observation and explicit watermark values.
- Keep every bound explicit; reject any request without explicit bounds.
- Add positive, negative, and boundary fixtures for admit, defer, and missing-bound cases.
- Reference the reviewed Celld watermark-shedding design as a bounded, non-parity input.

## Impact

The change gives callers a deterministic admission signal over explicit bounds. It does not run, kill, or sandbox a process.

Admission-watermark identities and receipts remain BLAKE3.

## Dependencies

This change has no downstream prerequisite.

## Non-goals

- Do not add a scheduler or a process manager.
- Do not read clocks, memory, or CPU load in the core.
- Do not add an implicit default limit.
- Do not claim parity with, or equivalence to, the Celld shedding implementation.
- Do not convert an admission decision into a proof of successful execution or sandboxing.
