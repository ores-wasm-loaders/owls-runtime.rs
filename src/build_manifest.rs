use crate::{
    Asset, AssetKind, Error, Policy, Release, Result, Runtime, SchemaVersion, parse_release,
};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
};

/// Inspect real build output. No conventional Flutter/Rust filenames are assumed.
/// Caller explicitly supplies entrypoint and prepare paths relative to the build directory.
pub fn inspect_build(
    root: &Path,
    base_url: &str,
    app_id: &str,
    release_id: &str,
    runtime: Runtime,
    entrypoint: &str,
    prepare: &[String],
) -> Result<Release> {
    let base =
        url::Url::parse(base_url).map_err(|_| Error::Manifest("invalid asset base".into()))?;
    if !base.path().ends_with('/') {
        return Err(Error::Manifest("asset base must end in slash".into()));
    }
    let mut paths = Vec::new();
    collect(root, root, &mut paths)?;
    paths.sort();
    let mut assets = Vec::new();
    let mut entry = None;
    for (index, path) in paths.iter().enumerate() {
        let relative = path
            .strip_prefix(root)
            .map_err(|_| Error::Manifest("outside build root".into()))?
            .to_str()
            .ok_or_else(|| Error::Manifest("non UTF-8 path".into()))?
            .replace('\\', "/");
        let kind = match path.extension().and_then(|v| v.to_str()).unwrap_or("") {
            "wasm" => AssetKind::Wasm,
            "mjs" => AssetKind::Module,
            "js" => {
                if runtime == Runtime::WasmBindgen {
                    AssetKind::Module
                } else {
                    AssetKind::Script
                }
            }
            "woff" | "woff2" | "ttf" | "otf" => AssetKind::Font,
            _ => AssetKind::Data,
        };
        let mut bytes = Vec::new();
        std::fs::File::open(path)?
            .take(268435457)
            .read_to_end(&mut bytes)?;
        if bytes.is_empty() || bytes.len() > 268435456 {
            return Err(Error::Budget);
        }
        let id = format!("asset-{index}");
        if relative == entrypoint {
            entry = Some(id.clone());
        }
        let mut url = base.clone();
        {
            let mut segments = url
                .path_segments_mut()
                .map_err(|_| Error::Manifest("invalid base".into()))?;
            segments.pop_if_empty();
            for part in relative.split('/') {
                segments.push(part);
            }
        }
        assets.push(Asset {
            id,
            url: url.into(),
            kind,
            role: None,
            stage: None,
            dependencies: None,
            bytes: bytes.len() as u64,
            sha256: format!("{:x}", Sha256::digest(&bytes)),
            prepare: prepare.contains(&relative),
        });
    }
    if prepare.iter().any(|p| {
        !paths
            .iter()
            .any(|v| v.strip_prefix(root).ok().and_then(Path::to_str) == Some(p.as_str()))
    }) {
        return Err(Error::Manifest("prepare path missing from build".into()));
    }
    let release = Release {
        schema_version: SchemaVersion::V2,
        app_id: app_id.into(),
        release: release_id.into(),
        runtime,
        framework: None,
        toolchain: None,
        entrypoint: entry.ok_or_else(|| Error::Manifest("entrypoint absent from build".into()))?,
        requires_cross_origin_isolation: None,
        assets,
        prepare_budget: None,
        activation: None,
        extensions: BTreeMap::new(),
    };
    parse_release(
        serde_json::to_value(release).map_err(|e| Error::Manifest(e.to_string()))?,
        &Policy::new(vec![base.origin().ascii_serialization()]),
    )
}
fn collect(root: &Path, directory: &Path, paths: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            return Err(Error::Manifest("build symlinks are not supported".into()));
        }
        if kind.is_dir() {
            collect(root, &entry.path(), paths)?;
        } else if kind.is_file() {
            if entry.path().strip_prefix(root).is_err() {
                return Err(Error::Manifest("outside root".into()));
            }
            paths.push(entry.path());
            if paths.len() > 512 {
                return Err(Error::Budget);
            }
        }
    }
    Ok(())
}
