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

#[test]
fn cargo_and_zed_package_identity_are_equal() {
    let cargo = read("Cargo.toml");
    let zed = read(".zpkg.toml");
    let cargo_package = assignments(&section(&cargo, "[package]"));
    let zed_package = assignments(&section(&zed, "[package]"));

    assert_eq!(cargo_package.get("name"), Some(&"owls-runtime".to_owned()));
    assert_eq!(zed_package.get("org"), Some(&"ores-wasm-loaders".to_owned()));
    assert_eq!(zed_package.get("name"), cargo_package.get("name"));
    assert_eq!(zed_package.get("version"), cargo_package.get("version"));
}

#[test]
fn cargo_zed_and_lock_use_one_exact_interface_release() {
    let cargo = read("Cargo.toml");
    let zed = read(".zpkg.toml");
    let lock = read(".zpkg.lock");

    let cargo_dependencies = assignments(&section(&cargo, "[dependencies]"));
    let zed_dependencies = assignments(&section(&zed, "[dependencies]"));
    let locked_package = assignments(&section(&lock, "[[package]]"));

    let cargo_requirement = cargo_dependencies
        .get("owls-interfaces")
        .expect("Cargo dependency must exist");
    let zed_requirement = zed_dependencies
        .get("ores-wasm-loaders/owls-interfaces")
        .expect("Zed dependency must exist");
    let locked_version = locked_package
        .get("version")
        .expect("lock version must exist");

    assert!(cargo_requirement.starts_with('='), "Cargo dependency must be exact");
    assert_eq!(cargo_requirement, zed_requirement);
    assert_eq!(cargo_requirement, &format!("={locked_version}"));
    assert_eq!(locked_package.get("org"), Some(&"ores-wasm-loaders".to_owned()));
    assert_eq!(locked_package.get("name"), Some(&"owls-interfaces".to_owned()));
    assert_eq!(
        locked_package.get("vcs_tag"),
        Some(&format!("v{locked_version}"))
    );
    assert_eq!(locked_package.get("format"), Some(&"tar.gz".to_owned()));

    let commit = locked_package
        .get("vcs_commit")
        .expect("lock commit must exist");
    let archive_sha = locked_package
        .get("sha256")
        .expect("lock archive digest must exist");
    assert!(is_lower_hex(commit, 40), "lock commit must be a full SHA-1");
    assert!(
        is_lower_hex(archive_sha, 64),
        "lock archive digest must be lowercase SHA-256"
    );
    assert!(
        locked_package
            .get("size")
            .expect("lock archive size must exist")
            .parse::<u64>()
            .is_ok_and(|size| size > 0),
        "lock archive size must be a positive integer"
    );
}
