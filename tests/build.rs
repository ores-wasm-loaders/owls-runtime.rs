use owls_runtime::{Runtime, inspect_build};
#[test]
fn manifest_is_derived_from_actual_output_and_rejects_guessed_paths() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("custom.wasm"),
        [0, 97, 115, 109, 1, 0, 0, 0],
    )
    .unwrap();
    let r = inspect_build(
        dir.path(),
        "https://assets.example/r1/",
        "demo",
        "r1",
        Runtime::RawWasm,
        "custom.wasm",
        &["custom.wasm".into()],
    )
    .unwrap();
    assert_eq!(r.assets[0].bytes, 8);
    assert!(r.assets[0].prepare);
    assert_eq!(r.assets[0].url, "https://assets.example/r1/custom.wasm");
    assert!(
        inspect_build(
            dir.path(),
            "https://assets.example/r1/",
            "demo",
            "r1",
            Runtime::RawWasm,
            "main.wasm",
            &[]
        )
        .is_err()
    );
}
