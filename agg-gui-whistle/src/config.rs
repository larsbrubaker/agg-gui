//! What an app tells agg-gui-whistle: where the model, the worker and the
//! microphone worklet are (web), and where a model file or the download
//! cache is (native). Every field has a default ([`Config::default`]), so an
//! app overrides only what differs:
//! `Config { worker_url: "./public/whistle/worker.js".into(), ..Config::default() }`.

use std::path::PathBuf;

/// whistle-rust's copy of `whistle.cact` on GitHub Pages (served with CORS
/// `*`), the second place the model is fetched from.
pub const WHISTLE_PAGES_MODEL_URL: &str =
    "https://larsbrubaker.github.io/whistle-rust/whistle.cact";

/// `whistle.cact` on Hugging Face at the revision the whistle crate was
/// tested against (`whistle::model_file::MODEL_URL`), the last fallback.
pub const HUGGING_FACE_MODEL_URL: &str = whistle::model_file::MODEL_URL;

/// How to find the model, the worker and the microphone.
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    /// Where to download `whistle.cact` from, tried in order after the
    /// cache. Relative URLs resolve against the page (web); natively only
    /// `http(s)` URLs are tried (with the `native-download` feature). The
    /// default: `whistle/whistle.cact` beside the page, then
    /// [`WHISTLE_PAGES_MODEL_URL`], then [`HUGGING_FACE_MODEL_URL`].
    pub model_urls: Vec<String>,
    /// Web: the URL of `web/worker.js`, in a directory that also holds the
    /// worker module (`agg_gui_whistle_worker.js` and its `_bg.wasm`).
    /// Default `whistle/worker.js`, relative to the page.
    pub worker_url: String,
    /// Web: the URL of `web/capture-worklet.js`. Default
    /// `whistle/capture-worklet.js`, relative to the page.
    pub worklet_url: String,
    /// The language to recognise when the keyboard names none (`"en"`,
    /// `"de"`, ...); `None` detects it.
    pub language: Option<String>,
    /// Native: a `whistle.cact` to load before trying the cache and the
    /// URLs (checked like a download).
    pub model_path: Option<PathBuf>,
    /// Native: where a downloaded model is kept. `None` is the platform's
    /// cache directory (`~/Library/Caches`, `%LOCALAPPDATA%`,
    /// `$XDG_CACHE_HOME` or `~/.cache`), under `agg-gui-whistle/`.
    pub cache_dir: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            model_urls: vec![
                "whistle/whistle.cact".into(),
                WHISTLE_PAGES_MODEL_URL.into(),
                HUGGING_FACE_MODEL_URL.into(),
            ],
            worker_url: "whistle/worker.js".into(),
            worklet_url: "whistle/capture-worklet.js".into(),
            language: None,
            model_path: None,
            cache_dir: None,
        }
    }
}
