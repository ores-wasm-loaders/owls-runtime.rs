use std::fs;
use std::path::Path;

fn workflow() -> String {
    fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join(".github/workflows/native-contract.yml"),
    )
    .expect("native contract workflow must be readable")
}

fn exact_ref(source: &str, name: &str) -> String {
    let prefix = format!("{name}:");
    let value = source
        .lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix(&prefix))
        .map(str::trim)
        .unwrap_or_else(|| panic!("missing {name}"));
    assert_eq!(value.len(), 40, "{name} must be a full commit SHA");
    assert!(
        value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "{name} must be lowercase hexadecimal",
    );
    value.to_owned()
}

#[test]
fn native_loader_admits_the_same_exact_contract_in_both_jobs() {
    let source = workflow();
    let interfaces = exact_ref(&source, "OWLS_INTERFACES_REF");
    let validator = exact_ref(&source, "TSJSV_REF");

    assert_ne!(interfaces, validator);
    assert!(source.contains("repository: ORESoftware/typespec-json-schema-validator"));
    assert!(source.contains("ref: ${{ env.TSJSV_REF }}"));
    assert!(source.contains("ref: ${{ env.OWLS_INTERFACES_REF }}"));
    assert!(source.contains("typespec-json-schema-validator.mjs check"));
    assert!(source.contains("--typespec=contracts/main.tsp"));
    assert!(source.contains("--schema=schemas/release.schema.json"));
    assert!(source.contains("--contract-ir=.typespec-json-schema-validator/contract-ir.json"));
    assert!(source.contains("node scripts/verify-contract-ir.mjs"));
    assert!(source.contains("node scripts/check-language-projections.mjs"));
    assert!(source.contains("include-hidden-files: true"));
    assert!(!source.contains("typespec-json-schema-validator\n          ref: main"));
    assert!(!source.contains("typespec-json-schema-validator\n          ref: master"));
}
