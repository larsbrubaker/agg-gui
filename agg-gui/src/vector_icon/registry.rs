//! Process-global icon registry: id → [`VectorIcon`].
//!
//! Widgets deep inside the tree paint icons that only the *host* knows
//! about (a node editor draws a boolean-operation strip; the artwork
//! belongs to the app, not to agg-gui). Threading an icon table down
//! through every schema type and every paint call would make the schema
//! expensive to clone and every intermediate layer aware of icons, so
//! the lookup goes through a global instead — the same shape as the
//! system font slot and the SVG parse-options cell.
//!
//! It is a `RwLock<HashMap>` rather than a thread-local because the
//! host registers once at startup while tests paint from many threads;
//! a thread-local would silently render icon-less in every thread but
//! the one that registered. A poisoned lock is recovered rather than
//! propagated: a panic elsewhere must not turn every later icon lookup
//! into a panic of its own.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock, RwLock};

use super::VectorIcon;

type IconMap = HashMap<Arc<str>, Arc<VectorIcon>>;

static ICONS: OnceLock<RwLock<IconMap>> = OnceLock::new();

fn cell() -> &'static RwLock<IconMap> {
    ICONS.get_or_init(|| RwLock::new(HashMap::new()))
}

/// Register `icon` under `id`, replacing any previous registration.
///
/// Ids are free-form; hosts should namespace them (`"boolean.combine"`)
/// so two features cannot collide.
pub fn register_icon(id: impl Into<Arc<str>>, icon: VectorIcon) {
    let mut map = cell().write().unwrap_or_else(|e| e.into_inner());
    map.insert(id.into(), Arc::new(icon));
}

/// Look up a registered icon. `None` means "nothing registered under
/// this id" — callers are expected to fall back to something visible
/// (text) rather than painting nothing.
pub fn icon(id: &str) -> Option<Arc<VectorIcon>> {
    let map = cell().read().unwrap_or_else(|e| e.into_inner());
    map.get(id).cloned()
}

/// Every registered id, unordered. Diagnostics and tests.
pub fn icon_ids() -> Vec<Arc<str>> {
    let map = cell().read().unwrap_or_else(|e| e.into_inner());
    map.keys().cloned().collect()
}

// There is deliberately no `clear_icons`. The registry is global and
// widgets read it mid-paint from any thread, so a clear is a foot-gun
// with no legitimate caller: production registers once at startup, and a
// test that wants isolation registers under its own id (re-registering
// an id replaces it, which is all "reset this icon" ever needs).
