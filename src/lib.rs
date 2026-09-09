#![forbid(unsafe_code)]
mod build_manifest;
mod host;
mod manifest;
mod runtime;
mod storage;
mod transport;
pub use build_manifest::inspect_build;
pub use host::Host;
pub use manifest::{Policy, dependency_closure, parse_release};
pub use owls_interfaces::v2::{Asset, AssetKind, Release, RuntimeKind as Runtime, SchemaVersion};
pub use runtime::{NativeRuntime, RunningModule};
pub use storage::{ByteStore, FileStore, MemoryStore};
pub use transport::{HttpTransport, Transport};
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid release: {0}")]
    Manifest(String),
    #[error("asset violates byte budget")]
    Budget,
    #[error("asset integrity or size mismatch")]
    Integrity,
    #[error("unknown asset")]
    Asset,
    #[error("unsupported runtime; native hosts require raw-wasm")]
    Runtime,
    #[error("operation cancelled")]
    Cancelled,
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("request failed")]
    Http(#[from] reqwest::Error),
    #[error("unexpected or missing asset content type")]
    ContentType,
    #[error("WASM execution: {0}")]
    Wasm(String),
}
pub type Result<T> = std::result::Result<T, Error>;
pub(crate) fn verify(asset: &Asset, bytes: &[u8]) -> Result<()> {
    use sha2::{Digest, Sha256};
    if bytes.len() as u64 != asset.bytes || format!("{:x}", Sha256::digest(bytes)) != asset.sha256 {
        return Err(Error::Integrity);
    }
    Ok(())
}
