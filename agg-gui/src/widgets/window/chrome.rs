//! Reusable paint helpers for window-style chrome — shadow halo,
//! rounded body fill, title-bar fill, collapse chevron, outer border.
//!
//! Why this module exists: the [`Window`](super::Window) widget owns a
//! lot of behaviour (drag, resize, maximize, close, snap, backbuffer)
//! that other "framed" UI elements don't want. AtomArtist's node
//! editor — and any other consumer that wants the same drop-shadow +
//! rounded-corner look — can call these stateless paint functions
//! directly without inheriting the rest of Window.
//!
//! All coords are local to the framed region (origin at bottom-left,
//! Y-up — agg-gui convention). The caller is responsible for
//! translating the `DrawCtx` into the frame's local space before
//! invoking these helpers.

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::theme::Visuals;

/// Geometry + colour bundle for a window-style frame. Construct one
/// per paint pass; default presets come from [`ChromeStyle::from_visuals`]
/// (matches the live `Window` widget exactly).
#[derive(Clone, Debug)]
pub struct ChromeStyle {
    pub corner_radius: f64,
    pub title_height: f64,

    pub shadow_blur: f64,
    pub shadow_dx: f64,
    pub shadow_dy: f64,
    pub shadow_steps: usize,
    pub shadow_color: Color,

    pub body_color: Color,
    pub border_color: Color,
    pub title_color: Color,
    pub title_text_color: Color,
    pub separator_color: Color,
}

impl ChromeStyle {
    /// Mirrors the constants `Window` paints with, so a refactored
    /// `Window` and a fresh consumer (NodeWidget) produce identical
    /// pixels for the same visuals snapshot.
    pub fn from_visuals(v: &Visuals) -> Self {
        Self {
            corner_radius: 8.0,
            title_height: 28.0,
            shadow_blur: 14.0,
            shadow_dx: 2.0,
            shadow_dy: 6.0,
            shadow_steps: 10,
            shadow_color: v.window_shadow,
            body_color: v.window_fill,
            border_color: v.window_stroke,
            title_color: v.window_title_fill,
            title_text_color: v.window_title_text,
            separator_color: v.window_stroke,
        }
    }
}

/// Paint the stacked-rounded-rect drop shadow that frames a window.
/// Drawn outside-in so the denser core overlays the softer halo.
pub fn paint_chrome_shadow(ctx: &mut dyn DrawCtx, w: f64, h: f64, style: &ChromeStyle) {
    let base = style.shadow_color;
    let steps = style.shadow_steps.max(1);
    for i in (0..steps).rev() {
        let t = i as f64 / steps as f64;
        let infl = t * style.shadow_blur;
        let falloff = (1.0 - t).powi(2) as f32;
        let alpha = base.a * falloff / steps as f32 * 6.0;
        ctx.set_fill_color(Color::rgba(base.r, base.g, base.b, alpha));
        ctx.begin_path();
        ctx.rounded_rect(
            style.shadow_dx - infl,
            -style.shadow_dy - infl,
            w + 2.0 * infl,
            h + 2.0 * infl,
            style.corner_radius + infl,
        );
        ctx.fill();
    }
}

/// Paint the rounded body fill. When `collapsed` is true the body
/// occupies the full `h` with all four corners rounded; otherwise the
/// title-bar strip at the top is excluded (the caller paints that
/// separately so the top corner radius reads as one shape, no
/// overlapping fills).
pub fn paint_chrome_body(
    ctx: &mut dyn DrawCtx,
    w: f64,
    h: f64,
    style: &ChromeStyle,
    collapsed: bool,
) {
    if collapsed {
        return;
    }
    let content_h = (h - style.title_height).max(0.0);
    if content_h <= 0.0 {
        return;
    }
    let r = style.corner_radius;
    ctx.set_fill_color(style.body_color);
    ctx.begin_path();
    ctx.rounded_rect(0.0, 0.0, w, content_h, r);
    ctx.rect(0.0, (content_h - r).max(0.0), w, r.min(content_h));
    ctx.fill();
}

/// Paint the title-bar fill + 1-px bottom separator + title label.
/// Does **not** paint the chevron or any buttons — those are real
/// child widgets ([`crate::widgets::ChevronWidget`] et al.) the caller
/// composes into the title bar's child list. This keeps interaction
/// (click + hover + focus) flowing through the standard parent/child
/// event dispatch instead of manual coordinate hit-tests.
///
/// `bar_x`/`bar_y` are the bar's lower-left in the frame's local
/// coordinate space; the bar's width is the frame's width and its
/// height is `style.title_height`.
pub fn paint_chrome_title_bar(
    ctx: &mut dyn DrawCtx,
    bar_x: f64,
    bar_y: f64,
    w: f64,
    style: &ChromeStyle,
    collapsed: bool,
    title: &str,
    font_size: f64,
) {
    let r = style.corner_radius;
    let h = style.title_height;

    // Fill — expanded windows pair with a square bottom edge against
    // the body separator; collapsed windows carry all four corners.
    ctx.set_fill_color(style.title_color);
    ctx.begin_path();
    ctx.rounded_rect(bar_x, bar_y, w, h, r);
    if !collapsed {
        // Square the bottom edge by overpainting the lower r-strip.
        ctx.rect(bar_x, bar_y, w, r.min(h));
    }
    ctx.fill();

    // 1-px separator at bar's bottom edge (only when expanded).
    if !collapsed {
        ctx.set_fill_color(style.separator_color);
        ctx.begin_path();
        ctx.rect(bar_x, bar_y, w, 1.0);
        ctx.fill();
    }

    // Title label. Inset to clear the chevron child slot on the left
    // (chevron occupies ~24 px when composed by the caller).
    if !title.is_empty() {
        ctx.set_fill_color(style.title_text_color);
        ctx.set_font_size(font_size);
        ctx.fill_text(title, bar_x + 24.0, bar_y + h * 0.5 - 4.0);
    }
}

/// Stroked outer border that frames body + title together.
pub fn paint_chrome_border(ctx: &mut dyn DrawCtx, w: f64, h: f64, style: &ChromeStyle) {
    ctx.set_stroke_color(style.border_color);
    ctx.set_line_width(1.0);
    ctx.begin_path();
    ctx.rounded_rect(
        0.5,
        0.5,
        (w - 1.0).max(0.0),
        (h - 1.0).max(0.0),
        style.corner_radius,
    );
    ctx.stroke();
}

/// Paint the collapse / expand chevron at `(cx, cy)` in the active
/// `DrawCtx` space. Half-size 4 px. Iconography matches conventional
/// UIs: ▸ when collapsed (click to expand), ▾ when expanded (click to
/// collapse). agg-gui is Y-up, so "▾" has its apex at the LOWER y.
pub fn paint_chevron(ctx: &mut dyn DrawCtx, cx: f64, cy: f64, collapsed: bool, color: Color) {
    let sz = 4.0;
    ctx.set_stroke_color(color);
    ctx.set_line_width(1.5);
    ctx.begin_path();
    if collapsed {
        // ▸ pointing right.
        ctx.move_to(cx, cy - sz);
        ctx.line_to(cx + sz, cy);
        ctx.line_to(cx, cy + sz);
    } else {
        // ▾ pointing down — apex at the lower y in Y-up coords.
        ctx.move_to(cx - sz, cy + sz * 0.5);
        ctx.line_to(cx, cy - sz * 0.5);
        ctx.line_to(cx + sz, cy + sz * 0.5);
    }
    ctx.stroke();
}
