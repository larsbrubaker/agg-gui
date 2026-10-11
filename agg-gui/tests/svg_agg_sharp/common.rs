//! Shared helpers for the ported agg-sharp SVG tests: `render` wraps a body in
//! the 100x100 viewBox document the C# classes use and renders it at 100x100
//! (one user unit per pixel), and `Image::at` reads a straight-alpha pixel at
//! SVG's y-down coordinates, as C#'s `image.GetPixel(x, image.Height - 1 - y)`
//! does on agg-sharp's Y-up, straight-alpha `ImageBuffer`.

use agg_gui::framebuffer::unpremultiply_rgba_inplace;
use agg_gui::render_svg_to_framebuffer_at_size;

/// A rendered image in straight (un-premultiplied) RGBA, rows Y-up.
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// An RGBA pixel, as agg-sharp's `Color` (red, green, blue, alpha).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Color {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
    pub alpha: u8,
}

impl Color {
    pub const fn new(red: u8, green: u8, blue: u8, alpha: u8) -> Self {
        Self {
            red,
            green,
            blue,
            alpha,
        }
    }
}

impl Image {
    /// C#'s `GetPixel(x, y)`: the pixel at Y-up row `y`.
    pub fn get_pixel(&self, x: u32, y: u32) -> Color {
        let i = ((y * self.width + x) * 4) as usize;
        let p = &self.pixels;
        Color::new(p[i], p[i + 1], p[i + 2], p[i + 3])
    }

    /// The pixel at SVG coordinates (x, y), y down from the top.
    pub fn at(&self, x: u32, y: u32) -> Color {
        self.get_pixel(x, self.height - 1 - y)
    }

    /// The alpha at SVG coordinates (x, y).
    pub fn alpha_at(&self, x: u32, y: u32) -> i32 {
        self.at(x, y).alpha as i32
    }
}

/// Render a whole SVG document at `width` x `height` pixels.
pub fn render_document(svg: &str, width: u32, height: u32) -> Image {
    let fb =
        render_svg_to_framebuffer_at_size(svg.as_bytes(), width, height).expect("the SVG renders");
    let mut pixels = fb.pixels().to_vec();
    unpremultiply_rgba_inplace(&mut pixels);
    Image {
        width: fb.width(),
        height: fb.height(),
        pixels,
    }
}

/// `body` inside the C# classes' 100x100 viewBox document (with xlink), at 100x100.
pub fn render(body: &str) -> Image {
    render_document(
        &format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" viewBox=\"0 0 100 100\">{body}</svg>"
        ),
        100,
        100,
    )
}

/// Assert `actual` is within `tolerance` of `expected` (TUnit's `IsEqualTo(..).Within(..)`).
#[track_caller]
pub fn assert_within(actual: i32, expected: i32, tolerance: i32) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "expected {expected} within {tolerance}, got {actual}"
    );
}
