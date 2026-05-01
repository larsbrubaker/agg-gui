//! Theme system — dark / light mode colour palettes.
//!
//! # Overview
//!
//! [`Visuals`] holds every colour used by the widget library.  Two built-in
//! palettes are provided via [`Visuals::dark`] and [`Visuals::light`].
//!
//! The *current* visuals are stored in a thread-local so widgets can access
//! them from `paint()` without an extra parameter.  Call [`set_visuals`] once
//! per frame (before painting) to apply a palette; call [`current_visuals`] to
//! read it from inside a widget.
//!
//! [`DrawCtx::visuals()`](crate::draw_ctx::DrawCtx::visuals) is a convenience
//! that delegates to [`current_visuals`], so widget paint methods only need
//! `ctx.visuals()`.

use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::color::Color;

// ---------------------------------------------------------------------------
// Theme preference
// ---------------------------------------------------------------------------

/// User preference for which palette to apply.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
pub enum ThemePreference {
    #[default]
    Dark,
    Light,
    /// Follow the OS setting.  Unimplemented for now — falls back to `Dark`.
    System,
}

impl ThemePreference {
    pub fn key(self) -> &'static str {
        match self {
            ThemePreference::Dark => "dark",
            ThemePreference::Light => "light",
            ThemePreference::System => "system",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "dark" => Some(ThemePreference::Dark),
            "light" => Some(ThemePreference::Light),
            "system" => Some(ThemePreference::System),
            _ => None,
        }
    }
}

/// Built-in accent swatches exposed by the demo and usable by hosts.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
pub enum AccentColor {
    #[default]
    Blue,
    Purple,
    Pink,
    Red,
    Orange,
    Yellow,
    Green,
    Teal,
}

impl AccentColor {
    pub const ALL: [AccentColor; 8] = [
        AccentColor::Blue,
        AccentColor::Purple,
        AccentColor::Pink,
        AccentColor::Red,
        AccentColor::Orange,
        AccentColor::Yellow,
        AccentColor::Green,
        AccentColor::Teal,
    ];

    pub fn color(self) -> Color {
        match self {
            AccentColor::Blue => Color::rgb(0.22, 0.45, 0.88),
            AccentColor::Purple => Color::rgb(0.48, 0.36, 0.86),
            AccentColor::Pink => Color::rgb(0.78, 0.28, 0.58),
            AccentColor::Red => Color::rgb(0.82, 0.24, 0.24),
            AccentColor::Orange => Color::rgb(0.90, 0.46, 0.18),
            AccentColor::Yellow => Color::rgb(0.82, 0.62, 0.16),
            AccentColor::Green => Color::rgb(0.20, 0.62, 0.34),
            AccentColor::Teal => Color::rgb(0.14, 0.62, 0.66),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            AccentColor::Blue => "Blue",
            AccentColor::Purple => "Purple",
            AccentColor::Pink => "Pink",
            AccentColor::Red => "Red",
            AccentColor::Orange => "Orange",
            AccentColor::Yellow => "Yellow",
            AccentColor::Green => "Green",
            AccentColor::Teal => "Teal",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            AccentColor::Blue => "blue",
            AccentColor::Purple => "purple",
            AccentColor::Pink => "pink",
            AccentColor::Red => "red",
            AccentColor::Orange => "orange",
            AccentColor::Yellow => "yellow",
            AccentColor::Green => "green",
            AccentColor::Teal => "teal",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "blue" => Some(AccentColor::Blue),
            "purple" => Some(AccentColor::Purple),
            "pink" => Some(AccentColor::Pink),
            "red" => Some(AccentColor::Red),
            "orange" => Some(AccentColor::Orange),
            "yellow" => Some(AccentColor::Yellow),
            "green" => Some(AccentColor::Green),
            "teal" => Some(AccentColor::Teal),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Visuals (complete colour palette)
// ---------------------------------------------------------------------------

/// All colours used by the widget library.
///
/// The canonical way to access the active palette inside `Widget::paint` is:
/// ```ignore
/// let v = ctx.visuals();
/// ctx.set_fill_color(v.window_fill);
/// ```
#[derive(Clone, Debug)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
pub struct Visuals {
    // ── Chrome ────────────────────────────────────────────────────────────────
    /// Canvas / app background (behind all floating windows).
    pub bg_color: Color,
    /// Sidebar / panel background.
    pub panel_fill: Color,
    /// Top menu bar background.
    pub top_bar_bg: Color,

    // ── Floating window ───────────────────────────────────────────────────────
    /// Window content-area background.
    pub window_fill: Color,
    /// Window title bar background (idle).
    pub window_title_fill: Color,
    /// Window title bar background while dragging.
    pub window_title_fill_drag: Color,
    /// Drop-shadow colour (semi-transparent black/dark).
    pub window_shadow: Color,
    /// Thin border drawn around the window.
    pub window_stroke: Color,
    /// Title bar text colour.
    pub window_title_text: Color,
    /// Close button background (idle).
    pub window_close_bg: Color,
    /// Close button background (hovered).
    pub window_close_bg_hovered: Color,
    /// Close button × glyph colour.
    pub window_close_fg: Color,
    /// Resize edge / corner highlight colour when hovered (not yet dragging).
    pub window_resize_hover: Color,
    /// Resize edge / corner highlight colour while actively dragging to resize.
    pub window_resize_active: Color,

    // ── Text ──────────────────────────────────────────────────────────────────
    /// Body text colour.
    pub text_color: Color,
    /// Secondary / dimmed text (hints, labels).
    pub text_dim: Color,
    /// Hyperlink colour (idle).
    pub text_link: Color,
    /// Hyperlink colour (hovered).
    pub text_link_hovered: Color,

    // ── Accent / primary action colour ────────────────────────────────────────
    /// Used for checked states, active tabs, slider fill, button backgrounds.
    pub accent: Color,
    /// Accent colour when hovered.
    pub accent_hovered: Color,
    /// Accent colour when pressed / active.
    pub accent_pressed: Color,
    /// Low-opacity accent used for focus rings.
    pub accent_focus: Color,

    // ── Interactive widgets (checkbox, radio, drag-value, …) ──────────────────
    /// Widget background when unchecked / idle.
    pub widget_bg: Color,
    /// Widget background when hovered (unchecked).
    pub widget_bg_hovered: Color,
    /// Widget border / outline (unchecked).
    pub widget_stroke: Color,
    /// Widget border / outline (checked / active).
    pub widget_stroke_active: Color,

    // ── Slider / progress bar track ───────────────────────────────────────────
    pub track_bg: Color,

    // ── Scrollbar ─────────────────────────────────────────────────────────────
    pub scroll_track: Color,
    pub scroll_thumb: Color,
    pub scroll_thumb_hovered: Color,
    pub scroll_thumb_dragging: Color,

    // ── Separator / divider ───────────────────────────────────────────────────
    pub separator: Color,

    // ── Text selection highlight ──────────────────────────────────────────────
    /// Background colour behind selected text while the widget is focused.
    pub selection_bg: Color,
    /// Background colour behind selected text while the widget is NOT focused.
    /// Uses a neutral grey to signal that the selection is inactive.
    pub selection_bg_unfocused: Color,
}

impl Visuals {
    fn accent_hovered(accent: Color) -> Color {
        if accent == AccentColor::Blue.color() {
            return Color::rgb(0.30, 0.52, 0.92);
        }
        mix_color(accent, Color::white(), 0.18)
    }

    fn accent_pressed(accent: Color) -> Color {
        if accent == AccentColor::Blue.color() {
            return Color::rgb(0.16, 0.36, 0.72);
        }
        mix_color(accent, Color::black(), 0.18)
    }

    /// Return this palette with its primary accent replaced.
    pub fn with_accent(mut self, accent: Color) -> Self {
        let hovered = Self::accent_hovered(accent);
        let pressed = Self::accent_pressed(accent);
        let dark =
            0.299 * self.bg_color.r + 0.587 * self.bg_color.g + 0.114 * self.bg_color.b < 0.5;
        self.accent = accent;
        self.accent_hovered = hovered;
        self.accent_pressed = pressed;
        self.accent_focus = accent.with_alpha(0.45);
        self.text_link = if dark { hovered } else { pressed };
        self.text_link_hovered = if dark {
            mix_color(hovered, Color::white(), 0.12)
        } else {
            accent
        };
        self.widget_stroke_active = pressed;
        self.selection_bg = accent.with_alpha(0.45);
        self
    }

    /// Return this palette with one of the built-in accent swatches applied.
    pub fn with_accent_color(self, accent: AccentColor) -> Self {
        self.with_accent(accent.color())
    }

    /// Dark-mode palette matching egui's approximate dark colour scheme.
    pub fn dark() -> Self {
        let accent = Color::rgb(0.22, 0.45, 0.88);
        let accent_hovered = Color::rgb(0.30, 0.52, 0.92);
        let accent_pressed = Color::rgb(0.16, 0.36, 0.72);
        Self {
            // Chrome
            bg_color: Color::rgb(0.10, 0.10, 0.12),
            panel_fill: Color::rgb(0.13, 0.13, 0.15),
            top_bar_bg: Color::rgb(0.15, 0.15, 0.17),
            // Window
            window_fill: Color::rgb(0.15, 0.15, 0.18),
            window_title_fill: Color::rgb(0.20, 0.20, 0.24),
            window_title_fill_drag: Color::rgb(0.16, 0.16, 0.20),
            window_shadow: Color::rgba(0.0, 0.0, 0.0, 0.35),
            window_stroke: Color::rgba(1.0, 1.0, 1.0, 0.08),
            window_title_text: Color::rgba(1.0, 1.0, 1.0, 0.90),
            window_close_bg: Color::rgba(1.0, 1.0, 1.0, 0.12),
            window_close_bg_hovered: Color::rgba(1.0, 1.0, 1.0, 0.25),
            window_close_fg: Color::rgba(1.0, 1.0, 1.0, 0.80),
            window_resize_hover: Color::rgba(1.0, 1.0, 1.0, 0.40),
            window_resize_active: Color::rgba(1.0, 1.0, 1.0, 0.80),
            // Text
            text_color: Color::rgb(0.90, 0.90, 0.92),
            text_dim: Color::rgba(0.90, 0.90, 0.92, 0.50),
            text_link: Color::rgb(0.45, 0.65, 1.00),
            text_link_hovered: Color::rgb(0.35, 0.55, 0.90),
            // Accent
            accent,
            accent_hovered,
            accent_pressed,
            accent_focus: Color::rgba(0.22, 0.45, 0.88, 0.45),
            // Widgets
            widget_bg: Color::rgb(0.22, 0.22, 0.26),
            widget_bg_hovered: Color::rgb(0.28, 0.28, 0.33),
            widget_stroke: Color::rgba(0.60, 0.60, 0.65, 0.60),
            widget_stroke_active: accent_pressed,
            // Track
            track_bg: Color::rgb(0.25, 0.25, 0.28),
            // Scrollbar
            scroll_track: Color::rgba(1.0, 1.0, 1.0, 0.04),
            scroll_thumb: Color::rgba(1.0, 1.0, 1.0, 0.18),
            scroll_thumb_hovered: Color::rgba(1.0, 1.0, 1.0, 0.32),
            scroll_thumb_dragging: Color::rgba(1.0, 1.0, 1.0, 0.45),
            // Separator
            separator: Color::rgba(1.0, 1.0, 1.0, 0.10),
            // Selection
            selection_bg: Color::rgba(0.22, 0.45, 0.88, 0.45),
            selection_bg_unfocused: Color::rgba(0.60, 0.60, 0.65, 0.35),
        }
    }

    /// Light-mode palette matching egui's approximate light colour scheme.
    pub fn light() -> Self {
        let accent = Color::rgb(0.22, 0.45, 0.88);
        let accent_hovered = Color::rgb(0.30, 0.52, 0.92);
        let accent_pressed = Color::rgb(0.16, 0.36, 0.72);
        Self {
            // Chrome
            bg_color: Color::rgb(0.90, 0.90, 0.92),
            panel_fill: Color::rgb(0.92, 0.92, 0.95),
            top_bar_bg: Color::rgb(0.88, 0.88, 0.91),
            // Window
            window_fill: Color::rgb(0.97, 0.97, 0.98),
            window_title_fill: Color::rgb(0.87, 0.87, 0.91),
            window_title_fill_drag: Color::rgb(0.80, 0.80, 0.85),
            window_shadow: Color::rgba(0.0, 0.0, 0.0, 0.18),
            window_stroke: Color::rgba(0.0, 0.0, 0.0, 0.15),
            window_title_text: Color::rgba(0.05, 0.05, 0.10, 0.90),
            window_close_bg: Color::rgba(0.0, 0.0, 0.0, 0.08),
            window_close_bg_hovered: Color::rgba(0.0, 0.0, 0.0, 0.18),
            window_close_fg: Color::rgba(0.0, 0.0, 0.0, 0.65),
            window_resize_hover: Color::rgba(0.0, 0.0, 0.0, 0.30),
            window_resize_active: Color::rgba(0.0, 0.0, 0.0, 0.65),
            // Text
            text_color: Color::rgb(0.08, 0.08, 0.10),
            text_dim: Color::rgba(0.08, 0.08, 0.10, 0.50),
            text_link: Color::rgb(0.15, 0.35, 0.75),
            text_link_hovered: Color::rgb(0.10, 0.28, 0.62),
            // Accent
            accent,
            accent_hovered,
            accent_pressed,
            accent_focus: Color::rgba(0.22, 0.45, 0.88, 0.45),
            // Widgets
            widget_bg: Color::rgb(1.00, 1.00, 1.00),
            widget_bg_hovered: Color::rgb(0.92, 0.93, 0.95),
            widget_stroke: Color::rgb(0.75, 0.76, 0.78),
            widget_stroke_active: accent_pressed,
            // Track
            track_bg: Color::rgb(0.85, 0.86, 0.88),
            // Scrollbar
            scroll_track: Color::rgba(0.0, 0.0, 0.0, 0.04),
            scroll_thumb: Color::rgba(0.0, 0.0, 0.0, 0.18),
            scroll_thumb_hovered: Color::rgba(0.0, 0.0, 0.0, 0.32),
            scroll_thumb_dragging: Color::rgba(0.0, 0.0, 0.0, 0.45),
            // Separator
            separator: Color::rgba(0.0, 0.0, 0.0, 0.12),
            // Selection
            selection_bg: Color::rgba(0.22, 0.45, 0.88, 0.45),
            selection_bg_unfocused: Color::rgba(0.45, 0.45, 0.50, 0.35),
        }
    }

    /// Choose a palette from a [`ThemePreference`].  `System` falls back to dark.
    pub fn for_preference(pref: ThemePreference) -> Self {
        match pref {
            ThemePreference::Light => Self::light(),
            _ => Self::dark(),
        }
    }
}

fn mix_color(a: Color, b: Color, t: f32) -> Color {
    let u = t.clamp(0.0, 1.0);
    Color::rgba(
        a.r + (b.r - a.r) * u,
        a.g + (b.g - a.g) * u,
        a.b + (b.b - a.b) * u,
        a.a + (b.a - a.a) * u,
    )
}

// ---------------------------------------------------------------------------
// Thread-local active visuals
// ---------------------------------------------------------------------------

thread_local! {
    static VISUALS: RefCell<Visuals> = RefCell::new(Visuals::dark());
}

/// Monotonic counter bumped every time `set_visuals` installs a new palette.
///
/// Backbuffered widgets (e.g. `Label`) compare this against the epoch they
/// last rasterised at and self-invalidate on mismatch — without this, a
/// `Label` whose color follows `visuals.text_color` would keep blitting the
/// bitmap it baked in the old palette after a dark/light flip, leaving
/// stale-coloured text until some other mutation invalidated the cache.
static VISUALS_EPOCH: AtomicU64 = AtomicU64::new(1);

/// Current visuals epoch.  See [`VISUALS_EPOCH`] docstring for how the
/// widget layer uses it.
pub fn current_visuals_epoch() -> u64 {
    VISUALS_EPOCH.load(Ordering::Relaxed)
}

/// Replace the active [`Visuals`].
///
/// Call this once per frame *before* painting, typically from the platform
/// render loop after reading the user's `ThemePreference`.
pub fn set_visuals(v: Visuals) {
    VISUALS.with(|cell| *cell.borrow_mut() = v);
    VISUALS_EPOCH.fetch_add(1, Ordering::Relaxed);
}

/// Clone and return the active [`Visuals`].
///
/// Widget `paint()` methods call this (via [`DrawCtx::visuals`]) to look up
/// colours at render time rather than at construction time.
pub fn current_visuals() -> Visuals {
    VISUALS.with(|cell| cell.borrow().clone())
}
