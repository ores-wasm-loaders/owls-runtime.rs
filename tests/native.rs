use owls_runtime::*;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

struct Fetch {
    bytes: Vec<u8>,
    calls: Arc<AtomicUsize>,
}
impl Transport for Fetch {
    fn fetch(&self, _: &Asset, _: &AtomicBool) -> Result<Vec<u8>> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        Ok(self.bytes.clone())
    }
}

struct FetchById {
    bodies: BTreeMap<String, Vec<u8>>,
    order: Arc<Mutex<Vec<String>>>,
}
impl Transport for FetchById {
    fn fetch(&self, asset: &Asset, _: &AtomicBool) -> Result<Vec<u8>> {
        self.order.lock().unwrap().push(asset.id.clone());
        self.bodies.get(&asset.id).cloned().ok_or(Error::Asset)
    }
}

fn manifest(bytes: &[u8]) -> Value {
    json!({"schemaVersion":1,"appId":"demo","release":"r1","runtime":"raw-wasm","entrypoint":"main","assets":[{
      "id":"main","url":"https://assets.example/main.wasm","kind":"wasm","bytes":bytes.len(),
      "sha256":format!("{:x}",Sha256::digest(bytes)),"prepare":true}]})
}

fn dependency_manifest(shared: &[u8], main: &[u8]) -> Value {
    json!({
      "schemaVersion": 2,
      "appId": "demo",
      "release": "r2",
      "runtime": "raw-wasm",
      "entrypoint": "main",
      "assets": [
        {
          "id": "shared",
          "url": "https://assets.example/shared.wasm",
          "kind": "wasm",
          "role": "chunk",
          "stage": "lazy",
          "bytes": shared.len(),
          "sha256": format!("{:x}", Sha256::digest(shared)),
          "prepare": false
        },
        {
          "id": "main",
          "url": "https://assets.example/main-v2.wasm",
          "kind": "wasm",
          "role": "module",
          "stage": "lazy",
          "dependencies": ["shared"],
          "bytes": main.len(),
          "sha256": format!("{:x}", Sha256::digest(main)),
          "prepare": false
        }
      ]
    })
}

fn policy() -> Policy {
    Policy::new(vec!["https://assets.example".into()])
}

#[test]
fn prefetch_then_native_execution_reuses_verified_bytes() {
    let bytes=wat::parse_str("(module (func (export \"add\") (param i32 i32) (result i32) local.get 0 local.get 1 i32.add))").unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let mut host = Host::new(
        manifest(&bytes),
        policy(),
        Fetch {
            bytes,
            calls: calls.clone(),
        },
        MemoryStore::new(65536),
    )
    .unwrap();
    let cancel = AtomicBool::new(false);
    host.prefetch(&cancel).unwrap();
    let mut app = NativeRuntime::new(65536, 1000)
        .unwrap()
        .instantiate(&mut host, &cancel, |_| Ok(()))
        .unwrap();
    let add = app
        .instance
        .get_typed_func::<(i32, i32), i32>(&app.store, "add")
        .unwrap();
    assert_eq!(add.call(&mut app.store, (20, 22)).unwrap(), 42);
    assert_eq!(calls.load(Ordering::Relaxed), 1);
}

#[test]
fn v2_dependency_graph_is_preserved_and_traversed_dependency_first() {
    let shared = b"shared-1".to_vec();
    let main = b"main--v2".to_vec();
    let release = parse_release(dependency_manifest(&shared, &main), &policy()).unwrap();
    assert_eq!(release.schema_version, SchemaVersion::V2);
    assert_eq!(
        release.assets[1].dependencies.as_deref(),
        Some(["shared".to_string()].as_slice())
    );
    assert_eq!(
        dependency_closure(&release, "main")
            .unwrap()
            .into_iter()
            .map(|asset| asset.id.as_str())
            .collect::<Vec<_>>(),
        vec!["shared", "main"],
    );

    let order = Arc::new(Mutex::new(Vec::new()));
    let mut bodies = BTreeMap::new();
    bodies.insert("shared".into(), shared);
    bodies.insert("main".into(), main);
    let mut host = Host::new(
        dependency_manifest(b"shared-1", b"main--v2"),
        policy(),
        FetchById {
            bodies,
            order: order.clone(),
        },
        MemoryStore::new(65536),
    )
    .unwrap();
    let cancel = AtomicBool::new(false);
    assert_eq!(
        host.prefetch_asset("main", &cancel).unwrap(),
        vec!["shared", "main"]
    );
    assert_eq!(*order.lock().unwrap(), vec!["shared", "main"]);
    // The same explicit intent reuses verified cached bytes rather than refetching either edge.
    assert_eq!(
        host.prefetch_asset("main", &cancel).unwrap(),
        vec!["shared", "main"]
    );
    assert_eq!(*order.lock().unwrap(), vec!["shared", "main"]);
}

#[test]
fn invalid_dependency_graphs_fail_closed() {
    let shared = b"shared-1";
    let main = b"main--v2";

    let mut missing = dependency_manifest(shared, main);
    missing["assets"][1]["dependencies"] = json!(["missing"]);
    assert!(
        matches!(parse_release(missing, &policy()), Err(Error::Manifest(message)) if message.contains("missing asset"))
    );

    let mut self_edge = dependency_manifest(shared, main);
    self_edge["assets"][1]["dependencies"] = json!(["main"]);
    assert!(
        matches!(parse_release(self_edge, &policy()), Err(Error::Manifest(message)) if message.contains("cannot depend on itself"))
    );

    let mut duplicate = dependency_manifest(shared, main);
    duplicate["assets"][1]["dependencies"] = json!(["shared", "shared"]);
    assert!(
        matches!(parse_release(duplicate, &policy()), Err(Error::Manifest(message)) if message.contains("repeats dependency"))
    );

    let mut cycle = dependency_manifest(shared, main);
    cycle["assets"][0]["dependencies"] = json!(["main"]);
    assert!(
        matches!(parse_release(cycle, &policy()), Err(Error::Manifest(message)) if message.contains("dependency cycle"))
    );
}

#[test]
fn explicit_dependency_prefetch_obeys_budget_and_cancellation() {
    let shared = b"shared-1".to_vec();
    let main = b"main--v2".to_vec();
    let order = Arc::new(Mutex::new(Vec::new()));
    let mut bodies = BTreeMap::new();
    bodies.insert("shared".into(), shared);
    bodies.insert("main".into(), main);
    let mut small = policy();
    small.max_prepare_bytes = 8;
    let mut host = Host::new(
        dependency_manifest(b"shared-1", b"main--v2"),
        small,
        FetchById {
            bodies,
            order: order.clone(),
        },
        MemoryStore::new(65536),
    )
    .unwrap();
    assert!(matches!(
        host.prefetch_asset("main", &AtomicBool::new(false)),
        Err(Error::Budget)
    ));
    assert!(order.lock().unwrap().is_empty());
    assert!(matches!(
        host.prefetch_asset("main", &AtomicBool::new(true)),
        Err(Error::Budget) | Err(Error::Cancelled)
    ));
    assert!(order.lock().unwrap().is_empty());
}

#[test]
fn infinite_start_is_stopped_by_fuel() {
    let bytes = wat::parse_str("(module (func $run (loop br 0)) (start $run))").unwrap();
    let mut host = Host::new(
        manifest(&bytes),
        policy(),
        Fetch {
            bytes,
            calls: Arc::default(),
        },
        MemoryStore::new(1000),
    )
    .unwrap();
    assert!(
        NativeRuntime::new(65536, 100)
            .unwrap()
            .instantiate(&mut host, &AtomicBool::new(false), |_| Ok(()))
            .is_err()
    );
}

#[test]
fn memory_growth_is_bounded() {
    let bytes = wat::parse_str("(module (memory 2))").unwrap();
    let mut host = Host::new(
        manifest(&bytes),
        policy(),
        Fetch {
            bytes,
            calls: Arc::default(),
        },
        MemoryStore::new(1000),
    )
    .unwrap();
    assert!(
        NativeRuntime::new(65536, 1000)
            .unwrap()
            .instantiate(&mut host, &AtomicBool::new(false), |_| Ok(()))
            .is_err()
    );
}

#[test]
fn invalid_schema_origin_and_duplicates_fail() {
    let bytes = b"12345678";
    let mut r = manifest(bytes);
    r["extra"] = json!(true);
    assert!(parse_release(r, &policy()).is_err());
    let mut r = manifest(bytes);
    r["assets"][0]["url"] = json!("https://evil.example/a");
    assert!(parse_release(r, &policy()).is_err());
    let mut r = manifest(bytes);
    let a = r["assets"][0].clone();
    r["assets"].as_array_mut().unwrap().push(a);
    assert!(parse_release(r, &policy()).is_err());

    for origins in [
        vec![],
        vec!["http://assets.example".into()],
        vec!["https://ASSETS.example".into()],
        vec!["https://assets.example/".into()],
        vec!["https://assets.example/path".into()],
        vec!["https://user@assets.example".into()],
    ] {
        assert!(parse_release(manifest(bytes), &Policy::new(origins)).is_err());
    }
}

#[test]
fn budget_and_cancellation_prevent_network() {
    let bytes = b"12345678".to_vec();
    let calls = Arc::new(AtomicUsize::new(0));
    let mut p = policy();
    p.max_prepare_bytes = 1;
    let mut host = Host::new(
        manifest(&bytes),
        p,
        Fetch {
            bytes,
            calls: calls.clone(),
        },
        MemoryStore::new(1000),
    )
    .unwrap();
    assert!(matches!(
        host.prefetch(&AtomicBool::new(false)),
        Err(Error::Budget)
    ));
    assert!(matches!(
        host.bytes("main", &AtomicBool::new(true)),
        Err(Error::Cancelled)
    ));
    assert_eq!(calls.load(Ordering::Relaxed), 0);
}

#[test]
fn changed_bytes_fail_integrity() {
    let r = manifest(b"12345678");
    let mut host = Host::new(
        r,
        policy(),
        Fetch {
            bytes: b"87654321".to_vec(),
            calls: Arc::default(),
        },
        MemoryStore::new(1000),
    )
    .unwrap();
    assert!(matches!(
        host.prefetch(&AtomicBool::new(false)),
        Err(Error::Integrity)
    ));
}

#[test]
fn file_cache_survives_restart_and_rejects_path_keys() {
    let dir = tempfile::tempdir().unwrap();
    let digest = "a".repeat(64);
    let mut one = FileStore::new(dir.path().into(), 1024).unwrap();
    one.put(&digest, b"data").unwrap();
    let two = FileStore::new(dir.path().into(), 1024).unwrap();
    assert_eq!(two.get(&digest).unwrap().unwrap(), b"data");
    assert!(two.get("../outside").is_err());
}
