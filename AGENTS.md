# OWLS native Rust package
Keep host policy, byte transport/storage, and WASM engine integration separate.
Native execution accepts raw-wasm only. Browser glue and Flutter WasmGC need their browser hosts.
No ambient WASI, filesystem, network or process imports. Fuel and memory limits must remain enabled.
Read ~/codes/AGENTS.md or .ores/agents/AGENTS.md when available. Never commit .ores/.

