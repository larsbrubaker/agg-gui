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
    /// System-wide LCD-subpixel override.  When `Some(true|false)`, text-
    /// rendering widgets honour it directly.  When `None` (the default),
    /// [`lcd_enabled`] derives the effective value from
    /// [`crate::device_scale`]: LCD is enabled at standard DPI (scale ≤
    /// 1.25) and disabled at HiDPI, because LCD subpixel rendering only
    /// pays off when subpixels are roughly the size of a glyph stem; at
    /// 2× scale the AA halo is already wide enough that grayscale wins on
    /// chroma fringing while looking identical otherwise.  An explicit
    /// [`set_lcd_enabled`] overrides the auto-derivation; apps that just
    /// want the default should never need to call it.
    static LCD_ENABLED:     RefCell<Option<bool>> = const { RefCell::new(None) };
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

/// Whether widgets should rasterise text through the LCD subpixel path.
///
/// Whether widgets should rasterise text through the LCD subpixel path.
///
/// **Hard cap first:** LCD is NEVER used above standard density.  The gate
/// keys on the *effective* scale ([`crate::ux_scale::effective_scale`] =
/// device DPR × UX zoom), because LCD subpixel rendering only pays off when
/// individual physical pixels are large enough to resolve the R/G/B
/// sub-stripes — and "small pixels" come from a high UX zoom (mobile /
/// accessibility) just as much as from a HiDPI panel.  Above `1.25×` it is
/// pure overhead with no visible benefit, so we force the cheaper grayscale
/// path *regardless of any explicit override*.  This also keeps
/// CPU-backbuffered widgets (the menu bar) off the LCD blit at high scale,
/// where they would otherwise shrink.
///
/// At standard density the explicit override set via [`set_lcd_enabled`]
/// wins if present; otherwise LCD defaults on.  So platform shells generally
/// don't need to call [`set_lcd_enabled`] at all.
pub fn lcd_enabled() -> bool {
    // Never LCD at high effective density — overrides included.
    if crate::ux_scale::effective_scale() > 1.25 {
        return false;
    }
    if let Some(explicit) = LCD_ENABLED.with(|c| *c.borrow()) {
        return explicit;
    }
    true
}

/// Pin LCD subpixel rendering to a specific value, overriding the
/// device-scale-derived default.  System-window toggles use this; apps
/// that just want sensible default behaviour should not call it.
pub fn set_lcd_enabled(on: bool) {
    LCD_ENABLED.with(|c| *c.borrow_mut() = Some(on));
    bump_typography_epoch();
}

/// Drop any explicit override and return to device-scale-derived auto.
/// Counterpart to [`set_lcd_enabled`]; used by tests and by System-
/// window "reset to default" affordances.
pub fn clear_lcd_enabled_override() {
    LCD_ENABLED.with(|c| *c.borrow_mut() = None);
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

pub fn current_gamma() -> f64 {
    GAMMA.with(|c| *c.borrow())
}
pub fn set_gamma(v: f64) {
    let clamped = v.clamp(0.5, 2.5);
    GAMMA.with(|c| *c.borrow_mut() = clamped);
    bump_typography_epoch();
}

pub fn current_width() -> f64 {
    WIDTH.with(|c| *c.borrow())
}
pub fn set_width(v: f64) {
    let clamped = v.clamp(0.75, 1.25);
    WIDTH.with(|c| *c.borrow_mut() = clamped);
    bump_typography_epoch();
}

pub fn current_interval() -> f64 {
    INTERVAL.with(|c| *c.borrow())
}
pub fn set_interval(v: f64) {
    let clamped = v.clamp(-0.2, 0.2);
    INTERVAL.with(|c| *c.borrow_mut() = clamped);
    bump_typography_epoch();
}

pub fn current_faux_weight() -> f64 {
    FAUX_WEIGHT.with(|c| *c.borrow())
}
pub fn set_faux_weight(v: f64) {
    let clamped = v.clamp(-1.0, 1.0);
    FAUX_WEIGHT.with(|c| *c.borrow_mut() = clamped);
    bump_typography_epoch();
}

pub fn current_faux_italic() -> f64 {
    FAUX_ITALIC.with(|c| *c.borrow())
}
pub fn set_faux_italic(v: f64) {
    let clamped = v.clamp(-1.0, 1.0);
    FAUX_ITALIC.with(|c| *c.borrow_mut() = clamped);
    bump_typography_epoch();
}

pub fn current_primary_weight() -> f64 {
    PRIMARY_WEIGHT.with(|c| *c.borrow())
}
pub fn set_primary_weight(v: f64) {
    let clamped = v.clamp(0.0, 1.0);
    PRIMARY_WEIGHT.with(|c| *c.borrow_mut() = clamped);
    bump_typography_epoch();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lcd_flag_explicit_override_round_trips() {
        // Reset to known state — thread-locals can leak across tests reusing
        // the same worker thread.  Standard density so the high-scale cap in
        // `lcd_enabled` doesn't mask the override under test.
        crate::device_scale::set_device_scale(1.0);
        crate::ux_scale::set_ux_scale(1.0);
        set_lcd_enabled(false);
        assert!(!lcd_enabled());
        set_lcd_enabled(true);
        assert!(lcd_enabled());
        clear_lcd_enabled_override();
    }

    #[test]
    fn test_lcd_flag_auto_derives_from_device_scale_when_no_override() {
        use crate::device_scale::set_device_scale;
        clear_lcd_enabled_override();
        set_device_scale(1.0);
        assert!(lcd_enabled(), "standard DPI should default to LCD on");
        set_device_scale(2.0);
        assert!(!lcd_enabled(), "HiDPI should default to LCD off");
        // Restore to a sane state for sibling tests.
        set_device_scale(1.0);
    }

    #[test]
    fn test_lcd_auto_disabled_at_high_effective_scale_from_ux_zoom() {
        // LCD subpixel rendering is pointless overhead once the on-screen
        // pixel density is high — and "high density" can come from the UX
        // zoom (mobile / accessibility) just as much as from the device DPR.
        // A device at 1.0 DPR with ux_scale 1.7 renders everything at 1.7×;
        // LCD must auto-disable there, exactly as it does at 1.7× DPR.
        // Regression: the auto-derivation keyed on `device_scale` alone, so a
        // ux-zoomed standard-DPI display kept LCD on, which routed the
        // CPU-backbuffered menu bar through the LCD blit and rendered it tiny.
        use crate::device_scale::set_device_scale;
        use crate::ux_scale::set_ux_scale;
        clear_lcd_enabled_override();
        set_device_scale(1.0);
        set_ux_scale(1.0);
        assert!(lcd_enabled(), "standard DPI + no zoom should default to LCD on");
        set_ux_scale(1.7);
        assert!(
            !lcd_enabled(),
            "high effective scale via ux zoom should default to LCD off"
        );
        // The high-scale gate is a HARD CAP: it wins even over an explicit
        // override, so "force LCD on" can't reintroduce the overhead (and the
        // tiny-menu blit) at high density.
        set_lcd_enabled(true);
        assert!(
            !lcd_enabled(),
            "explicit LCD-on override must still be capped off at high effective scale"
        );
        // ...but at standard density the explicit override is honoured.
        set_ux_scale(1.0);
        assert!(lcd_enabled(), "override LCD-on must apply at standard density");
        // Restore sane state for sibling tests.
        clear_lcd_enabled_override();
        set_ux_scale(1.0);
        set_device_scale(1.0);
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
