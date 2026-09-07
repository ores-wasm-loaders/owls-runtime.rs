# OWLS Rust native host

A shared loader for public WASM assets in Rust servers and native desktop applications. Uses zed-pkg for the shared contract dependency, locked Cargo dependencies for ecosystem libraries, and Wasmi for explicit raw-WASM execution.

`Host<T, S>` combines a strict release manifest, immutable policy, a `Transport`, and a `ByteStore`. `prefetch` checks budgets and verifies bytes without compiling or instantiating; it is optional preparation, not a prerequisite for activation. `bytes` reuses verified cache entries and refetches corrupt entries. `HttpTransport` disables redirects, uses HTTPS, requires a response content type compatible with the manifest asset kind, bounds decoded reads, and applies a total request timeout. Preparation is sequential and therefore bounded to one request per host.

Policy origins must be non-empty canonical HTTPS origins such as `https://assets.example`: no path, credentials, query, fragment, non-canonical casing, or trailing slash. Asset URLs remain exact canonical HTTPS URLs on one of those origins. Origin validation, content-type validation, byte limits, and SHA-256 verification are independent gates; none substitutes for another.

`NativeRuntime` accepts `raw-wasm` releases and enforces instruction fuel, memory, and instance limits. Configure a linker only for the capabilities your product intends to expose. No ambient WASI or process/network/filesystem imports exist. Browser wasm-bindgen glue and Flutter WasmGC require a browser host; the loader rejects those release types.

`MemoryStore` bounds resident bytes. `FileStore` writes digest-keyed entries atomically in a private per-product directory and limits individual reads/writes; the application owns total disk quota and old-release cleanup. The cancellation flag is checked between chunks; a blocking HTTP read is bounded by the configured request timeout. WASM execution is bounded by fuel, not interrupted by the download cancellation flag.

Compose or implement `Transport` and `ByteStore`, or provide specific Wasmi imports through the configuration callback. Consumers patch `owls-interfaces` to the root Zed-installed crate in their own `Cargo.toml`; see `ores-wasm-loaders-test/owls-e2e` for an actual external consumer.

Tests execute a WASM add function, exhaust fuel, enforce memory limits, reject malformed releases and non-canonical policies, enforce preparation budgets and cancellation, verify MIME/type boundaries, and verify persistent cache behavior. Run `cargo fmt --all -- --check`, `cargo clippy --locked --all-targets --all-features -- -D warnings`, and `cargo test --locked --all-features` after installing Zed dependencies.
