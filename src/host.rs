use crate::{
    ByteStore, Error, Policy, Release, Result, Transport, dependency_closure, parse_release, verify,
};
use std::sync::atomic::{AtomicBool, Ordering};

// Accumulation must fail closed even if callers configure u64::MAX ceilings.
// Unchecked sum::<u64>() can panic in debug and wrap in release, letting an
// attacker-controlled manifest bypass a preparation budget.
fn within_prepare_budget<'a>(
    mut assets: impl Iterator<Item = &'a crate::Asset>,
    policy: &Policy,
) -> bool {
    assets
        .try_fold(0_u64, |total, asset| {
            (asset.bytes <= policy.max_asset_bytes)
                .then(|| total.checked_add(asset.bytes))
                .flatten()
        })
        .is_some_and(|total| total <= policy.max_prepare_bytes)
}

/// An immutable release owns one host; mutable access serializes preparation/cache updates.
pub struct Host<T, S> {
    release: Release,
    policy: Policy,
    transport: T,
    store: S,
}
impl<T: Transport, S: ByteStore> Host<T, S> {
    pub fn new(value: serde_json::Value, policy: Policy, transport: T, store: S) -> Result<Self> {
        let release = parse_release(value, &policy)?;
        Ok(Self {
            release,
            policy,
            transport,
            store,
        })
    }
    pub fn release(&self) -> &Release {
        &self.release
    }
    pub fn bytes(&mut self, id: &str, cancelled: &AtomicBool) -> Result<Vec<u8>> {
        if cancelled.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        let asset = self
            .release
            .assets
            .iter()
            .find(|asset| asset.id == id)
            .ok_or(Error::Asset)?;
        if asset.bytes > self.policy.max_asset_bytes {
            return Err(Error::Budget);
        }
        // The store is allowed to perform I/O. Cancellation can arrive while
        // it reads, including when an integrity-valid cache hit would otherwise
        // return immediately or a miss would start a network fetch.
        let cached = self.store.get(&asset.sha256);
        if cancelled.load(Ordering::Acquire) {
            return Err(Error::Cancelled);
        }
        if let Ok(Some(bytes)) = cached
            && verify(asset, &bytes).is_ok()
        {
            return Ok(bytes);
        }
        let bytes = self.transport.fetch(asset, cancelled)?;
        if cancelled.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        verify(asset, &bytes)?;
        let _ = self.store.put(&asset.sha256, &bytes);
        if cancelled.load(Ordering::Acquire) {
            return Err(Error::Cancelled);
        }
        Ok(bytes)
    }

    /// Ambient preparation remains controlled by each asset's `prepare` bit.
    pub fn prefetch(&mut self, cancelled: &AtomicBool) -> Result<()> {
        if cancelled.load(Ordering::Acquire) {
            return Err(Error::Cancelled);
        }
        let assets: Vec<_> = self
            .release
            .assets
            .iter()
            .filter(|asset| asset.prepare)
            .collect();
        if !within_prepare_budget(assets.iter().copied(), &self.policy) {
            return Err(Error::Budget);
        }
        let ids: Vec<_> = assets.iter().map(|asset| asset.id.clone()).collect();
        for id in ids {
            self.bytes(&id, cancelled)?;
        }
        Ok(())
    }

    /// Explicit intent for one asset prepares its full admitted dependency closure.
    /// `prepare:false` dependencies may participate here because the caller named the root;
    /// this does not turn them into ambient speculative work.
    pub fn prefetch_asset(&mut self, id: &str, cancelled: &AtomicBool) -> Result<Vec<String>> {
        if cancelled.load(Ordering::Acquire) {
            return Err(Error::Cancelled);
        }
        let closure = dependency_closure(&self.release, id)?;
        if !within_prepare_budget(closure.iter().copied(), &self.policy) {
            return Err(Error::Budget);
        }
        let ids: Vec<_> = closure.iter().map(|asset| asset.id.clone()).collect();
        for asset_id in &ids {
            self.bytes(asset_id, cancelled)?;
        }
        Ok(ids)
    }
}

#[cfg(test)]
mod bounded_prepare_tests {
    use super::within_prepare_budget;
    use crate::{Asset, AssetKind, Policy};

    fn asset(id: &str, bytes: u64) -> Asset {
        Asset {
            id: id.into(),
            url: format!("https://assets.example/{id}"),
            kind: AssetKind::Data,
            role: None,
            stage: None,
            dependencies: None,
            bytes,
            sha256: "0".repeat(64),
            prepare: true,
        }
    }

    #[test]
    fn rejects_overflow_even_when_per_asset_budget_is_unbounded() {
        let policy = Policy {
            origins: vec!["https://assets.example".into()],
            max_asset_bytes: u64::MAX,
            max_prepare_bytes: u64::MAX,
        };
        let huge = asset("huge", u64::MAX);
        let one = asset("one", 1);
        assert!(!within_prepare_budget([&huge, &one].into_iter(), &policy));
        assert!(!within_prepare_budget([&one, &huge].into_iter(), &policy));
        assert!(within_prepare_budget([&huge].into_iter(), &policy));
    }

    #[test]
    fn respects_independent_total_and_asset_budgets() {
        let first = asset("first", 8);
        let second = asset("second", 5);
        let base = Policy {
            origins: vec!["https://assets.example".into()],
            max_asset_bytes: 10,
            max_prepare_bytes: 12,
        };
        assert!(!within_prepare_budget([&first, &second].into_iter(), &base));
        assert!(within_prepare_budget([&first].into_iter(), &base));
        let per_asset = Policy {
            max_asset_bytes: 7,
            ..base
        };
        assert!(!within_prepare_budget([&first].into_iter(), &per_asset));
        assert!(within_prepare_budget([&second].into_iter(), &per_asset));
    }
}

#[cfg(test)]
mod cache_cancellation_tests {
    use super::Host;
    use crate::{
        Asset, AssetKind, ByteStore, Error, Policy, Release, Result, Runtime, SchemaVersion,
        Transport,
    };
    use sha2::{Digest, Sha256};
    use std::collections::BTreeMap;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    struct CancellingCache {
        cancelled: Arc<AtomicBool>,
        cached: Option<Vec<u8>>,
    }
    impl ByteStore for CancellingCache {
        fn get(&self, _key: &str) -> Result<Option<Vec<u8>>> {
            self.cancelled.store(true, Ordering::SeqCst);
            Ok(self.cached.clone())
        }
        fn put(&mut self, _key: &str, _bytes: &[u8]) -> Result<()> {
            panic!("a cancelled fetch must not populate the cache");
        }
    }

    struct NoNetwork;
    impl Transport for NoNetwork {
        fn fetch(&self, _asset: &Asset, _cancelled: &AtomicBool) -> Result<Vec<u8>> {
            panic!("a cancelled cache read must never start network I/O");
        }
    }

    fn host(
        cancelled: Arc<AtomicBool>,
        cached: Option<Vec<u8>>,
    ) -> Host<NoNetwork, CancellingCache> {
        let bytes = b"safe".to_vec();
        let digest = format!("{:x}", Sha256::digest(&bytes));
        Host {
            release: Release {
                schema_version: SchemaVersion::V2,
                app_id: "synthetic-app".into(),
                release: "synthetic-release".into(),
                runtime: Runtime::RawWasm,
                framework: None,
                toolchain: None,
                entrypoint: "cached".into(),
                requires_cross_origin_isolation: None,
                assets: vec![Asset {
                    id: "cached".into(),
                    url: "https://assets.example/cached".into(),
                    kind: AssetKind::Wasm,
                    role: None,
                    stage: None,
                    dependencies: None,
                    bytes: 4,
                    sha256: digest,
                    prepare: false,
                }],
                prepare_budget: None,
                activation: None,
                extensions: BTreeMap::new(),
            },
            policy: Policy::new(vec!["https://assets.example".into()]),
            transport: NoNetwork,
            store: CancellingCache { cancelled, cached },
        }
    }

    #[test]
    fn cancellation_during_verified_cache_hit_rejects_bytes() {
        let cancelled = Arc::new(AtomicBool::new(false));
        let mut host = host(Arc::clone(&cancelled), Some(b"safe".to_vec()));
        assert!(matches!(
            host.bytes("cached", &cancelled),
            Err(Error::Cancelled)
        ));
        assert!(cancelled.load(Ordering::SeqCst));
    }

    #[test]
    fn cancellation_during_cache_miss_prevents_network_fetch() {
        let cancelled = Arc::new(AtomicBool::new(false));
        let mut host = host(Arc::clone(&cancelled), None);
        assert!(matches!(
            host.bytes("cached", &cancelled),
            Err(Error::Cancelled)
        ));
    }

    #[test]
    fn already_cancelled_prefetch_is_denied_even_without_prepared_assets() {
        let cancelled = Arc::new(AtomicBool::new(true));
        let mut host = host(Arc::clone(&cancelled), None);
        assert!(matches!(host.prefetch(&cancelled), Err(Error::Cancelled)));
        assert!(matches!(
            host.prefetch_asset("cached", &cancelled),
            Err(Error::Cancelled)
        ));
    }
}
