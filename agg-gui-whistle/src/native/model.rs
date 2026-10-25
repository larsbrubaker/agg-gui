//! Where the native engine thread (`mod.rs`) gets `whistle.cact`: the path
//! the app configured, then the download cache, then (feature
//! `native-download`) each `http(s)` URL in turn, saved into the cache.
//! Every copy is checked against the pinned size and SHA-256
//! (`model_check.rs`) before use; a bad cached copy is deleted.
//!
//! The cache holds `<cache dir>/<sha256>/whistle.cact`, so another model
//! version never reads this one. The cache dir is the app's
//! (`Config::cache_dir`) or the platform's, under `agg-gui-whistle/`.

use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::model_check::{self, Check};
use crate::protocol::LoadFailure;

/// The places to try, from the app's config.
#[derive(Debug, Clone)]
pub(super) struct Source {
    path: Option<PathBuf>,
    cache_dir: Option<PathBuf>,
    urls: Vec<String>,
}

/// The platform's cache directory for this crate.
fn platform_cache_dir() -> Option<PathBuf> {
    let env = |k: &str| {
        std::env::var_os(k)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    let base = if cfg!(target_os = "macos") {
        env("HOME").map(|h| h.join("Library/Caches"))
    } else if cfg!(windows) {
        env("LOCALAPPDATA")
    } else {
        env("XDG_CACHE_HOME").or_else(|| env("HOME").map(|h| h.join(".cache")))
    };
    base.map(|b| b.join("agg-gui-whistle"))
}

/// Why a step found no model, for the final report.
struct Failures {
    details: Vec<String>,
    mismatched: bool,
}

impl Failures {
    fn mismatch(&mut self, what: &str) {
        self.mismatched = true;
        self.details
            .push(format!("{what}: not the expected whistle.cact"));
    }
}

impl Source {
    pub(super) fn from_config(config: &Config) -> Source {
        Source {
            path: config.model_path.clone(),
            cache_dir: config.cache_dir.clone().or_else(platform_cache_dir),
            urls: config
                .model_urls
                .iter()
                .filter(|u| u.starts_with("https://") || u.starts_with("http://"))
                .cloned()
                .collect(),
        }
    }

    fn cache_file(&self) -> Option<PathBuf> {
        self.cache_dir
            .as_ref()
            .map(|d| d.join(model_check::expected_sha256()).join("whistle.cact"))
    }

    /// The checked model and where it came from, or why there is none.
    pub(super) fn load(
        &self,
        progress: &dyn Fn(u64, u64),
    ) -> Result<(Vec<u8>, String), (LoadFailure, String)> {
        let mut failures = Failures {
            details: Vec::new(),
            mismatched: false,
        };
        if let Some(path) = &self.path {
            match std::fs::read(path) {
                Ok(bytes) if model_check::check_bytes(&bytes) == Check::Ok => {
                    return Ok((bytes, path.display().to_string()));
                }
                Ok(_) => failures.mismatch(&path.display().to_string()),
                Err(err) => failures.details.push(format!("{}: {err}", path.display())),
            }
        }
        let cached = self.cache_file();
        if let Some(file) = &cached {
            if let Ok(bytes) = std::fs::read(file) {
                if model_check::check_bytes(&bytes) == Check::Ok {
                    progress(bytes.len() as u64, bytes.len() as u64);
                    return Ok((bytes, "cache".into()));
                }
                let _ = std::fs::remove_file(file);
            }
        }
        for url in &self.urls {
            match download::get(url, progress) {
                Ok(bytes) if model_check::check_bytes(&bytes) == Check::Ok => {
                    if let Some(file) = &cached {
                        save(file, &bytes);
                    }
                    return Ok((bytes, url.clone()));
                }
                Ok(_) => failures.mismatch(url),
                Err(err) => failures.details.push(err),
            }
        }
        if failures.details.is_empty() {
            failures
                .details
                .push("no model path, cached copy or download URL".into());
        }
        let reason = if failures.mismatched {
            LoadFailure::Checksum
        } else {
            LoadFailure::Download
        };
        Err((reason, failures.details.join("; ")))
    }
}

/// Write `bytes` to `file` through a temporary name, so a crash never
/// leaves half a model where the cache is read. Failure only costs the
/// next start a download.
fn save(file: &Path, bytes: &[u8]) {
    let Some(dir) = file.parent() else {
        return;
    };
    let partial = file.with_extension("part");
    let saved = std::fs::create_dir_all(dir)
        .and_then(|_| std::fs::write(&partial, bytes))
        .and_then(|_| std::fs::rename(&partial, file));
    if let Err(err) = saved {
        eprintln!("agg-gui-whistle: could not cache the model: {err}");
        let _ = std::fs::remove_file(&partial);
    }
}

#[cfg(feature = "native-download")]
mod download {
    use std::io::Read;
    use std::sync::{Arc, OnceLock};

    use crate::model_check;

    /// One agent, over rustls with the RustCrypto provider and the Mozilla
    /// roots compiled in: the stack agg-gui's `http_fetch` uses.
    fn agent() -> &'static ureq::Agent {
        static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
        AGENT.get_or_init(|| {
            let tls = ureq::tls::TlsConfig::builder()
                .provider(ureq::tls::TlsProvider::Rustls)
                .unversioned_rustls_crypto_provider(Arc::new(rustls_rustcrypto::provider()))
                .root_certs(ureq::tls::RootCerts::WebPki)
                .build();
            ureq::Agent::config_builder().tls_config(tls).build().into()
        })
    }

    /// GET `url`, reporting progress; refuses more bytes than the model has.
    pub(super) fn get(url: &str, progress: &dyn Fn(u64, u64)) -> Result<Vec<u8>, String> {
        let mut response = agent().get(url).call().map_err(|e| format!("{url}: {e}"))?;
        let expected = model_check::expected_bytes();
        let total = response.body().content_length().unwrap_or(expected as u64);
        let mut reader = response
            .body_mut()
            .with_config()
            .limit(expected as u64 + 1)
            .reader();
        let mut bytes = Vec::with_capacity(expected);
        let mut buf = vec![0u8; 64 * 1024];
        let mut reported = 0;
        loop {
            let n = reader.read(&mut buf).map_err(|e| format!("{url}: {e}"))?;
            if n == 0 {
                break;
            }
            bytes.extend_from_slice(&buf[..n]);
            if bytes.len() - reported >= 256 * 1024 {
                reported = bytes.len();
                progress(bytes.len() as u64, total);
            }
        }
        progress(bytes.len() as u64, total);
        Ok(bytes)
    }
}

#[cfg(not(feature = "native-download"))]
mod download {
    /// Built without `native-download`: no URL is fetched.
    pub(super) fn get(url: &str, _progress: &dyn Fn(u64, u64)) -> Result<Vec<u8>, String> {
        Err(format!("{url}: built without the native-download feature"))
    }
}

#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;
