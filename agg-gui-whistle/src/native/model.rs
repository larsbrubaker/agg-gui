//! Where the native engine thread (`mod.rs`) gets `whistle.cact`: the path
//! the app configured, then the download cache, then (feature
//! `native-download`) each `http(s)` URL in turn, saved into the cache.
//! Every copy is checked against the pinned size and SHA-256
//! (`model_check.rs`) before use; a bad cached copy is deleted.
//!
//! A download that stalls (no bytes for [`STALL`], or no answer) times out
//! and counts as a failed download, which the next tap retries.
//! When every place fails, the last failure names the reason: a network
//! failure after a wrong file is reported as a network failure.
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

/// Why the steps found no model, for the final report.
struct Failures {
    details: Vec<String>,
    /// The kind of the last failure, the one reported.
    last: LoadFailure,
}

impl Failures {
    fn mismatch(&mut self, what: &str) {
        self.last = LoadFailure::Checksum;
        self.details
            .push(format!("{what}: not the expected whistle.cact"));
    }

    fn unreachable(&mut self, detail: String) {
        self.last = LoadFailure::Download;
        self.details.push(detail);
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
            last: LoadFailure::Download,
        };
        if let Some(path) = &self.path {
            match std::fs::read(path) {
                Ok(bytes) if model_check::check_bytes(&bytes) == Check::Ok => {
                    return Ok((bytes, path.display().to_string()));
                }
                Ok(_) => failures.mismatch(&path.display().to_string()),
                Err(err) => failures.unreachable(format!("{}: {err}", path.display())),
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
                Err(err) => failures.unreachable(err),
            }
        }
        if failures.details.is_empty() {
            failures
                .details
                .push("no model path, cached copy or download URL".into());
        }
        Err((failures.last, failures.details.join("; ")))
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
    use std::time::Duration;

    use super::STALL;
    use crate::model_check;

    /// An agent over rustls with the RustCrypto provider and the Mozilla
    /// roots compiled in (the stack agg-gui's `http_fetch` uses) that gives
    /// up on a server silent for `stall`: ureq 3 has no timeouts of its
    /// own. The body has no total limit: a slow link that keeps delivering
    /// may take as long as it needs.
    pub(super) fn agent_with(stall: Duration) -> ureq::Agent {
        let tls = ureq::tls::TlsConfig::builder()
            .provider(ureq::tls::TlsProvider::Rustls)
            .unversioned_rustls_crypto_provider(Arc::new(rustls_rustcrypto::provider()))
            .root_certs(ureq::tls::RootCerts::WebPki)
            .build();
        ureq::Agent::config_builder()
            .tls_config(tls)
            .timeout_connect(Some(stall))
            .timeout_recv_response(Some(stall))
            .timeout_per_read(Some(stall))
            .build()
            .into()
    }

    fn agent() -> &'static ureq::Agent {
        static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
        AGENT.get_or_init(|| agent_with(STALL))
    }

    /// GET `url`, reporting progress; refuses more bytes than the model has
    /// (some other file, a download failure saying so, as on the web).
    pub(super) fn get(url: &str, progress: &dyn Fn(u64, u64)) -> Result<Vec<u8>, String> {
        get_with(agent(), url, progress)
    }

    /// [`get`] on `agent`. Progress counts against the model's pinned size,
    /// not `Content-Length` (which counts encoded bytes when the body is
    /// compressed).
    pub(super) fn get_with(
        agent: &ureq::Agent,
        url: &str,
        progress: &dyn Fn(u64, u64),
    ) -> Result<Vec<u8>, String> {
        let mut response = agent.get(url).call().map_err(|e| format!("{url}: {e}"))?;
        let expected = model_check::expected_bytes();
        let total = expected as u64;
        // The loop enforces the model's size, with its own message.
        let mut reader = response.body_mut().with_config().limit(u64::MAX).reader();
        let mut bytes = Vec::with_capacity(expected);
        let mut buf = vec![0u8; 64 * 1024];
        let mut reported = 0;
        loop {
            let n = reader.read(&mut buf).map_err(|e| format!("{url}: {e}"))?;
            if n == 0 {
                break;
            }
            if bytes.len() + n > expected {
                return Err(format!("{url} is larger than whistle.cact"));
            }
            bytes.extend_from_slice(&buf[..n]);
            if bytes.len() - reported >= 256 * 1024 {
                reported = bytes.len();
                progress((bytes.len() as u64).min(total), total);
            }
        }
        progress((bytes.len() as u64).min(total), total);
        Ok(bytes)
    }
}

/// A download silent this long (no connection, no answer, no bytes) has
/// stalled.
#[cfg(feature = "native-download")]
const STALL: std::time::Duration = std::time::Duration::from_secs(20);

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
