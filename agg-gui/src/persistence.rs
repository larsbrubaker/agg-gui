//! Small persistence helpers shared across platform harnesses.
//!
//! Both the native (disk) and wasm (localStorage) harnesses follow the
//! same auto-save policy: each frame, serialize the current session
//! state, compare to the last-persisted blob, and write only when they
//! differ AND no mouse button is currently held.  The "no mouse held"
//! guard keeps us from hammering the backend while the user is
//! mid-drag / mid-resize.
//!
//! This module owns the diff-and-save policy so hosts don't reimplement
//! it.  Callers plug in their own serializer and persist backend via
//! closures — [`AutoSave::tick`] takes care of the rest.

/// Tracks the last-persisted serialized blob so the platform harness
/// can avoid writing unchanged state every frame.
///
/// # Example (pseudo-Rust)
/// ```ignore
/// let mut auto_save = AutoSave::new();
/// loop {
///     paint_frame();
///     auto_save.tick(
///         mouse_buttons_held == 0,
///         || state.serialize(),
///         |blob| std::fs::write("state.json", blob).ok(),
///     );
/// }
/// ```
#[derive(Default)]
pub struct AutoSave {
    last_saved: String,
}

impl AutoSave {
    /// Create an empty auto-save tracker.  First `tick` call will always
    /// persist (because the last-saved blob is empty).
    pub const fn new() -> Self {
        Self {
            last_saved: String::new(),
        }
    }

    /// Consider persisting the current state.
    ///
    /// `idle` — guard that must be `true` for the save to happen.
    /// Typically `mouse_buttons_held == 0`; callers can layer other
    /// conditions (e.g. "no in-flight drag").
    ///
    /// `serialize_now` — closure that produces the current serialized
    /// blob.  Only invoked when `idle` is `true`.
    ///
    /// `persist` — closure that writes the blob to the platform backend
    /// (file / localStorage / network).  Only invoked when the blob
    /// differs from the last persisted version.
    ///
    /// Returns `true` iff the persist closure was called.
    pub fn tick<S, P>(&mut self, idle: bool, serialize_now: S, persist: P) -> bool
    where
        S: FnOnce() -> String,
        P: FnOnce(&str),
    {
        if !idle {
            return false;
        }
        let blob = serialize_now();
        if blob == self.last_saved {
            return false;
        }
        persist(&blob);
        self.last_saved = blob;
        true
    }

    /// Seed the tracker with a blob that's already on disk (on startup).
    /// Prevents the first `tick` from writing the same content back to
    /// the backend unnecessarily.
    pub fn seed(&mut self, last_saved: String) {
        self.last_saved = last_saved;
    }
}
