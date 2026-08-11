# Resource Watermark Specification

## Purpose

Define an explicit resource-watermark admission decision for Bounded Exec so no run admits without explicit bounds.

## ADDED Requirements

### Requirement: Every bound is explicit

r[bounded_exec.watermark.policy] Bounded Exec MUST require an explicit resource observation and explicit high and low watermarks for admission. It MUST NOT impose an implicit default limit.

#### Scenario: Operator supplies a request

- GIVEN a request carries an explicit observation and explicit watermarks
- WHEN admission validation runs
- THEN the request MUST validate
- AND every bound MUST be explicit

#### Scenario: Request lacks explicit bounds

- GIVEN a request omits a bound or a watermark
- WHEN admission validation runs
- THEN validation MUST fail
- AND no implicit default MUST be assumed

### Requirement: The core admits from supplied values

r[bounded_exec.watermark.decision] The pure core MUST return admit or defer from a supplied observation and explicit watermarks. It MUST read no clock, memory, or scheduler state.

#### Scenario: Observation at or below the high watermark

- GIVEN an observation at or below the high watermark
- WHEN the core decides
- THEN the disposition MUST be admit

#### Scenario: Observation above the high watermark

- GIVEN an observation above the high watermark
- WHEN the core evaluates admission
- THEN the disposition MUST be defer
- AND admission MUST stay deferred until the low watermark is reached

### Requirement: Decisions do not prove execution

r[bounded_exec.watermark.boundary] Bounded Exec MUST NOT claim process execution, sandboxing, or resource-success authority. These remain caller-owned.

#### Scenario: Caller over-claims

- GIVEN a caller or documentation claims execution, sandboxing, or resource success from admission
- WHEN ownership and boundary verification runs
- THEN that claim MUST fail verification
- AND the caller-owned boundary MUST remain explicit

### Requirement: Failure coverage remains explicit

r[bounded_exec.watermark.verification] Positive and negative fixtures MUST cover every declared admission and boundary.

#### Scenario: Complete focused matrix passes

- GIVEN admission, fixtures, documentation, and dependency evidence are complete
- WHEN focused no-std, Wasm, package, workspace, Clippy, Cairn, and Nix verification runs
- THEN at-or-below-high inputs MUST admit and above-high inputs MUST defer until low
- AND each bound-less, reversed, coupled, or overclaiming input MUST fail as declared
