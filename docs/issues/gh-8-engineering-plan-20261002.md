# Engineering plan for #8: DEN-3959: preserve and traverse admitted asset dependency DAG in native Rust

Tracks https://github.com/ores-wasm-loaders/owls-runtime.rs/issues/8

## OWLS invariants

- Preparation remains fetch/verify-only and never executes application code.
- Activation owns runtime effects and consumes only admitted immutable release/dependency evidence.
- Release IDs and admitted asset identities are immutable; conflicting content fails closed.
- JSON Schema and TypeSpec remain independent peer contract authorities where contract changes are involved.
- Native execution keeps ambient filesystem/network/process capability disabled unless explicitly admitted.
- Flutter/native caches do not become browser/runtime authority and dependency ordering remains explicit.

## Verification

- Add deterministic positive fixtures for the dependency/release graph.
- Add negative fixtures for missing nodes, cycles, stale release IDs, digest mismatch, unsupported host capability, and preparation/activation boundary violations.
- Exercise exact-head package/runtime tests and cross-host parity where required.
- Bind restored registry evidence or generated artifacts to immutable source and checksum provenance.
- Treat skipped/zero-step CI as non-evidence.

This draft records the bounded implementation path; #8 remains open until executable behavior and exact-head proof land.
