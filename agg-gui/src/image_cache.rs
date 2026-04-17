//! Content-keyed pixel cache shared across `Label` (and future) backbuffers.
//!
//! # Why
//!
//! Widgets like `TreeView` and `InspectorPanel` rebuild their child widgets on
//! every `layout()` call, so a per-instance cache cannot hit on rebuild —
//! every new `Label` would re-rasterize its glyphs from scratch.  This module
//! owns a thread-local `HashMap<LabelPixelKey, Arc<Vec<u8>>>` so that any
//! Label with the same text/font/size/color/bounds configuration reuses the
//! same `Arc`.
//!
//! # Pairs with the GL texture cache
//!
//! Downstream in the GL renderer, a second cache is keyed on `Arc::as_ptr`
//! with the stored value holding a `Weak<Vec<u8>>` + GL texture handle.  When
//! this L1 cache evicts an entry AND no Label is mid-paint holding a strong
//! clone, the `Arc` drops, the `Weak` in the GL cache fails to upgrade, and
//! the GL texture is scheduled for deletion on the next frame.  Pattern
//! mirrors MatterCAD's `ConditionalWeakTable<byte[], ImageTexturePlugin>`.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use crate::color::Color;

/// Maximum entries retained in the L1 pixel cache.  Reached only when ≥ this
/// many unique (text, font, size, color, bounds) tuples are live at once; in
/// practice the inspector + treeview hold a few hundred.  When exceeded, the
/// least-recently-used key is evicted — dropping the last strong ref to its
/// `Arc` and letting the GL cache release the texture.
const CACHE_CAP: usize = 1024;

/// Fully-qualifies a cache entry.  All inputs that can affect the rendered
/// pixels MUST appear here — missing a field means two visually-distinct
/// labels collide on the same cached bitmap.
#[derive(Clone, Eq, PartialEq, Hash)]
pub struct LabelPixelKey {
    text:       String,
    /// Pointer identity of the primary font's data `Arc<Vec<u8>>`.  The
    /// fallback chain is implicit in the primary font's identity (fallbacks
    /// are attached at construction and never change for a given `Font`
    /// instance), so only the primary pointer is needed here.
    font_ptr:   usize,
    /// Bit pattern of the `f64` font size — so `14.0` and `14.0000001` keep
    /// distinct entries rather than colliding on `==`.
    size_bits:  u64,
    /// Packed `[r, g, b, a]` in 8-bit channels so colour identity survives
    /// conversion without floating-point fuzz.
    color_bits: u32,
    w:          u32,
    h:          u32,
    align:      u8,
}

impl LabelPixelKey {
    pub fn new(
        text:      &str,
        font_ptr:  usize,
        font_size: f64,
        color:     Color,
        w:         u32,
        h:         u32,
        align:     u8,
    ) -> Self {
        let r = (color.r * 255.0).clamp(0.0, 255.0) as u32;
        let g = (color.g * 255.0).clamp(0.0, 255.0) as u32;
        let b = (color.b * 255.0).clamp(0.0, 255.0) as u32;
        let a = (color.a * 255.0).clamp(0.0, 255.0) as u32;
        Self {
            text:       text.to_owned(),
            font_ptr,
            size_bits:  font_size.to_bits(),
            color_bits: (r << 24) | (g << 16) | (b << 8) | a,
            w, h, align,
        }
    }
}

struct CacheInner {
    /// Owned pixel buffers keyed by content identity.  Each value is the
    /// single `Arc` handed out to every requester of the same key — pointer
    /// identity is stable for as long as the entry lives, which is what
    /// allows the GL texture cache to key on `Arc::as_ptr`.
    map:   HashMap<LabelPixelKey, Arc<Vec<u8>>>,
    /// LRU order: front = coldest, back = hottest.
    order: VecDeque<LabelPixelKey>,
}

impl CacheInner {
    fn new() -> Self { Self { map: HashMap::new(), order: VecDeque::new() } }

    fn touch(&mut self, key: &LabelPixelKey) {
        if let Some(pos) = self.order.iter().position(|k| k == key) {
            self.order.remove(pos);
        }
        self.order.push_back(key.clone());
    }

    fn get_or_insert<F: FnOnce() -> Vec<u8>>(
        &mut self,
        key:    LabelPixelKey,
        raster: F,
    ) -> Arc<Vec<u8>> {
        if let Some(arc) = self.map.get(&key) {
            let arc = Arc::clone(arc);
            self.touch(&key);
            return arc;
        }
        let arc = Arc::new(raster());
        self.map.insert(key.clone(), Arc::clone(&arc));
        self.order.push_back(key);
        while self.order.len() > CACHE_CAP {
            if let Some(cold) = self.order.pop_front() {
                self.map.remove(&cold);
            }
        }
        arc
    }
}

thread_local! {
    static LABEL_PIXEL_CACHE: RefCell<CacheInner> = RefCell::new(CacheInner::new());
}

/// Return the cached pixel buffer for `key`, rasterizing via `raster` on the
/// first request.  `raster` MUST produce a `Vec<u8>` of length
/// `key.w * key.h * 4` in **straight-alpha** RGBA8, top-row-first order.
pub fn get_or_raster<F: FnOnce() -> Vec<u8>>(
    key:    LabelPixelKey,
    raster: F,
) -> Arc<Vec<u8>> {
    LABEL_PIXEL_CACHE.with(|c| c.borrow_mut().get_or_insert(key, raster))
}

/// Evict all entries.  Useful when the active theme changes wholesale and
/// every cached bitmap is guaranteed stale.  Does NOT delete GL textures —
/// they're released lazily as their `Weak` references die.
pub fn clear() {
    LABEL_PIXEL_CACHE.with(|c| {
        let mut c = c.borrow_mut();
        c.map.clear();
        c.order.clear();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(text: &str, w: u32, h: u32) -> LabelPixelKey {
        LabelPixelKey::new(text, 0x1234, 14.0, Color::black(), w, h, 0)
    }

    /// Two requests with the same key must return `Arc`s with identical
    /// pointer identity — this is the property the GL texture cache relies on.
    #[test]
    fn test_same_key_returns_same_arc() {
        clear();
        let a = get_or_raster(k("hello", 100, 30), || vec![0u8; 100 * 30 * 4]);
        let b = get_or_raster(k("hello", 100, 30), || {
            panic!("raster must NOT run on cache hit")
        });
        assert!(Arc::ptr_eq(&a, &b));
    }

    /// Different keys must produce distinct `Arc`s (and run `raster` each).
    #[test]
    fn test_different_keys_produce_distinct_arcs() {
        clear();
        let a = get_or_raster(k("one", 100, 30), || vec![1u8; 100 * 30 * 4]);
        let b = get_or_raster(k("two", 100, 30), || vec![2u8; 100 * 30 * 4]);
        assert!(!Arc::ptr_eq(&a, &b));
    }

    /// LRU eviction must release the cache's strong reference.  After
    /// eviction, if no other strong holder remains, the `Weak` must fail to
    /// upgrade — this is what lets the GL cache know the texture is freeable.
    #[test]
    fn test_lru_eviction_drops_weak_target() {
        clear();
        let evictable = get_or_raster(k("evictable", 8, 8), || vec![0u8; 8 * 8 * 4]);
        let weak = Arc::downgrade(&evictable);
        drop(evictable);

        // Push CACHE_CAP distinct keys through — evicts the first entry.
        for i in 0..(CACHE_CAP + 1) {
            let key = LabelPixelKey::new(
                &format!("k{i}"), 0x2222, 14.0, Color::black(), 8, 8, 0,
            );
            let _ = get_or_raster(key, || vec![0u8; 8 * 8 * 4]);
        }

        assert!(
            weak.upgrade().is_none(),
            "after LRU eviction, the only strong ref drops and Weak must die"
        );
    }
}
