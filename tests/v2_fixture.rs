use std::collections::BTreeSet;

use owls_runtime::{Policy, Release, SchemaVersion, dependency_closure, parse_release};
use serde_json::Value;

/// The admitted `owls-interfaces` fixture corpus (pinned by the native contract workflow).
const FIXTURES: [(&str, &str); 4] = [
    (
        "dioxus",
        include_str!("../zed_modules/ores-wasm-loaders/owls-interfaces/fixtures/valid/dioxus.json"),
    ),
    (
        "flutter",
        include_str!(
            "../zed_modules/ores-wasm-loaders/owls-interfaces/fixtures/valid/flutter.json"
        ),
    ),
    (
        "leptos",
        include_str!("../zed_modules/ores-wasm-loaders/owls-interfaces/fixtures/valid/leptos.json"),
    ),
    (
        "legacy-v1",
        include_str!(
            "../zed_modules/ores-wasm-loaders/owls-interfaces/fixtures/valid/legacy-v1.json"
        ),
    ),
];

/// Normalized route closures (asset identifiers, dependency-first) that browser, Flutter and
/// native hosts must agree on for the admitted fixtures. These are consumer expectations over
/// the admitted wire contract, not a third contract authority.
const ROUTE_CLOSURES: [(&str, &str, &[&str]); 2] = [
    ("dioxus", "/app", &["chunks-app.wasm"]),
    (
        "dioxus",
        "/app/reports",
        &["chunks-app.wasm", "chunks-reports.wasm"],
    ),
];

fn admit(name: &str) -> Release {
    let (_, text) = FIXTURES
        .iter()
        .find(|(fixture, _)| *fixture == name)
        .expect("known fixture");
    let fixture: Value = serde_json::from_str(text).expect("fixture JSON");
    let origins: BTreeSet<String> = fixture["assets"]
        .as_array()
        .expect("fixture assets")
        .iter()
        .map(|asset| {
            let url = url::Url::parse(asset["url"].as_str().expect("asset url")).expect("url");
            url.origin().ascii_serialization()
        })
        .collect();
    parse_release(fixture, &Policy::new(origins.into_iter().collect()))
        .unwrap_or_else(|error| panic!("native host admits `{name}` fixture: {error}"))
}

fn closure_ids(release: &Release, id: &str) -> Vec<String> {
    dependency_closure(release, id)
        .expect("admitted closure")
        .into_iter()
        .map(|asset| asset.id.clone())
        .collect()
}

#[test]
fn admitted_dioxus_fixture_preserves_v2_dependency_edges() {
    let release = admit("dioxus");
    assert_eq!(release.schema_version, SchemaVersion::V2);
    let reports = release
        .activation
        .as_ref()
        .and_then(|activation| activation.routes.as_ref())
        .and_then(|routes| routes.get("/app/reports"))
        .expect("fixture reports route");
    let closure = closure_ids(&release, reports);
    assert!(
        closure.len() >= 2,
        "fixture must exercise at least one dependency edge"
    );
    assert_eq!(closure.last().expect("route target"), reports);
}

#[test]
fn admitted_route_closures_match_the_shared_normalized_expectations() {
    for (fixture, route, expected) in ROUTE_CLOSURES {
        let release = admit(fixture);
        let target = release
            .activation
            .as_ref()
            .and_then(|activation| activation.routes.as_ref())
            .and_then(|routes| routes.get(route))
            .unwrap_or_else(|| panic!("`{fixture}` declares route `{route}`"));
        assert_eq!(
            closure_ids(&release, target),
            expected.iter().map(|id| id.to_string()).collect::<Vec<_>>(),
            "`{fixture}` route `{route}` closure drifted",
        );
    }
}

#[test]
fn every_admitted_asset_closure_is_dependency_first_and_duplicate_free() {
    for (name, _) in FIXTURES {
        let release = admit(name);
        for asset in &release.assets {
            let closure = closure_ids(&release, &asset.id);
            assert_eq!(
                closure.last(),
                Some(&asset.id),
                "`{name}` closure ends at root"
            );
            let unique: BTreeSet<_> = closure.iter().collect();
            assert_eq!(
                unique.len(),
                closure.len(),
                "`{name}` closure is duplicate-free"
            );
            for (position, id) in closure.iter().enumerate() {
                let member = release
                    .assets
                    .iter()
                    .find(|candidate| &candidate.id == id)
                    .expect("closure member is admitted");
                for dependency in member.dependencies.as_deref().unwrap_or_default() {
                    let dependency_position = closure
                        .iter()
                        .position(|candidate| candidate == dependency)
                        .expect("dependency is inside the closure");
                    assert!(
                        dependency_position < position,
                        "`{name}`: `{dependency}` must precede `{id}`"
                    );
                }
            }
            if asset.dependencies.is_none() && name != "dioxus" {
                assert_eq!(
                    closure,
                    vec![asset.id.clone()],
                    "edge-free assets close over themselves"
                );
            }
        }
    }
}
