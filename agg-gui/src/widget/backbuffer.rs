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
    /// slider drag in the System window's typography controls invalidate
    /// every cached `Label` bitmap without bespoke hooks per widget.
    pub typography_epoch: u64,
    /// Async-state epoch (see
    /// [`crate::animation::async_state_epoch`]) — bumped when an
    /// off-thread / async source (e.g. an image fetch + decode)
    /// finishes outside the normal event-dispatch path that would
    /// otherwise mark widgets dirty.  Mismatch forces a re-raster
    /// so freshly-loaded data lands in newly-laid-out bounds.
    pub async_state_epoch: u64,
    /// Retained `LcdCoverage` planes (Y-up) kept alive between paints so an
    /// over-scan-band widget can *partially* re-raster only the edited line
    /// strip into the existing pixels instead of rebuilding the whole (up to
    /// 2×-viewport) buffer on every keystroke. Only populated on the band path
    /// (see [`crate::widget::Widget::backbuffer_band`]); `None` for every other
    /// widget, so the extra memory and reuse machinery never touch them.
    pub lcd_buffer: Option<crate::lcd_coverage::LcdBuffer>,
    /// Set by the framework immediately before a band widget's `paint`: `true`
    /// means the retained [`Self::lcd_buffer`] was reused unchanged (same size,
    /// mode and styling epochs), so the widget may repaint only its dirty line
    /// strip and leave the rest of the buffer intact. `false` forces a full
    /// repaint (first raster, resize, re-anchor, theme flip). Transient — the
    /// widget reads it during `paint` and must not rely on it afterwards.
    pub partial_allowed: bool,
    /// Stamped with a globally-unique value (see [`next_content_version`]) on
    /// every raster that changes the published `pixels` / `lcd_alpha` content.
    /// It lets backends that cache GPU textures by buffer *identity* detect an
    /// in-place content change: the dirty-strip path updates the retained planes
    /// through `Arc::make_mut`, which keeps the byte-buffer address stable, so a
    /// pointer-keyed texture cache would otherwise miss the edit. Pairing the
    /// buffer pointer with this version disambiguates. `0` means "never rastered".
    pub content_version: u64,
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
            lcd_buffer: None,
            partial_allowed: false,
            content_version: 0,
        }
    }

    /// Mark the cache dirty so the next paint re-rasterises.
    pub fn invalidate(&mut self) {
        self.dirty = true;
    }
}

/// Monotone, process-global content-revision counter for backbuffer caches.
///
/// Every raster that changes a cache's published pixels stamps
/// [`BackbufferCache::content_version`] with a fresh value from here. Making it
/// *global* (rather than per-cache) means a `(buffer_ptr, version)` pair can
/// never collide across surface lifetimes — a freed buffer's address may be
/// reused by a different cache, but the version it carries will always differ.
/// That keeps a pointer-keyed GPU texture cache ABA-proof. Starts at 1 so `0`
/// stays reserved for "never rastered".
pub(crate) fn next_content_version() -> u64 {
    static NEXT_CONTENT_VERSION: AtomicU64 = AtomicU64::new(1);
    NEXT_CONTENT_VERSION.fetch_add(1, Ordering::Relaxed)
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

/// Over-scan band request for a scrolling backbuffered widget.
///
/// Returned by [`crate::widget::Widget::backbuffer_band`]. It lets a widget
/// (today only [`crate::widgets::TextArea`]) raster a *band* of content taller
/// than its own viewport — the visible rect plus an extra `overscan_top` above
/// and `overscan_bottom` below — so that ordinary scrolling within the band
/// becomes a pure blit-offset change instead of a full backbuffer re-raster.
///
/// # Contract
///
/// * **Units.** `overscan_top` / `overscan_bottom` are extra logical pixels of
///   content rastered above / below the widget bounds. `blit_dy` is the
///   vertical blit offset in logical pixels (positive shifts the band's content
///   *up* in Y-up, revealing lower content). The band is anchored in content
///   space by the widget; `blit_dy` is the residual between the live scroll
///   offset and the anchor the band was rastered at.
/// * **Quantization.** The framework quantizes both the overscan extents and
///   `blit_dy` to whole *physical* pixels before sizing the buffer, translating
///   the raster, and translating the blit — so the LCD subpixel structure is
///   never resampled. The widget may pass raw (unquantized) logical values.
/// * **Who clips.** The framework clips the band blit to the widget's bounds on
///   every backend, so the over-scan margins never paint over sibling widgets.
///   The widget is responsible for painting opaque content across the *whole*
///   band (including the over-scan margins) so no gap shows through as the band
///   is scrolled, and for painting any fixed chrome (border) in `paint_overlay`
///   so it does not scroll with the band.
/// * **Invalidation.** The widget must keep the band's anchor (and hence the
///   rastered content) out of its backbuffer-cache signature, so scrolling
///   within the band does not invalidate the cache. It must re-anchor and
///   invalidate only when the live offset leaves the band (or the size /
///   content / style changes).
///
/// Returning `None` (the default) opts out entirely and keeps the byte-for-byte
/// original bounds-sized raster + 1:1 blit path.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BackbufferBand {
    /// Extra logical pixels of content rastered above the widget's top edge.
    pub overscan_top: f64,
    /// Extra logical pixels of content rastered below the widget's bottom edge.
    pub overscan_bottom: f64,
    /// Vertical blit offset in logical pixels (live offset − band anchor).
    /// Positive shifts the band content up (Y-up), revealing lower content.
    pub blit_dy: f64,
    /// Widget-local Y-up logical `(lo, hi)` extent of the rows the widget will
    /// repaint *if* the framework grants a partial (`partial_allowed`) re-raster
    /// this paint. `None` means a granted partial would still repaint the whole
    /// band (no confined strip). The framework uses this to limit the top-down
    /// plane update to exactly those rows, so it must cover byte-for-byte the
    /// same extent the widget fills and clips during its strip repaint.
    pub dirty_strip_y: Option<(f64, f64)>,
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
