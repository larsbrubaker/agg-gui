//! Per-widget colour overrides and the frame switch for [`super::TextField`].
//!
//! Split out of `text_field.rs` to keep the main file under the
//! 800-line cap. See the [`TextFieldTheme`] struct + the
//! `with_theme` builder method for the public surface, and `with_frame` for
//! a field without its own background and border (as `TextArea::with_frame`).

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

    /// Paint the background and border (`true`, the default) or leave both to
    /// the host, so its own frame is the field (agg-sharp's `Border = 0` with a
    /// transparent background). A frameless field draws no focus ring (the
    /// focused border); the host shows focus. The text insets are unchanged.
    pub fn with_frame(mut self, frame: bool) -> Self {
        self.frame = frame;
        self
    }

    /// Switch the frame on or off after construction (see
    /// [`with_frame`](Self::with_frame)). The next layout re-rasters.
    pub fn set_frame(&mut self, frame: bool) {
        if self.frame != frame {
            self.frame = frame;
            crate::animation::request_draw();
        }
    }

    /// Whether the background and border are painted.
    pub fn has_frame(&self) -> bool {
        self.frame
    }
}
