use crate::{
    ByteStore, Error, Policy, Release, Result, Transport, dependency_closure, parse_release, verify,
};
use std::sync::atomic::{AtomicBool, Ordering};

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
        if assets.iter().map(|asset| asset.bytes).sum::<u64>() > self.policy.max_prepare_bytes
            || assets
                .iter()
                .any(|asset| asset.bytes > self.policy.max_asset_bytes)
        {
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
        if closure.iter().map(|asset| asset.bytes).sum::<u64>() > self.policy.max_prepare_bytes
            || closure
                .iter()
                .any(|asset| asset.bytes > self.policy.max_asset_bytes)
        {
            return Err(Error::Budget);
        }
        let ids: Vec<_> = closure.iter().map(|asset| asset.id.clone()).collect();
        for asset_id in &ids {
            self.bytes(asset_id, cancelled)?;
        }
        Ok(ids)
    }
}
