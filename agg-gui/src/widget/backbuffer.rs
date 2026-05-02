//! Widget backbuffer types: caching specs, retained state, and compositing layers.
//!
//! Any widget can opt into a cached CPU backbuffer by returning `Some(&mut ...)`
//! from [`Widget::backbuffer_cache_mut`].  The framework's `paint_subtree`
//! handles caching transparently: when the widget is dirty (or has no bitmap
//! yet) it allocates a fresh `Framebuffer`, runs `widget.paint` + all children
//! into it via a software `GfxCtx`, and caches the resulting RGBA8 pixels as a
//! shared `Arc<Vec<u8>>`.  Every subsequent frame that finds the widget clean
//! just blits the cached pixels through `ctx.draw_image_rgba_arc` — zero AGG
//! cost in steady state.  On the GL backend the `Arc`'s pointer identity keys
//! the GPU texture cache (see `arc_texture_cache`), so the hardware texture
//! is also reused across frames and dropped when the bitmap drops.
//!
//! LCD subpixel rendering works naturally inside a backbuffer: the widget
//! paints its own background first (so text has a solid dst) and then any
//! `fill_text` call composites the per-channel coverage mask onto that
//! destination.  No walk / sample / bg-declaration needed.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use crate::layout_props::Insets;

/// How a widget's backbuffer stores pixels.
///
/// The choice controls what the framework allocates as the render
/// target during `paint_subtree_backbuffered` and how the cached
/// bitmap is composited back onto the parent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackbufferMode {
    /// 8-bit straight-alpha RGBA.  Standard Porter-Duff `SRC_ALPHA,
    /// ONE_MINUS_SRC_ALPHA` composite on blit.  Works for any widget,
    /// including ones with transparent areas.  Text inside is grayscale
    /// AA (no LCD subpixel).
    Rgba,
    /// 3 bytes-per-pixel **composited opaque RGB** — no alpha channel.
    /// Every fill (rects, strokes, text, etc.) inside the buffer goes
    /// through the 3× horizontal supersample + 5-tap filter + per-channel
    /// src-over pipeline described in `lcd-subpixel-compositing.md`.
    /// The buffer is blitted as an opaque RGB texture.
    ///
    /// **Contract:** the widget is responsible for painting content
    /// that covers its full bounds with opaque fills (starting with a
    /// bg rect).  Uncovered pixels land as black on the parent because
    /// there is no alpha channel to carry "no paint here."
    LcdCoverage,
}

/// Unified backbuffer target kind requested by a widget.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackbufferKind {
    /// Paint directly into the parent render target.
    None,
    /// Retained software RGBA framebuffer.
    SoftwareRgba,
    /// Retained software LCD coverage framebuffer.
    SoftwareLcd,
    /// Retained GL framebuffer object.
    GlFbo,
}

/// Widget-owned backbuffer request. Windows use this for retained GL FBOs,
/// while existing label/text-field CPU caches map naturally to the software
/// variants.
#[derive(Clone, Copy, Debug)]
pub struct BackbufferSpec {
    pub kind: BackbufferKind,
    pub cached: bool,
    pub alpha: f64,
    pub outsets: Insets,
    pub rounded_clip: Option<f64>,
}

impl BackbufferSpec {
    pub const fn none() -> Self {
        Self {
            kind: BackbufferKind::None,
            cached: false,
            alpha: 1.0,
            outsets: Insets::ZERO,
            rounded_clip: None,
        }
    }
}

impl Default for BackbufferSpec {
    fn default() -> Self {
        Self::none()
    }
}

/// A CPU bitmap owned by a widget that opts into backbuffer caching.
///
/// The framework re-rasterises when the cache's explicit dirty flag is set or
/// when global styling epochs change.
pub struct BackbufferCache {
    /// In **Rgba** mode: top-row-first RGBA8 pixels, straight alpha.
    /// Blitted via [`DrawCtx::draw_image_rgba_arc`].
    ///
    /// In **LcdCoverage** mode: top-row-first **colour plane** — 3
    /// bytes/pixel (R_premult, G_premult, B_premult) matching the
    /// convention of [`crate::lcd_coverage::LcdBuffer::color_plane`]
    /// flipped to top-down.  The companion alpha plane lives in
    /// [`Self::lcd_alpha`].
    pub pixels: Option<Arc<Vec<u8>>>,
    /// `LcdCoverage`-mode companion to `pixels`: top-row-first per-channel
    /// **alpha plane** (3 bytes/pixel, `(R_alpha, G_alpha, B_alpha)`).
    /// `None` means this is a plain Rgba cache.  When `Some`, the blit
    /// step uses [`DrawCtx::draw_lcd_backbuffer_arc`] to preserve the
    /// per-channel subpixel information through to the destination —
    /// required for LCD chroma to survive the cache round-trip.
    pub lcd_alpha: Option<Arc<Vec<u8>>>,
    pub width: u32,
    pub height: u32,
    /// When true, the next paint will re-rasterise rather than reusing
    /// `pixels`.  Widgets set this from their mutation paths
    /// (`set_text`, `set_color`, focus/hover changes, etc.) and the
    /// framework clears it after a successful re-raster.
    pub dirty: bool,
    /// Visuals epoch (see [`crate::theme::current_visuals_epoch`]) recorded
    /// the last time this cache was populated.  `paint_subtree_backbuffered`
    /// compares it against the live epoch and forces a re-raster on mismatch,
    /// so widgets whose text/fill colours come from `ctx.visuals()` refresh
    /// automatically on a dark/light theme flip without needing every widget
    /// to subscribe to theme-change events.
    pub theme_epoch: u64,
    /// Typography epoch (see
    /// [`crate::font_settings::current_typography_epoch`]) — same
    /// pattern as `theme_epoch` but for font / size scale / LCD /
    /// hinting / gamma / width / interval / faux-* globals.  Lets a
    /// slider drag in the LCD Subpixel demo invalidate every cached
    /// `Label` bitmap without bespoke hooks per widget.
    pub typography_epoch: u64,
    /// Async-state epoch (see
    /// [`crate::animation::async_state_epoch`]) — bumped when an
    /// off-thread / async source (e.g. an image fetch + decode)
    /// finishes outside the normal event-dispatch path that would
    /// otherwise mark widgets dirty.  Mismatch forces a re-raster
    /// so freshly-loaded data lands in newly-laid-out bounds.
    pub async_state_epoch: u64,
}

impl BackbufferCache {
    pub fn new() -> Self {
        Self {
            pixels: None,
            lcd_alpha: None,
            width: 0,
            height: 0,
            dirty: true,
            theme_epoch: 0,
            typography_epoch: 0,
            async_state_epoch: 0,
        }
    }

    /// Mark the cache dirty so the next paint re-rasterises.
    pub fn invalidate(&mut self) {
        self.dirty = true;
    }
}

impl Default for BackbufferCache {
    fn default() -> Self {
        Self::new()
    }
}

static NEXT_BACKBUFFER_ID: AtomicU64 = AtomicU64::new(1);

/// Retained widget backbuffer state shared by software and GL implementations.
pub struct BackbufferState {
    id: u64,
    pub cache: BackbufferCache,
    pub dirty: bool,
    pub width: u32,
    pub height: u32,
    pub spec_kind: BackbufferKind,
    /// Visuals epoch recorded the last time this retained surface was repainted.
    /// Retained backend layers compare it against the live theme epoch so a
    /// dark/light flip rebuilds the window/layer in the shared paint path.
    pub theme_epoch: u64,
    /// Typography epoch recorded the last time this retained surface was
    /// repainted. Without this, a clean parent FBO can keep compositing old
    /// text after global font/LCD settings change.
    pub typography_epoch: u64,
    /// Async-state epoch (see [`crate::animation::async_state_epoch`])
    /// recorded the last paint.  Mismatch forces a re-raster so a
    /// freshly-arrived async result (image fetch, font load) doesn't
    /// composite the previous frame's stale FBO contents.
    pub async_state_epoch: u64,
    pub repaint_count: u64,
    pub composite_count: u64,
}

impl BackbufferState {
    pub fn new() -> Self {
        Self {
            id: NEXT_BACKBUFFER_ID.fetch_add(1, Ordering::Relaxed),
            cache: BackbufferCache::new(),
            dirty: true,
            width: 0,
            height: 0,
            spec_kind: BackbufferKind::None,
            theme_epoch: 0,
            typography_epoch: 0,
            async_state_epoch: 0,
            repaint_count: 0,
            composite_count: 0,
        }
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn invalidate(&mut self) {
        self.dirty = true;
        self.cache.invalidate();
    }
}

impl Default for BackbufferState {
    fn default() -> Self {
        Self::new()
    }
}

/// Offscreen compositing layer requested by a widget for itself and its
/// descendants.
#[derive(Clone, Copy, Debug)]
pub struct CompositingLayer {
    /// Extra transparent pixels to the left of the widget bounds.
    pub outset_left: f64,
    /// Extra transparent pixels below the widget bounds.
    pub outset_bottom: f64,
    /// Extra transparent pixels to the right of the widget bounds.
    pub outset_right: f64,
    /// Extra transparent pixels above the widget bounds.
    pub outset_top: f64,
    /// Whole-layer opacity applied while compositing back to the parent.
    pub alpha: f64,
}

impl CompositingLayer {
    pub const fn new(
        outset_left: f64,
        outset_bottom: f64,
        outset_right: f64,
        outset_top: f64,
        alpha: f64,
    ) -> Self {
        Self {
            outset_left,
            outset_bottom,
            outset_right,
            outset_top,
            alpha,
        }
    }
}
