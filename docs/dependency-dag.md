# Native asset dependency DAG

`owls-runtime` consumes the dependency-DAG wire contract admitted by
`ores-wasm-loaders/owls-interfaces` at
`cfe0b18fe9ae361d94ea0646627cb732465b5496`.

The editable wire authorities remain the independently maintained TypeSpec and JSON Schema in
`owls-interfaces`. `ORESoftware/typespec-json-schema-validator` is pinned in the native contract
workflow and produces comparison/admission evidence (Schema B, parity receipt, Contract IR and
language-projection receipts); native Rust code is not a third authority.

Native admission preserves schema v2, validates dependency identifiers and rejects missing,
duplicate, self-referential and cyclic edges. `dependency_closure()` returns a deterministic,
dependency-first, duplicate-free closure. `Host::prefetch_asset()` applies explicit intent to that
closure while retaining the existing byte, integrity, cancellation and cache boundaries.

Ambient `Host::prefetch()` is intentionally unchanged: only assets whose authored `prepare` bit is
true participate. A lazy `prepare:false` asset does not become speculative work merely because it
is named by another asset; explicit intent is required.

Native execution remains restricted to `raw-wasm`. This DAG support does not make the native host a
browser wasm-bindgen, Dioxus, Leptos or Flutter execution environment and does not grant filesystem,
network, WASI or process capabilities.
