# Input Delivery Specification

## Purpose

Defines the `input-delivery` capability.

## Requirements

### Requirement: Reader closure is not a run failure

r[bounded_exec.input_delivery.reader_closed] A stdin write that fails with broken pipe, connection reset, or connection aborted MUST end writing without failing the run, whether the child exited on its own, timed out, or was cancelled. It MUST NOT change the returned observation.

#### Scenario: The child exits without reading large input

- GIVEN piped input larger than any pipe buffer and a child that exits 0 without reading
- WHEN execution runs
- THEN completion MUST be exited with exit code 0 and a successful disposition
- AND the observation MUST equal the one for the same child with small input, except for the declared input size

#### Scenario: The child exits without reading small input

- GIVEN small piped input and a child that exits 0 without reading
- WHEN execution runs repeatedly and under parallel load
- THEN every run MUST return the identical successful observation
- AND no run MUST fail with a stdin write error

### Requirement: Input delivery is reported as a bounded fact

r[bounded_exec.input_delivery.fact] `ExecutionOutput` MUST report the offered input size and a typed input status, both derived only from the request and from whether a child was created. The status MUST distinguish no started child, null-device input, and piped input. The observation MUST NOT include any fact that depends on writer progress, such as bytes written or whether the read end closed first.

#### Scenario: The child reads all input

- GIVEN piped input and a child that reads it to end of file
- WHEN execution runs
- THEN input facts MUST report piped input with the offered size

#### Scenario: Input is the null device

- GIVEN a request whose input is the null device
- WHEN execution runs
- THEN input facts MUST report null-device input with zero offered bytes

#### Scenario: The child is stopped before reading

- GIVEN piped input larger than any pipe buffer and a child that is timed out or cancelled without reading
- WHEN execution runs
- THEN completion MUST stay timed out or cancelled
- AND input facts MUST report piped input with the offered size

### Requirement: Other stdin failures still fail the run

r[bounded_exec.input_delivery.write_errors] Every stdin write failure other than reader closure MUST still fail the run with its existing typed error. This includes other I/O errors, a zero-byte write, and a writer stopped by teardown.

#### Scenario: The writer sees a non-closure error

- GIVEN a stdin write that fails with an error other than broken pipe, connection reset, or connection aborted
- WHEN the writer observes it
- THEN the run MUST fail with a stdin write I/O error carrying that error

#### Scenario: Teardown stops a blocked writer

- GIVEN a stdin writer blocked on a pipe whose reader stays open
- WHEN teardown stops the writer
- THEN the writer MUST report a stdin teardown timeout

### Requirement: Delivery facts make no consumption claim

r[bounded_exec.input_delivery.boundary] Input facts MUST mean only the declared input size and how standard input was provided. Bounded Exec MUST NOT claim that the pipe accepted any byte, or that the child read, parsed, or acted on any byte.

#### Scenario: A consumer needs proof of consumption

- GIVEN a consumer that requires proof that the child consumed its input
- WHEN it evaluates input facts
- THEN those facts MUST NOT serve as that proof
- AND the consumer MUST obtain a child-side acknowledgement from the child's own output or protocol

### Requirement: Input delivery is verified under load

r[bounded_exec.input_delivery.verification] Verification MUST pair immediate-exit runs with pipe-exceeding and small input, repeated sequentially and under parallel load and compared for identical observations, with read-to-end runs, non-closure write errors, stopped writers, timeout and cancellation stops, and output-limit dispositions. The immediate-exit checks MUST fail when the completion-dependent closure guard is restored.

#### Scenario: Focused input checks run

- GIVEN unread, fully read, timed-out, cancelled, and output-flooding fixtures
- WHEN focused checks run
- THEN unread input MUST never fail a run or change its observation
- AND non-closure write errors MUST still fail

#### Scenario: The old guard is restored

- GIVEN a scratch copy where reader closure fails the run unless completion is not exited
- WHEN the immediate-exit checks run
- THEN the pipe-exceeding input check MUST fail
