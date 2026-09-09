use crate::{Asset, AssetKind, Error, Result};
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

/// Reject a response that could turn a missing executable asset into HTML.
/// The release manifest remains the source of the exact asset kind; MIME is an
/// additional transport invariant, not a replacement for SHA-256 verification.
pub(crate) fn response_content_type_allowed(asset: &Asset, value: Option<&str>) -> bool {
    let Some(value) = value else {
        return false;
    };
    let mime = value
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    match &asset.kind {
        AssetKind::Wasm => mime == "application/wasm",
        AssetKind::Module | AssetKind::Script => matches!(
            mime.as_str(),
            "application/javascript"
                | "application/ecmascript"
                | "text/javascript"
                | "text/ecmascript"
                | "application/x-javascript"
        ),
        AssetKind::Font => matches!(
            mime.as_str(),
            "font/otf"
                | "font/ttf"
                | "font/woff"
                | "font/woff2"
                | "application/font-woff"
                | "application/font-woff2"
                | "application/vnd.ms-fontobject"
        ),
        AssetKind::Data => {
            !mime.is_empty() && mime != "text/html" && mime != "application/xhtml+xml"
        }
    }
}

impl Transport for HttpTransport {
    fn fetch(&self, asset: &Asset, cancelled: &AtomicBool) -> Result<Vec<u8>> {
        if cancelled.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        let mut response = self.client.get(&asset.url).send()?;
        if response.status().is_redirection() {
            return Err(Error::Manifest("redirect denied".into()));
        }
        response = response.error_for_status()?;
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok());
        if !response_content_type_allowed(asset, content_type) {
            return Err(Error::ContentType);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(kind: AssetKind) -> Asset {
        Asset {
            id: "asset".into(),
            url: "https://assets.example/asset".into(),
            kind,
            role: None,
            stage: None,
            dependencies: None,
            bytes: 1,
            sha256: "0".repeat(64),
            prepare: true,
        }
    }

    #[test]
    fn content_type_must_match_the_manifest_kind() {
        assert!(response_content_type_allowed(
            &asset(AssetKind::Wasm),
            Some("application/wasm; charset=binary")
        ));
        assert!(!response_content_type_allowed(
            &asset(AssetKind::Wasm),
            Some("text/html")
        ));
        assert!(!response_content_type_allowed(
            &asset(AssetKind::Wasm),
            None
        ));
        assert!(response_content_type_allowed(
            &asset(AssetKind::Module),
            Some("text/javascript")
        ));
        assert!(response_content_type_allowed(
            &asset(AssetKind::Script),
            Some("application/javascript; charset=utf-8")
        ));
        assert!(!response_content_type_allowed(
            &asset(AssetKind::Module),
            Some("application/wasm")
        ));
        assert!(response_content_type_allowed(
            &asset(AssetKind::Font),
            Some("font/woff2")
        ));
        assert!(response_content_type_allowed(
            &asset(AssetKind::Data),
            Some("application/json")
        ));
        assert!(!response_content_type_allowed(
            &asset(AssetKind::Data),
            Some("application/xhtml+xml")
        ));
    }
}
