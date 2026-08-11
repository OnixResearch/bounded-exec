## Admission core

- [x] [serial] Add the explicit resource-watermark admission fact and decision to the core. r[bounded_exec.watermark.policy]
- [x] [serial] Implement the pure admit and defer decision from an explicit observation and explicit watermarks. r[bounded_exec.watermark.decision]
- [x] [serial] Document that the caller owns scheduling and resource enforcement. r[bounded_exec.watermark.boundary]

## Verification

- [x] [parallel] Add positive cases for admit at or below high and defer above high until low. r[bounded_exec.watermark.verification]
- [x] [parallel] Add negative and boundary cases for missing explicit bounds, reversed watermarks, an implicit default limit, and clock or memory reads. r[bounded_exec.watermark.verification]
- [x] [serial] Run no-std, Wasm, package, workspace, Clippy, Cairn, and Nix checks, then document non-claims. r[bounded_exec.watermark.verification]
