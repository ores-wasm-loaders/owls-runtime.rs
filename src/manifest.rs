use crate::{AssetKind, Error, Release, Result, Runtime};
use std::collections::BTreeSet;
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
    let schema: serde_json::Value =
        serde_json::from_str(owls_interfaces::RELEASE_SCHEMA).expect("compiled schema");
    let validator =
        jsonschema::validator_for(&schema).map_err(|e| Error::Manifest(e.to_string()))?;
    if !validator.is_valid(&value) {
        return Err(Error::Manifest("JSON Schema mismatch".into()));
    }
    // JSON Schema treats 8.0 as an integer. Normalize only after schema validation.
    value["schemaVersion"] = serde_json::json!(1);
    for asset in value["assets"].as_array_mut().expect("validated array") {
        asset["bytes"] = serde_json::json!(asset["bytes"].as_f64().expect("validated number") as u64);
    }
    let r: Release = serde_json::from_value(value).map_err(|e| Error::Manifest(e.to_string()))?;
    let mut ids = BTreeSet::new();
    let mut urls = BTreeSet::new();
    for a in &r.assets {
        let u = url::Url::parse(&a.url).map_err(|_| Error::Manifest("invalid URL".into()))?;
        if u.scheme() != "https"
            || !u.username().is_empty()
            || u.password().is_some()
            || u.query().is_some()
            || u.fragment().is_some()
            || u.as_str() != a.url
            || !policy.origins.contains(&u.origin().ascii_serialization())
        {
            return Err(Error::Manifest("canonical HTTPS allowlist required".into()));
        }
        if !ids.insert(&a.id) || !urls.insert(&a.url) {
            return Err(Error::Manifest("duplicate asset".into()));
        }
    }
    let expected = match r.runtime {
        Runtime::RawWasm => AssetKind::Wasm,
        Runtime::WasmBindgen => AssetKind::Module,
        Runtime::FlutterWeb => AssetKind::Script,
    };
    if !r
        .assets
        .iter()
        .any(|a| a.id == r.entrypoint && a.kind == expected)
    {
        return Err(Error::Manifest("invalid entrypoint".into()));
    }
    Ok(r)
}
