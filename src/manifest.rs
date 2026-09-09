use crate::{Asset, AssetKind, Error, Release, Result, Runtime};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone)]
pub struct Policy {
    pub origins: Vec<String>,
    pub max_asset_bytes: u64,
    pub max_prepare_bytes: u64,
}
impl Policy {
    pub fn new(origins: Vec<String>) -> Self {
        Self {
            origins,
            max_asset_bytes: 64 * 1024 * 1024,
            max_prepare_bytes: 8 * 1024 * 1024,
        }
    }
}

pub fn parse_release(mut value: serde_json::Value, policy: &Policy) -> Result<Release> {
    if policy.max_asset_bytes == 0 || policy.max_prepare_bytes == 0 {
        return Err(Error::Budget);
    }
    if policy.origins.is_empty()
        || policy
            .origins
            .iter()
            .any(|origin| !canonical_origin(origin))
    {
        return Err(Error::Manifest("canonical HTTPS origins required".into()));
    }
    let schema: serde_json::Value =
        serde_json::from_str(owls_interfaces::RELEASE_SCHEMA).expect("compiled schema");
    let validator =
        jsonschema::validator_for(&schema).map_err(|e| Error::Manifest(e.to_string()))?;
    if !validator.is_valid(&value) {
        return Err(Error::Manifest("JSON Schema mismatch".into()));
    }

    // JSON Schema treats 1.0/2.0 and integral byte counts as integers. Normalize the
    // serde_json representation only after schema validation without changing the admitted
    // schema version. The old host rewrote every document to v1 and therefore erased v2
    // dependency-DAG provenance.
    let schema_version = value["schemaVersion"]
        .as_f64()
        .expect("validated schemaVersion number");
    value["schemaVersion"] = serde_json::json!(schema_version as u64);
    for asset in value["assets"].as_array_mut().expect("validated array") {
        asset["bytes"] =
            serde_json::json!(asset["bytes"].as_f64().expect("validated number") as u64);
    }

    let release: Release =
        serde_json::from_value(value).map_err(|e| Error::Manifest(e.to_string()))?;
    let mut ids = BTreeSet::new();
    let mut urls = BTreeSet::new();
    for asset in &release.assets {
        let url = url::Url::parse(&asset.url).map_err(|_| Error::Manifest("invalid URL".into()))?;
        if url.scheme() != "https"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || url.as_str() != asset.url
            || !policy.origins.contains(&url.origin().ascii_serialization())
        {
            return Err(Error::Manifest("canonical HTTPS allowlist required".into()));
        }
        if !ids.insert(&asset.id) || !urls.insert(&asset.url) {
            return Err(Error::Manifest("duplicate asset".into()));
        }
    }

    for asset in &release.assets {
        let mut dependencies = BTreeSet::new();
        for dependency in asset.dependencies.as_deref().unwrap_or_default() {
            if dependency == &asset.id {
                return Err(Error::Manifest(format!(
                    "asset `{}` cannot depend on itself",
                    asset.id
                )));
            }
            if !ids.contains(dependency) {
                return Err(Error::Manifest(format!(
                    "asset `{}` depends on missing asset `{dependency}`",
                    asset.id
                )));
            }
            if !dependencies.insert(dependency) {
                return Err(Error::Manifest(format!(
                    "asset `{}` repeats dependency `{dependency}`",
                    asset.id
                )));
            }
        }
    }
    // Traversing each root also proves the admitted graph is acyclic. The contract caps the
    // graph at 512 assets and 64 edges per asset, so this simple deterministic walk is bounded.
    for id in &ids {
        dependency_closure(&release, id)?;
    }

    let expected = match release.runtime {
        Runtime::RawWasm => AssetKind::Wasm,
        Runtime::WasmBindgen => AssetKind::Module,
        Runtime::FlutterWeb => AssetKind::Script,
    };
    if !release
        .assets
        .iter()
        .any(|asset| asset.id == release.entrypoint && asset.kind == expected)
    {
        return Err(Error::Manifest("invalid entrypoint".into()));
    }
    Ok(release)
}

/// Return a dependency-first, duplicate-free closure ending with `asset_id`.
///
/// This is host behavior over the TJSV-admitted wire projection, not a new wire authority.
pub fn dependency_closure<'a>(release: &'a Release, asset_id: &str) -> Result<Vec<&'a Asset>> {
    let assets: BTreeMap<&str, &Asset> = release
        .assets
        .iter()
        .map(|asset| (asset.id.as_str(), asset))
        .collect();
    if !assets.contains_key(asset_id) {
        return Err(Error::Asset);
    }
    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    let mut ordered = Vec::new();
    visit_dependency(asset_id, &assets, &mut visiting, &mut visited, &mut ordered)?;
    Ok(ordered)
}

fn visit_dependency<'a>(
    id: &str,
    assets: &BTreeMap<&str, &'a Asset>,
    visiting: &mut BTreeSet<String>,
    visited: &mut BTreeSet<String>,
    ordered: &mut Vec<&'a Asset>,
) -> Result<()> {
    if visited.contains(id) {
        return Ok(());
    }
    if !visiting.insert(id.to_string()) {
        return Err(Error::Manifest(format!(
            "asset dependency cycle encountered at `{id}`"
        )));
    }
    let asset = assets
        .get(id)
        .copied()
        .ok_or_else(|| Error::Manifest(format!("dependency references missing asset `{id}`")))?;
    for dependency in asset.dependencies.as_deref().unwrap_or_default() {
        visit_dependency(dependency, assets, visiting, visited, ordered)?;
    }
    visiting.remove(id);
    visited.insert(id.to_string());
    ordered.push(asset);
    Ok(())
}

fn canonical_origin(value: &str) -> bool {
    let Ok(origin) = url::Url::parse(value) else {
        return false;
    };
    origin.scheme() == "https"
        && origin.host_str().is_some()
        && origin.username().is_empty()
        && origin.password().is_none()
        && origin.path() == "/"
        && origin.query().is_none()
        && origin.fragment().is_none()
        && origin.origin().ascii_serialization() == value
}
