//! Optional settings persistence: a tiny string-blob store trait, its
//! `localStorage` backend (wasm), an in-memory backend (tests / native), and
//! [`SettingsAutoSave`], the diff-guarded, pointer-idle-gated writer.
//!
//! Extracted from AtomArtist's `demo-wasm/src/web_settings.rs` +
//! `web_lifecycle.rs`. The shell does not decide *what* an app persists — the
//! app composes its own blob (text, JSON, …). What lives here is *where* and
//! *when*: one `localStorage` key, written only when the blob changed, never
//! mid-drag, and forced on page hide ([`crate::WebShellHost::on_page_hide`]).
//!
//! Every operation degrades rather than fails: storage disabled (private mode,
//! blocked cookies), quota exceeded, or a missing key all behave as "nothing
//! stored" / a logged no-op. Settings must never block startup.

use std::cell::RefCell;

use agg_gui::persistence::AutoSave;

/// Why a settings write did not land.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SettingsError {
    /// The origin has no usable storage (private mode, blocked, not a browser).
    Unavailable,
    /// The backend rejected the write (quota exceeded, storage disabled
    /// mid-session).
    WriteFailed(String),
}

impl std::fmt::Display for SettingsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable => write!(f, "settings storage unavailable"),
            Self::WriteFailed(e) => write!(f, "settings write failed: {e}"),
        }
    }
}

impl std::error::Error for SettingsError {}

/// A place a single settings blob lives.
pub trait SettingsStore {
    /// The stored blob, or `None` when absent or unreachable.
    fn load(&self) -> Option<String>;
    /// Replace the stored blob.
    fn save(&self, blob: &str) -> Result<(), SettingsError>;
}

/// In-memory store — for tests and for native builds of code that is written
/// against [`SettingsStore`].
#[derive(Debug, Default)]
pub struct MemorySettings {
    blob: RefCell<Option<String>>,
    writes: std::cell::Cell<u32>,
}

impl MemorySettings {
    pub fn new(initial: Option<String>) -> Self {
        Self {
            blob: RefCell::new(initial),
            writes: std::cell::Cell::new(0),
        }
    }

    /// How many times [`SettingsStore::save`] ran.
    pub fn write_count(&self) -> u32 {
        self.writes.get()
    }
}

impl SettingsStore for MemorySettings {
    fn load(&self) -> Option<String> {
        self.blob.borrow().clone()
    }

    fn save(&self, blob: &str) -> Result<(), SettingsError> {
        *self.blob.borrow_mut() = Some(blob.to_string());
        self.writes.set(self.writes.get() + 1);
        Ok(())
    }
}

/// `window.localStorage` under one key. On a non-browser target every call
/// reports [`SettingsError::Unavailable`] / `None`.
#[derive(Debug, Clone)]
pub struct LocalStorageSettings {
    key: String,
}

impl LocalStorageSettings {
    /// A store for `key`. Namespace it (`"myapp.settings"`) so sibling keys
    /// can be added later without collision.
    pub fn new(key: impl Into<String>) -> Self {
        Self { key: key.into() }
    }

    pub fn key(&self) -> &str {
        &self.key
    }

    #[cfg(target_arch = "wasm32")]
    fn storage() -> Option<web_sys::Storage> {
        web_sys::window().and_then(|w| w.local_storage().ok().flatten())
    }
}

impl SettingsStore for LocalStorageSettings {
    #[cfg(target_arch = "wasm32")]
    fn load(&self) -> Option<String> {
        Self::storage().and_then(|s| s.get_item(&self.key).ok().flatten())
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn load(&self) -> Option<String> {
        None
    }

    #[cfg(target_arch = "wasm32")]
    fn save(&self, blob: &str) -> Result<(), SettingsError> {
        let storage = Self::storage().ok_or(SettingsError::Unavailable)?;
        storage
            .set_item(&self.key, blob)
            .map_err(|e| SettingsError::WriteFailed(format!("{e:?}")))
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn save(&self, _blob: &str) -> Result<(), SettingsError> {
        Err(SettingsError::Unavailable)
    }
}

/// Diff-guarded, idle-gated writer over a [`SettingsStore`].
///
/// Call [`Self::tick`] from [`crate::WebShellHost::on_idle`] with
/// `control.pointer_idle()`, and [`Self::flush`] from
/// [`crate::WebShellHost::on_page_hide`]. `compose` runs only when a write is
/// allowed, so idle ticks cost nothing; the store is written only when the
/// blob differs from the last one written (or loaded).
pub struct SettingsAutoSave<S: SettingsStore> {
    store: S,
    auto: AutoSave,
}

impl<S: SettingsStore> SettingsAutoSave<S> {
    /// Wrap `store`, seeding the diff with whatever it already holds so the
    /// first tick does not rewrite an identical value. Returns the loaded blob
    /// too, for the app to parse its settings from.
    pub fn new(store: S) -> (Self, Option<String>) {
        let mut auto = AutoSave::new();
        let loaded = store.load();
        if let Some(blob) = &loaded {
            auto.seed(blob.clone());
        }
        (Self { store, auto }, loaded)
    }

    pub fn store(&self) -> &S {
        &self.store
    }

    /// Persist when `pointer_idle` and the blob changed. Returns whether a
    /// write was attempted. A failed write is logged and swallowed.
    pub fn tick(&mut self, pointer_idle: bool, compose: impl FnOnce() -> String) -> bool {
        let store = &self.store;
        self.auto.tick(pointer_idle, compose, |blob| {
            if let Err(e) = store.save(blob) {
                log::warn!("agg-gui-web-shell: {e}");
            }
        })
    }

    /// Persist regardless of held buttons — the page-hide path, where a drag
    /// in progress is no reason to lose the user's settings.
    pub fn flush(&mut self, compose: impl FnOnce() -> String) -> bool {
        self.tick(true, compose)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loaded_blob_seeds_the_diff() {
        let (mut saver, loaded) = SettingsAutoSave::new(MemorySettings::new(Some("a".into())));
        assert_eq!(loaded.as_deref(), Some("a"));
        assert!(
            !saver.tick(true, || "a".into()),
            "identical blob not rewritten"
        );
        assert_eq!(saver.store().write_count(), 0);
    }

    #[test]
    fn changed_blob_is_written_once() {
        let (mut saver, _) = SettingsAutoSave::new(MemorySettings::new(None));
        assert!(saver.tick(true, || "b".into()));
        assert!(!saver.tick(true, || "b".into()));
        assert_eq!(saver.store().load().as_deref(), Some("b"));
        assert_eq!(saver.store().write_count(), 1);
    }

    #[test]
    fn held_pointer_defers_the_write_and_skips_compose() {
        let (mut saver, _) = SettingsAutoSave::new(MemorySettings::new(None));
        let mut composed = false;
        assert!(!saver.tick(false, || {
            composed = true;
            "c".into()
        }));
        assert!(!composed);
        assert_eq!(saver.store().write_count(), 0);
    }

    #[test]
    fn flush_ignores_the_pointer_guard() {
        let (mut saver, _) = SettingsAutoSave::new(MemorySettings::new(None));
        assert!(saver.flush(|| "d".into()));
        assert_eq!(saver.store().load().as_deref(), Some("d"));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn local_storage_is_unavailable_off_the_web() {
        let s = LocalStorageSettings::new("k");
        assert_eq!(s.load(), None);
        assert_eq!(s.save("x"), Err(SettingsError::Unavailable));
    }
}
