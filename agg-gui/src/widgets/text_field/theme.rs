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
///
/// The state colours follow agg-sharp's `ThemeConfig.EditFieldColors`
/// (`Inactive`, `Hovered`, `Focused`), as `ThemedTextEditWidget` reads them:
/// focus wins over hover. An unset hovered or focused colour falls back to
/// the field's normal colour (`background`, `text_color`), then to the
/// visuals, so a theme that sets none of them looks as before. The border's
/// unset hovered and focused colours keep their own visuals fallbacks
/// (`widget_stroke_active`, `accent`).
///
/// [`TextArea::with_theme`](crate::widgets::TextArea::with_theme) takes the
/// same theme, so single- and multi-line editors share one palette.
#[derive(Clone, Copy, Debug, Default)]
pub struct TextFieldTheme {
    pub background: Option<Color>,
    /// Background while the pointer is over the unfocused field.
    pub background_hovered: Option<Color>,
    /// Background while the field has keyboard focus.
    pub background_focused: Option<Color>,
    pub text_color: Option<Color>,
    /// Text colour while the pointer is over the unfocused field.
    pub text_color_hovered: Option<Color>,
    /// Text colour while the field has keyboard focus (agg-sharp's
    /// `EditFieldColors.Focused.TextColor`).
    pub text_color_focused: Option<Color>,
    pub placeholder_color: Option<Color>,
    /// Hint colour while the field has keyboard focus (agg-sharp's
    /// `EditFieldColors.Focused.LightTextColor`). Unset falls back to
    /// `placeholder_color`, then the visuals. Seen only where the hint shows
    /// while focused: a [`TextField`] with
    /// [`show_placeholder_when_focused`](Self::show_placeholder_when_focused),
    /// or a [`TextArea`](crate::widgets::TextArea), whose hint always shows.
    pub placeholder_color_focused: Option<Color>,
    /// Keep a [`TextField`]'s hint visible while it has focus and is empty,
    /// as agg-sharp's `ThemedTextEditWidget` does. `false` (the default)
    /// hides it on focus. A `TextArea` always shows its hint.
    pub show_placeholder_when_focused: bool,
    pub border_color: Option<Color>,
    pub border_color_hovered: Option<Color>,
    pub border_color_focused: Option<Color>,
    pub selection_bg: Option<Color>,
    pub selection_bg_unfocused: Option<Color>,
    pub cursor_color: Option<Color>,
    pub border_radius: Option<f64>,
    /// Border width while focused. Unset keeps the 2 px focus ring; agg-sharp's
    /// edit fields keep a 1 px border in every state (`Some(1.0)`). The border
    /// is 1 px in the other states.
    pub border_width_focused: Option<f64>,
}

impl TextFieldTheme {
    /// The background for the given state: focus over hover over normal,
    /// each unset state falling back to the normal colour, then `normal`.
    pub fn background_for(&self, focused: bool, hovered: bool, normal: Color) -> Color {
        let state = if focused {
            self.background_focused
        } else if hovered {
            self.background_hovered
        } else {
            None
        };
        state.or(self.background).unwrap_or(normal)
    }

    /// The text colour for the given state, resolved as
    /// [`background_for`](Self::background_for) does.
    pub fn text_color_for(&self, focused: bool, hovered: bool, normal: Color) -> Color {
        let state = if focused {
            self.text_color_focused
        } else if hovered {
            self.text_color_hovered
        } else {
            None
        };
        state.or(self.text_color).unwrap_or(normal)
    }

    /// The hint colour: `placeholder_color_focused` while focused, else (or
    /// when unset) `placeholder_color`, then `normal`.
    pub fn placeholder_color_for(&self, focused: bool, normal: Color) -> Color {
        let state = if focused {
            self.placeholder_color_focused
        } else {
            None
        };
        state.or(self.placeholder_color).unwrap_or(normal)
    }

    /// The border colour for the given state: focus over hover over normal,
    /// each unset colour falling back to its own visuals colour (`focused`,
    /// `hovered`, `normal`).
    pub fn border_color_for(
        &self,
        focused: bool,
        hovered: bool,
        normal: Color,
        hovered_fallback: Color,
        focused_fallback: Color,
    ) -> Color {
        if focused {
            self.border_color_focused.unwrap_or(focused_fallback)
        } else if hovered {
            self.border_color_hovered.unwrap_or(hovered_fallback)
        } else {
            self.border_color.unwrap_or(normal)
        }
    }

    /// The border width: [`border_width_focused`](Self::border_width_focused)
    /// (default 2) while focused, 1 otherwise.
    pub fn border_width_for(&self, focused: bool) -> f64 {
        if focused {
            self.border_width_focused.unwrap_or(2.0)
        } else {
            1.0
        }
    }
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

    /// Replace the colour overrides after construction (a live theme
    /// switch). The next layout re-rasters.
    pub fn set_theme(&mut self, theme: TextFieldTheme) {
        self.theme = theme;
        self.last_sig = None;
        crate::animation::request_draw();
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
