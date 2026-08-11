# Capture Capacity Specification

## Purpose

Move bounded output-retention allocation before child creation while preserving Bounded Exec capture semantics and claim boundaries.

## ADDED Requirements

### Requirement: Capture capacity is prepared explicitly

r[bounded_exec.capture_capacity.preparation] Bounded Exec MUST provide fallible capture preparation that reserves the complete declared retention limit before output observation.

#### Scenario: Capture preparation succeeds

- GIVEN a positive explicit retention limit with available memory
- WHEN capture preparation runs
- THEN the prepared state MUST own capacity for that limit
- AND its retained length MUST start at zero

#### Scenario: Capture preparation fails

- GIVEN the declared retention capacity cannot be allocated
- WHEN capture preparation runs
- THEN it MUST return a typed capacity error
- AND it MUST NOT return an empty growable fallback

### Requirement: Both streams prepare before spawn

r[bounded_exec.capture_capacity.pre_spawn] The shell MUST prepare stdout and stderr capture state after request validation and before child creation.

#### Scenario: Both streams prepare

- GIVEN both declared stream capacities are available
- WHEN execution setup runs
- THEN both prepared states MUST exist before `Command::spawn()`

#### Scenario: One stream cannot prepare

- GIVEN one stream capacity cannot be allocated
- WHEN execution setup runs
- THEN setup MUST fail before child creation
- AND any capacity prepared for the other stream MUST be released

### Requirement: Capture observation does not grow capacity

r[bounded_exec.capture_capacity.steady_state] Prepared capture observation MUST retain the bounded prefix without increasing its vector capacity.

#### Scenario: Output fits the limit

- GIVEN a prepared accumulator and output no longer than its limit
- WHEN chunks are observed
- THEN all bytes MUST be retained in order
- AND capacity MUST remain equal to its prepared value

#### Scenario: Output exceeds the limit

- GIVEN output exceeds the prepared limit
- WHEN all chunks are observed
- THEN only the declared prefix MUST be retained
- AND all observed bytes MUST still contribute to truncation accounting

### Requirement: Existing boundaries remain explicit

r[bounded_exec.capture_capacity.boundary] Capture preparation MUST NOT add implicit limits or claim process execution, sandboxing, deterministic latency, or global zero allocation.

#### Scenario: A caller uses prepared capture

- GIVEN capture preparation succeeds
- WHEN a caller evaluates that result
- THEN the result MUST mean only that selected retention capacity was prepared
- AND execution and policy authority MUST remain outside that result

### Requirement: Verification covers positive and negative paths

r[bounded_exec.capture_capacity.verification] Implementation MUST pair successful capture tests with allocation, overflow, flood, cancellation, and cleanup failures.

#### Scenario: Focused verification runs

- GIVEN positive and negative core and shell fixtures
- WHEN focused checks run
- THEN valid retained-prefix behavior MUST remain unchanged
- AND failed preparation MUST produce zero child starts
