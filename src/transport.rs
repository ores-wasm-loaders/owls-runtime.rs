use crate::{Asset, Error, Result};
use std::{
    io::Read,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
pub trait Transport {
    /// Return at most asset.bytes decoded bytes. Never execute them.
    fn fetch(&self, asset: &Asset, cancelled: &AtomicBool) -> Result<Vec<u8>>;
}
pub struct HttpTransport {
    client: reqwest::blocking::Client,
}
impl HttpTransport {
    pub fn new(timeout: Duration) -> Result<Self> {
        if timeout.is_zero() {
            return Err(Error::Budget);
        }
        Ok(Self {
            client: reqwest::blocking::Client::builder()
                .https_only(true)
                .redirect(reqwest::redirect::Policy::none())
                .timeout(timeout)
                .build()?,
        })
    }
}
impl Transport for HttpTransport {
    fn fetch(&self, asset: &Asset, cancelled: &AtomicBool) -> Result<Vec<u8>> {
        if cancelled.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        let mut response = self.client.get(&asset.url).send()?.error_for_status()?;
        if response.status().is_redirection() {
            return Err(Error::Manifest("redirect denied".into()));
        }
        let mut bytes = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            if cancelled.load(Ordering::Relaxed) {
                return Err(Error::Cancelled);
            }
            let n = response.read(&mut chunk)?;
            if n == 0 {
                break;
            }
            if (bytes.len() + n) as u64 > asset.bytes {
                return Err(Error::Budget);
            }
            bytes.extend_from_slice(&chunk[..n]);
        }
        Ok(bytes)
    }
}
