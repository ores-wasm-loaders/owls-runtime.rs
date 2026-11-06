use crate::{
    ByteStore, Error, Policy, Release, Result, Transport, dependency_closure, parse_release, verify,
};
use std::sync::atomic::{AtomicBool, Ordering};

// Accumulation must fail closed even if callers configure u64::MAX ceilings.
// Unchecked sum::<u64>() can panic in debug and wrap in release, letting an
// attacker-controlled manifest bypass a preparation budget.
fn within_prepare_budget<'a>(
    assets: impl Iterator<Item = &'a crate::Asset>,
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
        if let Ok(Some(bytes)) = self.store.get(&asset.sha256)
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
        Ok(bytes)
    }

    /// Ambient preparation remains controlled by each asset's `prepare` bit.
    pub fn prefetch(&mut self, cancelled: &AtomicBool) -> Result<()> {
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
        let per_asset = Policy { max_asset_bytes: 7, ..base };
        assert!(!within_prepare_budget([&first].into_iter(), &per_asset));
        assert!(within_prepare_budget([&second].into_iter(), &per_asset));
    }
}
