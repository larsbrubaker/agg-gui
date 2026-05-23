//! Themed painting for popup menus and menu bars.
//!
//! Text rendering is composed: the bar and popup widgets own `Label` widgets
//! for every text-bearing element (bar button text, item labels, shortcut
//! strings).  This module paints the chrome (backgrounds, hover panels,
//! separators, submenu chevrons, check/radio glyphs) inline, and the bar /
//! popup widgets paint their owned `Label` children through `paint_subtree`
//! so the framework's backbuffer + LCD subpixel path renders every glyph.

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::geometry::Rect;

use super::geometry::SEP_H;

/// Style values shared between the bar and popup painters.
///
/// Geometry + the two inline-glyph characters (check mark, radio mark).
/// Text colors are resolved per-`Label` from `ctx.visuals()` or set
/// explicitly when a row is hovered / opened.
///
/// The submenu chevron is painted as a vector stroke — independent of
/// the host's font, so it always renders regardless of whether the
/// host bundles Font Awesome, Bootstrap Icons, or no icon font at all.
///
/// The remaining glyph fields default to portable Unicode characters
/// present in every general-purpose font.  Hosts that bundle Font
/// Awesome can swap them for the matching FA glyphs (`\u{F00C}` check,
/// `\u{F111}` circle) so the indicators match the rest of the UI.
#[derive(Clone)]
pub struct MenuStyle {
    pub radius: f64,
    pub shadow_offset: (f64, f64),
    pub shadow_alpha: f32,
    pub pad_x: f64,
    pub icon_x: f64,
    pub label_x: f64,
    pub shortcut_right: f64,
    /// Glyph painted in the icon slot of a checked
    /// (`MenuSelection::Check { selected: true }`) row that has no
    /// explicit icon.  Default: U+2713 CHECK MARK.
    pub check_glyph: char,
    /// Glyph painted in the icon slot of a selected
    /// (`MenuSelection::Radio { selected: true }`) row that has no
    /// explicit icon.  Default: U+25CF BLACK CIRCLE.
    pub radio_glyph: char,
}

impl Default for MenuStyle {
    fn default() -> Self {
        Self {
            radius: 5.0,
            shadow_offset: (5.0, -5.0),
            shadow_alpha: 0.22,
            pad_x: 8.0,
            icon_x: 14.0,
            label_x: 32.0,
            shortcut_right: 28.0,
            check_glyph: '\u{2713}',
            radio_glyph: '\u{25CF}',
        }
    }
}

/// Three points (apex on the right, top-left, bottom-left) of the
/// submenu-indicator chevron painted at the right edge of a popup row.
///
/// Returned as a pure function so paint tests can verify the geometry
/// without driving a `DrawCtx`. Coordinates are in the same Y-up space
/// the row uses; the apex points to the right (toward where the
/// submenu opens).
pub fn submenu_chevron_points(row: Rect) -> [(f64, f64); 3] {
    let cx = row.x + row.width - 11.0;
    let cy = row.y + row.height * 0.5;
    let half_h = 4.0;
    let arm = 3.0;
    [
        (cx - arm, cy + half_h),
        (cx, cy),
        (cx - arm, cy - half_h),
    ]
}

/// Paint the submenu-indicator chevron as a stroked `>` polyline.
/// Font-independent — the previous implementation rendered a glyph
/// from the popup's text font, which left a tofu box on hosts whose
/// font (or icon fallback) lacked the configured code point.
pub fn paint_submenu_chevron(ctx: &mut dyn DrawCtx, row: Rect, color: Color) {
    let [a, b, c] = submenu_chevron_points(row);
    ctx.set_stroke_color(color);
    ctx.set_line_width(1.4);
    ctx.begin_path();
    ctx.move_to(a.0, a.1);
    ctx.line_to(b.0, b.1);
    ctx.line_to(c.0, c.1);
    ctx.stroke();
}

/// Paint the chrome (hover / open background fill) under a bar button.
/// The button's text label is painted separately by the caller via
/// `paint_subtree` on the bar's owned `Label`.
pub fn paint_menu_bar_button_bg(
    ctx: &mut dyn DrawCtx,
    rect: Rect,
    open: bool,
    hovered: bool,
) {
    if !open && !hovered {
        return;
    }
    let v = ctx.visuals();
    // Subtle desktop hover: a translucent accent tint under the label,
    // not the full accent.  Translucent because the underlying
    // `top_bar_bg` is already a very light gray (≈0.88 in the light
    // theme); a fully-opaque `widget_bg_hovered` panel reads as nothing
    // — `widget_bg_hovered` is only ~0.04 brighter than the bar.
    // Translucent accent stays visible on either theme while reserving
    // the FULL accent for the OPENED state.
    let bg = if open {
        v.accent
    } else {
        v.accent.with_alpha(0.18)
    };
    ctx.set_fill_color(bg);
    ctx.begin_path();
    ctx.rounded_rect(
        rect.x + 1.0,
        rect.y + 2.0,
        rect.width - 2.0,
        rect.height - 4.0,
        4.0,
    );
    ctx.fill();
}

/// The text colour the bar button's `Label` should use for the given
/// open / enabled state.  Returns `Color::white()` when the button is
/// open (white text on the accent fill), the theme's `text_color`
/// otherwise.
pub fn bar_button_text_color(ctx: &dyn DrawCtx, open: bool) -> Color {
    if open {
        Color::white()
    } else {
        ctx.visuals().text_color
    }
}

/// The text colour the popup row's `Label` should use for the given
/// open / hovered / enabled state.  Mirrors the historical
/// `paint_item_row` logic but exposed so the popup widget can push the
/// resolved colour into its owned `Label`s.
pub fn popup_row_text_color(ctx: &dyn DrawCtx, enabled: bool, open: bool) -> Color {
    let v = ctx.visuals();
    if !enabled {
        v.text_color.with_alpha(0.45)
    } else if open {
        Color::white()
    } else {
        v.text_color
    }
}

pub fn paint_panel(ctx: &mut dyn DrawCtx, rect: Rect, style: &MenuStyle) {
    let v = ctx.visuals();
    ctx.set_fill_color(Color::black().with_alpha(style.shadow_alpha));
    ctx.begin_path();
    ctx.rounded_rect(
        rect.x + style.shadow_offset.0,
        rect.y + style.shadow_offset.1,
        rect.width,
        rect.height,
        style.radius,
    );
    ctx.fill();

    ctx.set_fill_color(v.panel_fill);
    ctx.begin_path();
    ctx.rounded_rect(rect.x, rect.y, rect.width, rect.height, style.radius);
    ctx.fill();
    ctx.set_stroke_color(v.widget_stroke);
    ctx.set_line_width(1.0);
    ctx.begin_path();
    ctx.rounded_rect(rect.x, rect.y, rect.width, rect.height, style.radius);
    ctx.stroke();
}

pub fn paint_separator(ctx: &mut dyn DrawCtx, rect: Rect) {
    let v = ctx.visuals();
    ctx.set_stroke_color(v.widget_stroke.with_alpha(0.55));
    ctx.set_line_width(1.0);
    ctx.begin_path();
    ctx.move_to(rect.x + 8.0, rect.y + SEP_H * 0.5);
    ctx.line_to(rect.x + rect.width - 8.0, rect.y + SEP_H * 0.5);
    ctx.stroke();
}

/// Paint the hover / open background of a popup item row.  The row's
/// label text is painted separately via `paint_subtree` on the
/// popup-owned `Label`; this only fills the rounded backdrop.
pub fn paint_item_row_bg(
    ctx: &mut dyn DrawCtx,
    rect: Rect,
    hovered: bool,
    open: bool,
    enabled: bool,
) {
    let hovered = hovered && enabled;
    let open = open && enabled;
    if !hovered && !open {
        return;
    }
    // Same subtle/strong split as the bar button: hover hints with a
    // translucent accent tint; open commits with full accent.
    let v = ctx.visuals();
    let bg = if open {
        v.accent
    } else {
        v.accent.with_alpha(0.18)
    };
    ctx.set_fill_color(bg);
    ctx.begin_path();
    ctx.rounded_rect(
        rect.x + 3.0,
        rect.y + 2.0,
        rect.width - 6.0,
        rect.height - 4.0,
        3.0,
    );
    ctx.fill();
}
