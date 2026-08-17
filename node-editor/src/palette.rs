//! Canvas theme palette — light/dark colours used by the node-editor
//! canvas paint path.
//!
//! Split out of [`crate::draw`] so that module stays under the 800-line
//! cap. The palette is the only "theme" surface the canvas widget
//! cares about: every paint helper in `draw.rs` and `widget/*` reads
//! from this struct.

use agg_gui::Color;

/// Theme palette used by the canvas. Built from agg-gui's current
/// visuals so light / dark mode toggles flow through automatically.
/// Hosts that want different colours can construct one manually and
/// pass it via [`crate::NodeEditor::set_palette`].
pub struct CanvasPalette {
    pub canvas_bg: Color,
    pub canvas_grid: Color,
    pub node_body: Color,
    pub node_body_selected: Color,
    pub node_border: Color,
    /// Border colour for the currently-selected node — pulled from
    /// `Visuals::accent` so the selection reads clearly against any
    /// theme background.
    pub node_border_selected: Color,
    pub node_title_fallback: Color,
    pub label_text: Color,
    /// Border + badge colour for a node whose host reported an
    /// evaluation error (see [`crate::draw_error`]). Deliberately not
    /// theme-accent-derived: "broken" should not change meaning when the
    /// user picks a red accent.
    pub node_error: Color,
    /// Border + badge colour for a node whose output is *degraded* but
    /// present (see [`crate::model::BadgeSeverity::Warning`]). Amber, and
    /// deliberately not theme-accent-derived for the same reason as
    /// [`node_error`](Self::node_error).
    pub node_warning: Color,
}

impl CanvasPalette {
    /// The badge / outline colour for a given severity.
    pub fn badge_color(&self, severity: crate::model::BadgeSeverity) -> Color {
        match severity {
            crate::model::BadgeSeverity::Error => self.node_error,
            crate::model::BadgeSeverity::Warning => self.node_warning,
        }
    }
}

impl CanvasPalette {
    /// Build the palette from agg-gui's current visuals — adapts to
    /// light or dark mode automatically.
    pub fn from_visuals(v: &agg_gui::theme::Visuals) -> Self {
        let dark = 0.299 * v.bg_color.r + 0.587 * v.bg_color.g + 0.114 * v.bg_color.b < 0.5;
        let canvas_bg = if dark {
            Color::rgb(0.13, 0.14, 0.16)
        } else {
            Color::rgb(0.96, 0.96, 0.97)
        };
        let grid_alpha = if dark { 0.06 } else { 0.30 };
        let canvas_grid = if dark {
            Color::rgba(1.0, 1.0, 1.0, grid_alpha)
        } else {
            Color::rgba(0.0, 0.0, 0.0, grid_alpha * 0.3)
        };
        let node_body = if dark {
            Color::rgb(0.22, 0.23, 0.27)
        } else {
            Color::rgb(0.99, 0.99, 0.99)
        };
        let node_body_selected = if dark {
            Color::rgb(0.28, 0.32, 0.38)
        } else {
            Color::rgb(0.92, 0.94, 1.0)
        };
        let node_border = if dark {
            Color::rgba(0.0, 0.0, 0.0, 0.5)
        } else {
            Color::rgba(0.0, 0.0, 0.0, 0.18)
        };
        Self {
            canvas_bg,
            canvas_grid,
            node_body,
            node_body_selected,
            node_border,
            node_border_selected: v.accent,
            node_title_fallback: v.accent,
            label_text: v.text_color,
            // Lifted a little in dark mode so it doesn't muddy against
            // the dark node body.
            node_error: if dark {
                Color::rgb(0.95, 0.34, 0.30)
            } else {
                Color::rgb(0.82, 0.15, 0.13)
            },
            // Amber, darkened in light mode so the white `!` inside the
            // badge keeps its contrast against a pale background.
            node_warning: if dark {
                Color::rgb(0.98, 0.72, 0.20)
            } else {
                Color::rgb(0.79, 0.52, 0.03)
            },
        }
    }

    /// Backwards-compat shim used by simple call sites.
    pub fn dark() -> Self {
        Self::from_visuals(&agg_gui::theme::Visuals::dark())
    }
}
