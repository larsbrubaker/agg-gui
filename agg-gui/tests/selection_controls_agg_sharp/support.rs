//! Shared helpers for the check box, radio and toggle ports: the classic
//! (agg-sharp) control size, C#'s demo font, and C#'s `Render`: the widget
//! painted at `HOST_PAD` inside a host filled with the theme's background.
//!
//! C#'s `ThemeConfig` maps onto agg-gui's [`Visuals`] the way agg-sharp's
//! `SelectionControlStyle` derives it: `BackgroundColor` is `window_fill`,
//! `ButtonBackgroundColor` is `widget_bg`, `PrimaryAccentColor` is `accent`;
//! C#'s demo Dark and Light themes are `Visuals::dark()` and
//! `Visuals::light()`. The visuals and the control size are thread local in
//! agg-gui and every test runs on its own thread, so C#'s `[NotInParallel]`
//! needs no lock.

use std::sync::Arc;

use agg_gui::font_settings::{set_line_box, LineBox};
use agg_gui::text::Font;
use agg_gui::theme::{set_visuals, Visuals};
use agg_gui::widget::paint_subtree;
use agg_gui::{
    set_selection_control_size, Color, Framebuffer, GfxCtx, Rect, SelectionControlSize, Size,
    Widget,
};

pub const HOST_PAD: u32 = 4;

static LIBERATION_SANS: &[u8] = include_bytes!("../../../demo/assets/LiberationSans-Regular.ttf");

/// agg-sharp's default face.
pub fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(LIBERATION_SANS).expect("demo font"))
}

/// C#'s 12 point label, in pixels at device scale 1.
pub const TWELVE_POINT: f64 = 12.0 * 4.0 / 3.0;

/// MatterCAD's check boxes and radios: the classic size, with C#'s
/// `TextWidget` line box (one em, so a 12 point label is 16 high). Call
/// before building the controls.
pub fn use_classic_size() {
    set_selection_control_size(SelectionControlSize::Classic);
    set_line_box(LineBox::Em);
}

/// Lay `widget` out at its natural size at the origin.
pub fn layout(widget: &mut dyn Widget) -> Size {
    let s = widget.layout(Size::new(1000.0, 1000.0));
    widget.set_bounds(Rect::new(0.0, 0.0, s.width, s.height));
    s
}

/// C#'s `Render`: installs `theme` and paints the laid-out `widget` at
/// `HOST_PAD` inside a host cleared to the theme's background.
pub fn render(widget: &mut dyn Widget, theme: &Visuals) -> Framebuffer {
    set_visuals(theme.clone());
    let b = widget.bounds();
    let w = (b.width + f64::from(HOST_PAD) * 2.0).ceil() as u32;
    let h = (b.height + f64::from(HOST_PAD) * 2.0).ceil() as u32;
    let mut fb = Framebuffer::new(w, h);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(theme.window_fill);
        ctx.translate(f64::from(HOST_PAD), f64::from(HOST_PAD));
        paint_subtree(widget, &mut ctx);
    }
    fb
}

/// RGB of pixel (`x`, `y`), Y-up as C#'s `ImageBuffer`.
pub fn pixel(fb: &Framebuffer, x: u32, y: u32) -> [u8; 3] {
    let i = ((y * fb.width() + x) * 4) as usize;
    let p = fb.pixels();
    [p[i], p[i + 1], p[i + 2]]
}

/// A colour as the software renderer stores it.
pub fn rgb8(c: Color) -> [u8; 3] {
    let c = c.to_rgba8();
    [c.r, c.g, c.b]
}

/// C#'s `Near`: every channel within `tolerance`.
pub fn near(a: [u8; 3], b: [u8; 3], tolerance: i32) -> bool {
    (0..3).all(|i| (i32::from(a[i]) - i32::from(b[i])).abs() <= tolerance)
}

/// C#'s `IsWhite`.
pub fn is_white(c: [u8; 3]) -> bool {
    c.iter().all(|&v| v > 225)
}
