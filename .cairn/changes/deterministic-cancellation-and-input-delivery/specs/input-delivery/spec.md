# Input Delivery Specification

## Purpose

Make piped-input outcomes independent of scheduling. Report input delivery as a bounded fact instead of a scheduling-dependent failure.

## ADDED Requirements

### Requirement: Reader closure is not a run failure

r[bounded_exec.input_delivery.reader_closed] A stdin write that fails with broken pipe, connection reset, or connection aborted MUST end input delivery as reader-closed. It MUST NOT fail the run, whether the child exited on its own, timed out, or was cancelled.

#### Scenario: The child exits without reading large input

- GIVEN piped input larger than any pipe buffer and a child that exits without reading
- WHEN execution runs
- THEN the run MUST return an observation classified only by exit and output policy
- AND input delivery MUST report reader closure with fewer written bytes than offered bytes

#### Scenario: The child exits without reading small input

- GIVEN small piped input and a child that exits without reading
- WHEN execution runs repeatedly and under parallel load
- THEN every run MUST return the same successful disposition
- AND input delivery MUST report either every offered byte written or reader closure with fewer bytes written

### Requirement: Input delivery is reported as a bounded fact

r[bounded_exec.input_delivery.fact] `ExecutionOutput` MUST report offered input bytes, bytes written into the stdin pipe, and a typed delivery status. The status MUST distinguish no started child, null-device input, all bytes written with the pipe closed by the runner, and reader closure before all bytes were written.

#### Scenario: The child reads all input

- GIVEN piped input and a child that reads it to end of file
- WHEN execution runs
- THEN input delivery MUST report all bytes written
- AND the written-byte count MUST equal the offered-byte count

#### Scenario: Input is the null device

- GIVEN a request whose input is the null device
- WHEN execution runs
- THEN input delivery MUST report null-device input with zero offered and zero written bytes

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

r[bounded_exec.input_delivery.boundary] Input-delivery facts MUST mean only that the pipe accepted the reported bytes. They MUST NOT claim that the child read, parsed, or acted on any byte.

#### Scenario: A consumer needs proof of consumption

- GIVEN a consumer that requires proof that the child consumed its input
- WHEN it evaluates input-delivery facts
- THEN those facts MUST NOT serve as that proof
- AND the consumer MUST obtain consumption evidence from the child's own output or protocol

### Requirement: Input delivery is verified under load

r[bounded_exec.input_delivery.verification] Verification MUST pair unread-input runs, repeated sequentially and in parallel, with read-to-end runs, non-closure write errors, timeout stops, and output-limit dispositions.

#### Scenario: Focused input checks run

- GIVEN unread, fully read, timed-out, and output-flooding fixtures
- WHEN focused checks run
- THEN unread input MUST never fail a run
- AND non-closure write errors MUST still fail
