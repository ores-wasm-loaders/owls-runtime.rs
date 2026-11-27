# Delivery plan for #1: Implement shared WASM loading and zed-pkg distribution

Tracks https://github.com/ores-wasm-loaders/owls-runtime.rs/issues/1

## Scope

Define the first bounded delivery slice for the shared OWLS/WASM loading program without overstating runtime capability or weakening package/contract provenance.

## Guardrails

- Preparation fetches and verifies only; activation performs runtime effects.
- Release/package identities are immutable and digest-bound.
- TypeSpec and JSON Schema remain independent peer authorities for shared contracts.
- Generated framework glue and public documentation remain downstream of tested implementation evidence.
- Native/browser/Flutter hosts keep their capability boundaries explicit; no ambient capability is inferred.
- Zed package coordinates and dependencies are immutable/reproducible for release evidence.
- Public site claims point to actual tested repository capabilities.

## Verification

- Deterministic build/package fixtures.
- Negative tests for stale/mismatched digests, unsupported host capability, dependency graph failure, and preparation/activation leakage.
- Exact-head format/lint/test/package checks appropriate to this repository.
- Public/docs output checked against source-bound capability evidence.
- Zero-step or skipped CI remains non-evidence.

This draft advances #1; executable implementation and exact-head certification remain required before issue closure.
