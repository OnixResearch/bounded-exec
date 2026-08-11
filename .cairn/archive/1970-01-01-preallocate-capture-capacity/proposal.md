## Why

Bounded Exec requires explicit stdout and stderr retention limits. `CaptureAccumulator` still starts with an empty vector and grows while output is read.

The child process starts before capture storage is allocated. A permitted request can therefore start a child and then fail because retention memory is unavailable.

Bounded Exec needs a fallible capture-preparation step before spawn. The step must preserve the current bounded-prefix and truncation semantics.

## What Changes

- Add a fallible capture-capacity constructor that reserves the complete declared retention limit.
- Prepare stdout and stderr capture storage before child creation.
- Pass prepared capture state into the capture workers without later growth.
- Fail before spawn when either capture allocation fails.
- Preserve bounded-prefix retention, total observed-byte accounting, truncation classification, cancellation, and teardown behavior.
- Add positive and negative tests for exact limits, allocation failure, overflow, truncation, and cleanup.

## Impact

- **Core**: a fallible prepared-capture state and typed capacity-preparation error.
- **Shell**: pre-spawn preparation and transfer of prepared state into output workers.
- **Compatibility**: public constructor changes require an explicit migration or compatibility wrapper with no hidden allocation fallback.
- **Claims**: successful preparation does not prove child execution, sandboxing, latency, or host memory stability.

## Dependencies

This change has no downstream prerequisite. It can proceed beside `explicit-resource-watermark` without merging caller-owned admission policy into capture mechanics.

## Non-Goals

- Do not add implicit output limits.
- Do not add a general allocator, buffer pool, scheduler, or process manager.
- Do not preallocate child input or unrelated process resources.
- Do not change output from a retained prefix to a ring or suffix.
- Do not claim that the process performs no dynamic allocation after preparation.
