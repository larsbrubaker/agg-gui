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
use std::sync::Arc;

use crate::text::Font;

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
    /// so this is a stored-but-not-yet-applied flag today; it becomes
    /// live when we wire in a hinting-capable rasterizer.
    static HINTING_ENABLED: RefCell<bool> = RefCell::new(false);
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
}

// ---------------------------------------------------------------------------
// LCD subpixel toggle
// ---------------------------------------------------------------------------

pub fn lcd_enabled() -> bool {
    LCD_ENABLED.with(|c| *c.borrow())
}

pub fn set_lcd_enabled(on: bool) {
    LCD_ENABLED.with(|c| *c.borrow_mut() = on);
}

// ---------------------------------------------------------------------------
// Hinting toggle
// ---------------------------------------------------------------------------

pub fn hinting_enabled() -> bool {
    HINTING_ENABLED.with(|c| *c.borrow())
}

pub fn set_hinting_enabled(on: bool) {
    HINTING_ENABLED.with(|c| *c.borrow_mut() = on);
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
