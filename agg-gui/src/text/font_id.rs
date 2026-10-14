//! Stable identities for [`Font`]s, the keys of every cache derived from a
//! font.
//!
//! A pointer (the font data `Arc`'s, or the `Arc<Font>`'s) is not an identity:
//! two fonts can share the same data with different fallback chains, and a
//! freed font's address can be handed to the next font allocated. Caches keyed
//! on a pointer then serve one font's glyphs or measurements for another. So
//! every font carries two ids drawn from one process-wide counter that never
//! repeats:
//!
//! - [`Font::id`] is unique to each `Font` value: every constructor, variant
//!   and builder that changes the font stamps a fresh one. Widgets that cache
//!   anything about "the font they last laid out with" (`Label`) compare it.
//! - [`Font::shape_id`] identifies what shaping depends on: the face data, the
//!   face index and the fallback chain. Variants that only restyle outlines
//!   (faux italic and bold), change vertical metrics or flip tabular digits
//!   (which the shape cache keys separately) keep it, so they share shaped
//!   glyphs; [`Font::from_bytes`] and [`Font::with_fallback`] start a new one.
//!   `text/shape_cache.rs` keys on it.

use std::sync::atomic::{AtomicU64, Ordering};

use super::Font;

/// The last id handed out. Ids start at 1, so 0 can mean "no font yet".
static NEXT_FONT_ID: AtomicU64 = AtomicU64::new(0);

/// A font id no font has had before (and none will have again).
pub(super) fn next_font_id() -> u64 {
    // Relaxed suffices: only uniqueness matters, not ordering with other
    // memory; a u64 cannot wrap in any real process lifetime.
    NEXT_FONT_ID.fetch_add(1, Ordering::Relaxed) + 1
}

impl Font {
    /// This font's identity, unique among every `Font` the process creates
    /// (never 0). Key caches of anything derived from a font on it rather
    /// than on a pointer, which a later font can reuse.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// The identity of this font's shaping: equal for two fonts exactly when
    /// they share face data, face index and fallback chain (see the module
    /// docs), so their shaped glyphs are interchangeable.
    pub fn shape_id(&self) -> u64 {
        self.shape_id
    }

    /// Give this font a fresh [`id`](Self::id), keeping its shaping identity.
    pub(super) fn restamp_id(&mut self) {
        self.id = next_font_id();
    }

    /// Give this font fresh [`id`](Self::id) and
    /// [`shape_id`](Self::shape_id): its shaping changed.
    pub(super) fn restamp_shape_id(&mut self) {
        self.id = next_font_id();
        self.shape_id = self.id;
    }
}
