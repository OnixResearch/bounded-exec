## Context

`ExecutionLimits` checks positive stdout and stderr limits before spawn. The shell then starts the child and creates capture workers.

Each worker creates `CaptureAccumulator::new(limit_bytes)`. The accumulator grows its retained vector while the child can already run.

## Decisions

### 1. Capture preparation is fallible and explicit

**Choice:** Add a constructor that reserves the complete retention limit and returns a typed error when allocation fails.

The constructor keeps the configured limit and an empty retained length. It does not read files, clocks, environment, or process state.

**Rationale:** A declared bound must become available capacity before execution depends on it.

### 2. Both output captures are prepared before spawn

**Choice:** The shell prepares stdout and stderr accumulators after request validation and before `Command::spawn()`.

If either preparation fails, the shell returns the corresponding stream error and starts no child.

**Rationale:** Preparing only one stream or preparing after spawn leaves partial-start behavior.

### 3. Workers receive owned prepared state

**Choice:** Each capture worker receives its prepared accumulator. `observe()` retains bytes only within existing capacity.

A debug or test invariant checks that retained capacity does not change after preparation.

**Rationale:** The capture loop must not regain hidden vector-growth behavior.

### 4. Existing capture semantics remain unchanged

**Choice:** Retain the first `limit_bytes`, count all observed bytes, and classify truncation from observed and retained lengths.

Counter overflow remains a typed capture error. Output floods continue to drain the pipe while discarding bytes beyond the prefix.

**Rationale:** The change moves allocation time. It does not change the product mechanism.

### 5. Compatibility cannot hide fallback behavior

**Choice:** Replace the infallible constructor or retain it only as an explicit test helper with documented failure behavior.

Production paths must use the fallible constructor. They must not fall back to `Vec::new()` after failed preparation.

## Functional Core and Imperative Shell

The `no_std + alloc` core owns limit checks, fallible vector reservation, retention, counters, and final classification.

The standard-library shell owns request sequencing, child creation, pipe ownership, worker threads, deadlines, cancellation, and teardown.

## Risks and Trade-offs

- Large declared limits consume memory before the child starts.
- Two-stream preparation can reject requests that previously started and failed later.
- API migration can affect direct users of `CaptureAccumulator::new()`.

## Validation

Positive tests cover empty output, exact-limit output, truncation, repeated chunks, and unchanged outcome classification.

Negative tests cover allocation failure, one stream failing after the other prepares, observed-byte overflow, output floods, cancellation, and worker teardown.

A spawn probe must show zero child starts when capture preparation fails.
