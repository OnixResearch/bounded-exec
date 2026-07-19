# Agent Notes

- Preserve the `bounded-exec-core` `no_std` functional-core boundary.
- Keep filesystem discovery, environment reads, executable authorization, sandboxing, remote transport, output parsing, evidence, and lifecycle policy consumer-owned.
- Keep the standard-library shell limited to explicit process setup, bounded I/O, deadlines, cancellation, and owned teardown.
- Never add implicit default limits; every execution bound must remain explicit in `RunRequest`.
- Use named constants for every nontrivial numeric value.
- Add positive and negative tests together.
- Do not add sibling-path dependencies to consumers. Adoption requires an exact published Git revision.
- Run Cargo tests, Clippy, formatting, the core wasm check, and `nix flake check` before release claims.
- Do not push unless explicitly requested.
