use std::fs;
use std::path::Path;

const EXPECTED_DECLARATIONS: &[&str] = &[
    "Ores.WasmLoaders.Activation",
    "Ores.WasmLoaders.ActivationMode",
    "Ores.WasmLoaders.ApplicationId",
    "Ores.WasmLoaders.Asset",
    "Ores.WasmLoaders.AssetId",
    "Ores.WasmLoaders.AssetKind",
    "Ores.WasmLoaders.AssetRole",
    "Ores.WasmLoaders.AssetStage",
    "Ores.WasmLoaders.EntrypointId",
    "Ores.WasmLoaders.FrameworkKind",
    "Ores.WasmLoaders.HostSelector",
    "Ores.WasmLoaders.HttpsAssetUrl",
    "Ores.WasmLoaders.IslandName",
    "Ores.WasmLoaders.PrepareBudget",
    "Ores.WasmLoaders.PrepareStage",
    "Ores.WasmLoaders.RecordString",
    "Ores.WasmLoaders.RecordUnknown",
    "Ores.WasmLoaders.Release",
    "Ores.WasmLoaders.ReleaseId",
    "Ores.WasmLoaders.RuntimeKind",
    "Ores.WasmLoaders.SchemaVersion",
    "Ores.WasmLoaders.Sha256Hex",
    "Ores.WasmLoaders.ToolchainId",
];

const EXPECTED_OWLS_INTERFACES_REF: &str = "ff6d5ba6ea8da130854eb511994eb0547ff44988";
const EXPECTED_TJSV_REF: &str = "8bf7ebda4e35c0d11b5b5e5f865867bee4fc3085";

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

    assert_eq!(
        interfaces, EXPECTED_OWLS_INTERFACES_REF,
        "native admission must use the reviewed interface revision",
    );
    assert_ne!(interfaces, validator);
    assert_eq!(
        validator, EXPECTED_TJSV_REF,
        "native admission must use the reviewed current TJSV revision",
    );
    assert!(source.contains("repository: ORESoftware/typespec-json-schema-validator"));
    assert!(source.contains("ref: ${{ env.TSJSV_REF }}"));
    assert!(source.contains("ref: ${{ env.OWLS_INTERFACES_REF }}"));
    assert!(source.contains("typespec-json-schema-validator.mjs check"));
    assert!(source.contains("--typespec=contracts/main.tsp"));
    assert!(source.contains("--schema=schemas/release.schema.json"));
    assert!(source.contains("--instances=.typespec-json-schema-validator/instances"));
    assert!(source.contains("fixtures/valid/*.json"));
    assert!(source.contains("instances/Release/valid"));
    assert!(source.contains("--contract-ir=.typespec-json-schema-validator/contract-ir.json"));
    assert!(source.contains(&format!(
        "ORESoftware/typespec-json-schema-validator/actions/verify-contract-ir@{EXPECTED_TJSV_REF}"
    )));
    assert!(source.contains("tjsv-consumer-verification.json"));
    assert!(source.contains("node scripts/verify-contract-ir.mjs"));
    assert!(source.contains("node scripts/check-language-projections.mjs"));
    assert!(source.contains("include-hidden-files: true"));

    for declaration in EXPECTED_DECLARATIONS {
        assert!(
            source.contains(&format!("\"{declaration}\"")),
            "complete-scope TJSV admission is missing {declaration}"
        );
    }

    assert!(!source.contains("typespec-json-schema-validator\n          ref: main"));
    assert!(!source.contains("typespec-json-schema-validator\n          ref: master"));
}
