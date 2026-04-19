//! System-wide font / text rendering settings.
//!
//! Mirrors the `theme::current_visuals` / `theme::set_visuals` pattern and
//! the scrollbar-style globals (`current_scroll_style` / `set_scroll_style`).
//! Widgets that care about rendering style (`Label`, `Button`, `TextField`,
//! ...) should consult these at **layout/paint time** so changes made by
//! the System window propagate without a widget-tree rebuild.
//!
//! # Convention
//!
//! Each setting has:
//! - an **override** stored in a thread-local cell (`None` or `false` by default),
//! - a getter (e.g. [`current_system_font`], [`lcd_enabled`]),
//! - a setter (e.g. [`set_system_font`], [`set_lcd_enabled`]).
//!
//! Widgets pick between the global override and their own per-instance value
//! — analogous to how a `ScrollView` takes the global scroll style unless
//! the caller wired an explicit one.

use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use crate::text::Font;

// ---------------------------------------------------------------------------
// Typography epoch
// ---------------------------------------------------------------------------
//
// Bumped every time any typography-style global (font, size scale,
// LCD, hinting, gamma, width, interval, faux weight/italic, primary
// weight) changes.  Backbuffered widgets (`Label`, `TextField`, …)
// compare this epoch against the one they rasterised at and
// self-invalidate on mismatch — same trick we use for theme epoch.
// Without this, dragging a slider in the System window would leave
// pre-existing `Label` caches showing the old style until something
// else invalidated them.

static TYPOGRAPHY_EPOCH: AtomicU64 = AtomicU64::new(1);

/// Current typography epoch.  Widget render paths read this each frame.
pub fn current_typography_epoch() -> u64 {
    TYPOGRAPHY_EPOCH.load(Ordering::Relaxed)
}

/// Internal helper: called by every setter in this module after it
/// writes.  Keeps the epoch in lock-step with the globals.
fn bump_typography_epoch() {
    TYPOGRAPHY_EPOCH.fetch_add(1, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Thread-local storage
// ---------------------------------------------------------------------------

thread_local! {
    /// System-wide font override.  `None` means "widgets keep whatever font
    /// they were constructed with".
    static SYSTEM_FONT:     RefCell<Option<Arc<Font>>> = RefCell::new(None);
    /// System-wide font size multiplier — applied to every widget's own
    /// `font_size` at paint/layout time.  `1.0` = unchanged.  Acts like
    /// egui's `pixels_per_point` for typography: shrink or enlarge ALL
    /// text while preserving the relative hierarchy (body stays smaller
    /// than headings, etc.).
    static FONT_SIZE_SCALE: RefCell<f64>  = RefCell::new(1.0);
    /// System-wide LCD-subpixel toggle.  When `true`, text-rendering widgets
    /// should prefer LCD output whenever they can determine their background
    /// colour (needed for correct per-channel compositing); fall back to
    /// grayscale AA otherwise.
    static LCD_ENABLED:     RefCell<bool> = RefCell::new(false);
    /// System-wide hinting toggle — forwarded to the font engine when the
    /// engine supports it.  `ttf-parser` does NOT run a hinting interpreter,
    /// so what we do is **Y-axis-only baseline hinting**: snap the glyph
    /// origin's Y coordinate to the pixel grid before rasterisation,
    /// matching the `(y + 0.5).floor()` convention from the AGG C++
    /// `truetype_test_02_win` demo.  This preserves horizontal subpixel
    /// positioning (critical for LCD) while giving sharper vertical
    /// metrics — the pragmatic compromise used by the agg-rust reference.
    static HINTING_ENABLED: RefCell<bool> = RefCell::new(false);

    // ── Typography-style parameters (drive the TrueType LCD Subpixel demo
    // and, once the render pipeline is wired up, every text paint
    // globally).  Ranges mirror the agg-rust `truetype_test` demo so
    // numbers stay comparable against the reference implementation.

    /// Gamma correction applied post-raster.  1.0 = off (linear output).
    /// Range 0.5..=2.5.
    static GAMMA:          RefCell<f64> = RefCell::new(1.0);
    /// Horizontal glyph width scale.  1.0 = native widths.
    /// Range 0.75..=1.25.
    static WIDTH:          RefCell<f64> = RefCell::new(1.0);
    /// Extra letter-spacing as a fraction of em.  0.0 = unchanged.
    /// Range -0.2..=0.2.
    static INTERVAL:       RefCell<f64> = RefCell::new(0.0);
    /// Synthetic boldness via outline contour offset.
    /// Range -1.0..=1.0; 0.0 = unchanged, positive = heavier, negative = lighter.
    static FAUX_WEIGHT:    RefCell<f64> = RefCell::new(0.0);
    /// Synthetic italic slant expressed as a horizontal-shear factor.
    /// Range -1.0..=1.0; 0.0 = upright.
    static FAUX_ITALIC:    RefCell<f64> = RefCell::new(0.0);
    /// LCD primary-weight (the pixel coverage weight of the own-channel
    /// vs the neighbouring channels in the 3-tap distribution LUT).
    /// Range 0.0..=1.0; default 1/3 gives a neutral LUT.
    static PRIMARY_WEIGHT: RefCell<f64> = RefCell::new(1.0 / 3.0);
}

// ---------------------------------------------------------------------------
// Font
// ---------------------------------------------------------------------------

/// Current system font override, if set.  Widgets should prefer this over
/// their own `self.font` when the override is `Some(_)` so user changes in
/// the System window propagate live.
pub fn current_system_font() -> Option<Arc<Font>> {
    SYSTEM_FONT.with(|c| c.borrow().clone())
}

/// Replace the system font override.  Pass `None` to clear and fall back
/// to per-widget fonts.
pub fn set_system_font(font: Option<Arc<Font>>) {
    SYSTEM_FONT.with(|c| *c.borrow_mut() = font);
    bump_typography_epoch();
}

// ---------------------------------------------------------------------------
// Font size scale
// ---------------------------------------------------------------------------

/// Current font size multiplier.  Widgets reading a `self.font_size`
/// should consult this (via e.g. `Label::active_font_size`) so a single
/// slider in the System window can grow or shrink all text uniformly.
pub fn current_font_size_scale() -> f64 {
    FONT_SIZE_SCALE.with(|c| *c.borrow())
}

/// Set the system font-size multiplier.  Clamped to a sensible range so
/// typos or edge-case inputs can't hide every label or fry the layout.
pub fn set_font_size_scale(scale: f64) {
    let clamped = scale.clamp(0.5, 3.0);
    FONT_SIZE_SCALE.with(|c| *c.borrow_mut() = clamped);
    bump_typography_epoch();
}

// ---------------------------------------------------------------------------
// LCD subpixel toggle
// ---------------------------------------------------------------------------

pub fn lcd_enabled() -> bool {
    LCD_ENABLED.with(|c| *c.borrow())
}

pub fn set_lcd_enabled(on: bool) {
    LCD_ENABLED.with(|c| *c.borrow_mut() = on);
    bump_typography_epoch();
}

// ---------------------------------------------------------------------------
// Hinting toggle
// ---------------------------------------------------------------------------

pub fn hinting_enabled() -> bool {
    HINTING_ENABLED.with(|c| *c.borrow())
}

pub fn set_hinting_enabled(on: bool) {
    HINTING_ENABLED.with(|c| *c.borrow_mut() = on);
    bump_typography_epoch();
}

// ---------------------------------------------------------------------------
// Typography-style parameters
// ---------------------------------------------------------------------------
//
// All six follow the same shape: an immutable thread-local, a getter,
// and a clamping setter.  The clamp ranges mirror the agg-rust
// `truetype_test` demo so results stay numerically comparable.  Callers
// (System window widgets + the TrueType LCD Subpixel demo) bind to
// these via `Rc<Cell<f64>>` mirrors owned by `SystemCells`; the global
// is the source-of-truth for rendering, the cell is the source-of-truth
// for UI widgets and disk persistence.

pub fn current_gamma() -> f64 { GAMMA.with(|c| *c.borrow()) }
pub fn set_gamma(v: f64) {
    let clamped = v.clamp(0.5, 2.5);
    GAMMA.with(|c| *c.borrow_mut() = clamped);
    bump_typography_epoch();
}

pub fn current_width() -> f64 { WIDTH.with(|c| *c.borrow()) }
pub fn set_width(v: f64) {
    let clamped = v.clamp(0.75, 1.25);
    WIDTH.with(|c| *c.borrow_mut() = clamped);
    bump_typography_epoch();
}

pub fn current_interval() -> f64 { INTERVAL.with(|c| *c.borrow()) }
pub fn set_interval(v: f64) {
    let clamped = v.clamp(-0.2, 0.2);
    INTERVAL.with(|c| *c.borrow_mut() = clamped);
    bump_typography_epoch();
}

pub fn current_faux_weight() -> f64 { FAUX_WEIGHT.with(|c| *c.borrow()) }
pub fn set_faux_weight(v: f64) {
    let clamped = v.clamp(-1.0, 1.0);
    FAUX_WEIGHT.with(|c| *c.borrow_mut() = clamped);
    bump_typography_epoch();
}

pub fn current_faux_italic() -> f64 { FAUX_ITALIC.with(|c| *c.borrow()) }
pub fn set_faux_italic(v: f64) {
    let clamped = v.clamp(-1.0, 1.0);
    FAUX_ITALIC.with(|c| *c.borrow_mut() = clamped);
    bump_typography_epoch();
}

pub fn current_primary_weight() -> f64 { PRIMARY_WEIGHT.with(|c| *c.borrow()) }
pub fn set_primary_weight(v: f64) {
    let clamped = v.clamp(0.0, 1.0);
    PRIMARY_WEIGHT.with(|c| *c.borrow_mut() = clamped);
    bump_typography_epoch();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lcd_flag_default_off() {
        // Reset to known state — other tests in the same thread may have
        // flipped it.  Use try-reset pattern.
        set_lcd_enabled(false);
        assert!(!lcd_enabled());
        set_lcd_enabled(true);
        assert!(lcd_enabled());
        set_lcd_enabled(false);
    }

    #[test]
    fn test_hinting_flag_default_off() {
        set_hinting_enabled(false);
        assert!(!hinting_enabled());
        set_hinting_enabled(true);
        assert!(hinting_enabled());
        set_hinting_enabled(false);
    }

    #[test]
    fn test_system_font_default_none() {
        // Reset first — other tests may have set it.
        set_system_font(None);
        assert!(current_system_font().is_none());
    }
}
