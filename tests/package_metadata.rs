use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

fn read(name: &str) -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(name))
        .unwrap_or_else(|error| panic!("cannot read {name}: {error}"))
}

fn section<'a>(source: &'a str, header: &str) -> Vec<&'a str> {
    let mut lines = source.lines();
    for line in lines.by_ref() {
        if line.trim() == header {
            return lines
                .take_while(|line| !line.trim_start().starts_with('['))
                .collect();
        }
    }
    panic!("missing {header} section");
}

fn assignments(lines: &[&str]) -> BTreeMap<String, String> {
    lines
        .iter()
        .filter_map(|line| {
            let line = line.split('#').next()?.trim();
            if line.is_empty() {
                return None;
            }
            let (key, value) = line.split_once('=')?;
            Some((
                key.trim().trim_matches('"').to_owned(),
                value.trim().trim_matches('"').to_owned(),
            ))
        })
        .collect()
}

fn is_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn value<'a>(assignments: &'a BTreeMap<String, String>, key: &str) -> &'a str {
    assignments
        .get(key)
        .map(String::as_str)
        .unwrap_or_else(|| panic!("missing {key}"))
}

#[test]
fn cargo_and_zed_package_identity_are_equal() {
    let cargo = read("Cargo.toml");
    let zed = read(".zpkg.toml");
    let cargo_package = assignments(&section(&cargo, "[package]"));
    let zed_package = assignments(&section(&zed, "[package]"));

    assert_eq!(value(&cargo_package, "name"), "owls-runtime");
    assert_eq!(value(&zed_package, "org"), "ores-wasm-loaders");
    assert_eq!(value(&zed_package, "name"), value(&cargo_package, "name"));
    assert_eq!(
        value(&zed_package, "version"),
        value(&cargo_package, "version")
    );
}

#[test]
fn cargo_zed_and_lock_use_one_exact_interface_release() {
    let cargo = read("Cargo.toml");
    let zed = read(".zpkg.toml");
    let lock = read(".zpkg.lock");

    let cargo_dependencies = assignments(&section(&cargo, "[dependencies]"));
    let zed_dependencies = assignments(&section(&zed, "[dependencies]"));
    let locked_package = assignments(&section(&lock, "[[package]]"));

    let cargo_requirement = value(&cargo_dependencies, "owls-interfaces");
    let zed_requirement = value(&zed_dependencies, "ores-wasm-loaders/owls-interfaces");
    let locked_version = value(&locked_package, "version");

    assert!(
        cargo_requirement.starts_with('='),
        "Cargo dependency must be exact"
    );
    assert_eq!(cargo_requirement, zed_requirement);
    assert_eq!(cargo_requirement, format!("={locked_version}"));
    assert_eq!(value(&locked_package, "org"), "ores-wasm-loaders");
    assert_eq!(value(&locked_package, "name"), "owls-interfaces");
    assert_eq!(
        value(&locked_package, "vcs_tag"),
        format!("v{locked_version}")
    );
    assert_eq!(value(&locked_package, "format"), "tar.gz");

    let commit = value(&locked_package, "vcs_commit");
    let archive_sha = value(&locked_package, "sha256");
    assert!(is_lower_hex(commit, 40), "lock commit must be a full SHA-1");
    assert!(
        is_lower_hex(archive_sha, 64),
        "lock archive digest must be lowercase SHA-256"
    );
    assert!(
        value(&locked_package, "size")
            .parse::<u64>()
            .is_ok_and(|size| size > 0),
        "lock archive size must be a positive integer"
    );
}
