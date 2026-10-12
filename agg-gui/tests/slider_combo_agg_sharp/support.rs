//! Shared helpers for the slider / drop-down look ports: the C# tests'
//! accent theme (`Theme(dark)`), painting a widget at an offset on a cleared
//! framebuffer (C#'s `Render` of a root `GuiWidget` holding it), and C#'s
//! `IsNear` / `Luma` colour checks. Framebuffer rows are Y-up, row 0 at the
//! bottom, as C#'s `ImageBuffer`.

use std::sync::Arc;

use agg_gui::text::Font;
use agg_gui::widget::paint_subtree;
use agg_gui::{set_visuals, Color, Framebuffer, GfxCtx, Visuals, Widget};

static REGULAR: &[u8] = include_bytes!("../../../demo/assets/Nunito_Regular.ttf");

pub fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(REGULAR).expect("demo font"))
}

/// C#'s `Accent`, `new Color(30, 120, 230)`.
pub fn accent() -> Color {
    Color::rgb(30.0 / 255.0, 120.0 / 255.0, 230.0 / 255.0)
}

/// C#'s `Theme(dark)`: the built-in light or dark palette with the test
/// accent, installed for this thread's painting.
pub fn install_theme(dark: bool) -> Visuals {
    let base = if dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    let v = base.with_accent(accent());
    set_visuals(v.clone());
    v
}

/// Paint `widget` (laid out, bounds at its own origin) at (`x`, `y`) in a
/// `w` x `h` framebuffer cleared to `background`.
pub fn render_at(
    widget: &mut dyn Widget,
    (w, h): (u32, u32),
    (x, y): (f64, f64),
    background: Color,
) -> Framebuffer {
    let mut fb = Framebuffer::new(w, h);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(background);
        ctx.translate(x, y);
        paint_subtree(widget, &mut ctx);
    }
    fb
}

/// RGB of pixel (`x`, `y`), Y-up.
pub fn pixel(fb: &Framebuffer, x: i32, y: i32) -> [i32; 3] {
    let i = ((y as u32 * fb.width() + x as u32) * 4) as usize;
    let p = fb.pixels();
    [p[i] as i32, p[i + 1] as i32, p[i + 2] as i32]
}

/// A colour as C#'s 0-255 channels.
pub fn rgb(c: Color) -> [i32; 3] {
    [
        (c.r * 255.0).round() as i32,
        (c.g * 255.0).round() as i32,
        (c.b * 255.0).round() as i32,
    ]
}

/// C#'s `IsNear`: the summed channel difference is at most `tolerance`.
pub fn is_near(a: [i32; 3], b: [i32; 3], tolerance: i32) -> bool {
    (a[0] - b[0]).abs() + (a[1] - b[1]).abs() + (a[2] - b[2]).abs() <= tolerance
}

/// C#'s `Luma`.
pub fn luma(c: [i32; 3]) -> i32 {
    c[0] * 3 + c[1] * 6 + c[2]
}
