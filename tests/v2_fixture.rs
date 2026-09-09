use owls_runtime::{Policy, SchemaVersion, dependency_closure, parse_release};
use serde_json::Value;

#[test]
fn admitted_dioxus_fixture_preserves_v2_dependency_edges() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../zed_modules/ores-wasm-loaders/owls-interfaces/fixtures/valid/dioxus.json"
    ))
    .expect("admitted dioxus fixture");
    let release = parse_release(
        fixture,
        &Policy::new(vec!["https://assets.ores-wasm-loaders.test".into()]),
    )
    .expect("native host admits current interface fixture");
    assert_eq!(release.schema_version, SchemaVersion::V2);
    let reports = release
        .activation
        .as_ref()
        .and_then(|activation| activation.routes.as_ref())
        .and_then(|routes| routes.get("/app/reports"))
        .expect("fixture reports route");
    let closure = dependency_closure(&release, reports).expect("reports closure");
    assert!(
        closure.len() >= 2,
        "fixture must exercise at least one dependency edge"
    );
    assert_eq!(closure.last().expect("route target").id, *reports);
}
