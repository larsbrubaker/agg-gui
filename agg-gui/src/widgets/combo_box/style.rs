//! Per-instance styling for the closed [`super::ComboBox`] button.
//!
//! By default a `ComboBox` paints its closed box from the global
//! [`crate::theme::Visuals`] (`widget_bg` / `widget_stroke`) with a fixed
//! height and corner radius.  Apps that theme individual controls (e.g. a
//! toolbar of compact, flat dropdowns) attach a [`ComboBoxStyle`] via
//! [`super::ComboBox::with_style`]; every field is optional and `None` keeps
//! the default behaviour, so an all-`None` style paints exactly like an
//! unstyled combo.
//!
//! [`ComboBoxStateStyle`] is a second, independent set of optional
//! overrides for the interactive states (hover / focus outline, the open
//! box's fill) and for the dropdown list (panel fill, item text, hovered
//! item).  It is separate from `ComboBoxStyle` so existing struct-literal
//! constructions of `ComboBoxStyle` keep compiling.
//!
//! The closed-box painter lives here (split out of `combo_box.rs` to keep
//! that file under the 800-line limit); the popup painter is in
//! `popup_paint.rs` and only consumes the resolved closed height.

use super::*;
use crate::color::Color;

/// Optional per-instance overrides for the closed [`ComboBox`] button.
///
/// Every field defaults to `None`, meaning "use the built-in behaviour":
/// `fill` → `Visuals::widget_bg`, `border` → `Visuals::widget_stroke`,
/// `hover_fill` → same as the (resolved) fill (no hover change),
/// `radius` → 4 px, `height` → 24 px.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ComboBoxStyle {
    /// Background of the closed box.
    pub fill: Option<Color>,
    /// 1 px outline of the closed box.
    pub border: Option<Color>,
    /// Background while the cursor is over the closed box.
    pub hover_fill: Option<Color>,
    /// Corner radius of the closed box (the popup keeps its own radius).
    pub radius: Option<f64>,
    /// Height of the closed box; also drives hit-testing and where the
    /// popup attaches.
    pub height: Option<f64>,
}

/// Optional per-instance overrides for a [`ComboBox`]'s interactive states
/// and its dropdown list.
///
/// Every field defaults to `None`, meaning "use the built-in behaviour":
/// `hover_border` / `focus_border` → the rest border (no change),
/// `open_fill` → the rest / hover fill, `popup_fill` → `Visuals::widget_bg`,
/// `item_text` → `Visuals::text_color`, `item_hover_fill` →
/// `Visuals::widget_bg_hovered`, `item_hover_text` → the item text colour.
/// The selected row keeps its accent highlight.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ComboBoxStateStyle {
    /// Outline while the cursor is over the closed box.
    pub hover_border: Option<Color>,
    /// Outline while the combo has keyboard focus (wins over `hover_border`).
    pub focus_border: Option<Color>,
    /// Fill of the closed box while its list is open.
    pub open_fill: Option<Color>,
    /// Background of the open dropdown list.
    pub popup_fill: Option<Color>,
    /// Text colour of unselected, unhovered items in the list.
    pub item_text: Option<Color>,
    /// Background of the hovered (unselected) item.
    pub item_hover_fill: Option<Color>,
    /// Text colour of the hovered (unselected) item.
    pub item_hover_text: Option<Color>,
}

impl ComboBox {
    /// Apply per-instance state / list styling.  See [`ComboBoxStateStyle`].
    pub fn with_state_style(mut self, style: ComboBoxStateStyle) -> Self {
        self.state_style = style;
        self
    }

    /// The current state / list style overrides.
    pub fn state_style(&self) -> ComboBoxStateStyle {
        self.state_style
    }

    /// Apply per-instance styling to the closed box.  See [`ComboBoxStyle`].
    pub fn with_style(mut self, style: ComboBoxStyle) -> Self {
        self.style = style;
        self
    }

    /// The current per-instance style overrides.
    pub fn style(&self) -> ComboBoxStyle {
        self.style
    }

    /// Resolved height of the closed box.
    pub(super) fn closed_h(&self) -> f64 {
        self.style.height.unwrap_or(CLOSED_H).max(0.0)
    }

    /// Paint the closed box: background, outline and the ▼ arrow.
    pub(super) fn paint_closed_box(&self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let w = self.bounds.width;
        let h = self.closed_h();
        let r = self.style.radius.unwrap_or(CORNER_R).max(0.0);
        let base_fill = self.style.fill.unwrap_or(v.widget_bg);
        let rest_or_hover = if self.button_hovered {
            self.style.hover_fill.unwrap_or(base_fill)
        } else {
            base_fill
        };
        let fill = if self.open {
            self.state_style.open_fill.unwrap_or(rest_or_hover)
        } else {
            rest_or_hover
        };
        let rest_border = self.style.border.unwrap_or(v.widget_stroke);
        let border = if self.focused && self.state_style.focus_border.is_some() {
            self.state_style.focus_border
        } else if self.button_hovered {
            self.state_style.hover_border
        } else {
            None
        }
        .unwrap_or(rest_border);

        ctx.set_fill_color(fill);
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, w, h, r);
        ctx.fill();

        ctx.set_stroke_color(border);
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, w, h, r);
        ctx.stroke();

        // ── Dropdown arrow (▼) ────────────────────────────────────────────
        let arrow_x = w - ARROW_W * 0.5;
        let arrow_cy = h * 0.5;
        let arrow_sz = 4.0;
        ctx.set_fill_color(v.text_dim);
        ctx.begin_path();
        // Small downward triangle.
        ctx.move_to(arrow_x - arrow_sz, arrow_cy + arrow_sz * 0.5);
        ctx.line_to(arrow_x + arrow_sz, arrow_cy + arrow_sz * 0.5);
        ctx.line_to(arrow_x, arrow_cy - arrow_sz * 0.5);
        ctx.close_path();
        ctx.fill();
    }
}
