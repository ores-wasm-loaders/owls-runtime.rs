use owls_runtime::*;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
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
fn manifest(bytes: &[u8]) -> Value {
    json!({"schemaVersion":1,"appId":"demo","release":"r1","runtime":"raw-wasm","entrypoint":"main","assets":[{
      "id":"main","url":"https://assets.example/main.wasm","kind":"wasm","bytes":bytes.len(),
      "sha256":format!("{:x}",Sha256::digest(bytes)),"prepare":true}]})
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
    let mut noncanonical = policy();
    noncanonical.origins = vec!["https://ASSETS.example".into()];
    assert!(parse_release(manifest(bytes), &noncanonical).is_err());
    noncanonical.origins = vec!["https://assets.example/".into()];
    assert!(parse_release(manifest(bytes), &noncanonical).is_err());
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
