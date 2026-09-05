use crate::{ByteStore, Error, Policy, Release, Result, Transport, parse_release, verify};
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
        let a = self
            .release
            .assets
            .iter()
            .find(|a| a.id == id)
            .ok_or(Error::Asset)?;
        if a.bytes > self.policy.max_asset_bytes {
            return Err(Error::Budget);
        }
        if let Ok(Some(bytes)) = self.store.get(&a.sha256)
            && verify(a, &bytes).is_ok()
        {
            return Ok(bytes);
        }
        let bytes = self.transport.fetch(a, cancelled)?;
        if cancelled.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        verify(a, &bytes)?;
        let _ = self.store.put(&a.sha256, &bytes);
        Ok(bytes)
    }
    pub fn prefetch(&mut self, cancelled: &AtomicBool) -> Result<()> {
        let assets: Vec<_> = self.release.assets.iter().filter(|a| a.prepare).collect();
        if assets.iter().map(|a| a.bytes).sum::<u64>() > self.policy.max_prepare_bytes
            || assets.iter().any(|a| a.bytes > self.policy.max_asset_bytes)
        {
            return Err(Error::Budget);
        }
        let ids: Vec<_> = assets.iter().map(|a| a.id.clone()).collect();
        for id in ids {
            self.bytes(&id, cancelled)?;
        }
        Ok(())
    }
}
