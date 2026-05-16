//! Per-widget colour overrides for [`super::TextField`].
//!
//! Split out of `text_field.rs` to keep the main file under the
//! 800-line cap. See the [`TextFieldTheme`] struct + the
//! `with_theme` builder method for the public surface.

use super::*;

/// Per-widget colour overrides for [`TextField`]. When set via
/// [`TextField::with_theme`], paint reads from these instead of
/// the ambient [`crate::draw_ctx::DrawCtx::visuals`]. Lets callers
/// theme a single field to match a dialog palette without forking
/// the global visuals.
///
/// Any field set to `None` falls back to the corresponding
/// `visuals()` colour, so themes can override just what they need
/// (e.g. background + border for a dark-panel field that still
/// wants the ambient selection / cursor highlights).
#[derive(Clone, Copy, Debug, Default)]
pub struct TextFieldTheme {
    pub background: Option<Color>,
    pub text_color: Option<Color>,
    pub placeholder_color: Option<Color>,
    pub border_color: Option<Color>,
    pub border_color_hovered: Option<Color>,
    pub border_color_focused: Option<Color>,
    pub selection_bg: Option<Color>,
    pub selection_bg_unfocused: Option<Color>,
    pub cursor_color: Option<Color>,
    pub border_radius: Option<f64>,
}

impl TextField {
    /// Install a [`TextFieldTheme`] of per-widget colour overrides.
    /// Any field set to `None` on the theme falls back to the
    /// ambient `visuals()` palette, so callers can override just
    /// what they need (e.g. background + border for a dark-panel
    /// field that keeps the default selection / cursor highlights).
    pub fn with_theme(mut self, theme: TextFieldTheme) -> Self {
        self.theme = theme;
        self
    }
}
