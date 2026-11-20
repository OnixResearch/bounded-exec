# Cancellation Specification

## Purpose

Make an explicit cancellation flag that is already raised stop execution before any child is created, while keeping cancellation after spawn unchanged.

## ADDED Requirements

### Requirement: A raised flag prevents child creation

r[bounded_exec.cancellation.pre_spawn] The shell MUST read the cancellation flag after request validation and capture preparation and before child creation. When the flag is raised, the shell MUST return a cancelled observation without creating a child.

#### Scenario: The flag is raised before execution

- GIVEN a valid request and a cancellation flag that is already raised
- WHEN execution runs with that flag
- THEN no child process MUST be created
- AND completion and disposition MUST both be cancelled
- AND the exit code and the signal MUST be absent
- AND both captures MUST be empty, with zero observed bytes and no truncation
- AND input delivery MUST report that no child started and no bytes were written

#### Scenario: The request is invalid and the flag is raised

- GIVEN an invalid request, or capture capacity that cannot be prepared, and a raised cancellation flag
- WHEN execution runs with that flag
- THEN the validation or preparation error MUST be returned
- AND no cancelled observation MUST be returned

### Requirement: Cancellation after spawn is unchanged

r[bounded_exec.cancellation.post_spawn] When the flag is raised after child creation, the shell MUST tear down the owned processes, reap the direct child, and return a cancelled observation with the termination facts reported by reaping.

#### Scenario: The flag is raised while the child runs

- GIVEN a child that has started and is still running
- WHEN the cancellation flag is raised
- THEN owned processes MUST be terminated and the direct child reaped
- AND completion and disposition MUST both be cancelled
- AND the reaped termination signal MUST be reported where the platform supplies it

### Requirement: Cancellation is verified with spawn proofs

r[bounded_exec.cancellation.verification] Verification MUST pair a repeated pre-raised-flag proof, which shows zero child starts through a marker file, with a positive control, which shows the same request does start, and with a cancellation-after-spawn test.

#### Scenario: Spawn proofs run repeatedly

- GIVEN a command that creates a marker file as its first action
- WHEN it runs many times with a pre-raised flag
- THEN no marker MUST exist after any run
- AND the same command without a raised flag MUST create the marker
