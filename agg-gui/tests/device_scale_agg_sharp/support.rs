//! Shared helpers for the agg-sharp device-scale ports: running a measurement
//! at a device scale, painting a widget the way `App::paint` does (layout in
//! logical units, then a `ctx.scale(device_scale)` into a device-pixel
//! framebuffer cleared to white), and reading painted extents (Y-up, row 0 at
//! the bottom, as C#'s `ImageBuffer`).
//!
//! C# keys these tests `[NotInParallel]` because `GuiWidget.DeviceScale` is
//! process wide. agg-gui's device scale is thread local
//! (`device_scale.rs`) and every test runs on its own thread, so the tests
//! need no lock; [`at_device_scale`] still restores the previous value so a
//! single-threaded run (`--test-threads=1`) leaves no scale behind.

use std::sync::Arc;

use agg_gui::text::Font;
use agg_gui::widget::paint_subtree;
use agg_gui::{device_scale, set_device_scale, Color, Framebuffer, GfxCtx, Rect, Size, Widget};

static REGULAR: &[u8] = include_bytes!("../../../demo/assets/Nunito_Regular.ttf");

pub fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(REGULAR).expect("demo font"))
}

/// C#'s `AtDeviceScale`: runs `measure` with the device scale set, restoring
/// whatever it was before. The scale is read while widgets are built and
/// painted, so it has to be in place first.
pub fn at_device_scale<T>(scale: f64, measure: impl FnOnce() -> T) -> T {
    let saved = device_scale();
    set_device_scale(scale);
    let result = measure();
    set_device_scale(saved);
    result
}

/// Lay `root` out at `logical` and place it at the origin (C# `new
/// GuiWidget(w, h)` parent), returning the size it took.
pub fn layout_at(root: &mut dyn Widget, logical: Size) -> Size {
    let got = root.layout(logical);
    root.set_bounds(Rect::new(0.0, 0.0, got.width, got.height));
    got
}

/// A white framebuffer `logical` x the current device scale (plus C#'s two
/// spare pixels), with `paint` run through a `ctx.scale(device_scale)`
/// transform, as the App paints its tree on a HiDPI display.
pub fn paint_scaled(logical: Size, paint: impl FnOnce(&mut GfxCtx)) -> Framebuffer {
    let s = device_scale();
    let w = (logical.width * s).ceil() as u32 + 2;
    let h = (logical.height * s).ceil() as u32 + 2;
    let mut fb = Framebuffer::new(w, h);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::rgba(1.0, 1.0, 1.0, 1.0));
        ctx.scale(s, s);
        paint(&mut ctx);
    }
    fb
}

/// [`paint_scaled`] for a laid-out widget tree.
pub fn paint_widget_scaled(root: &mut dyn Widget) -> Framebuffer {
    let b = root.bounds();
    paint_scaled(Size::new(b.width, b.height), |ctx| paint_subtree(root, ctx))
}

/// RGBA of pixel (`x`, `y`), Y-up.
pub fn pixel(fb: &Framebuffer, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * fb.width() + x) * 4) as usize;
    let p = fb.pixels();
    [p[i], p[i + 1], p[i + 2], p[i + 3]]
}

pub const WHITE: [u8; 4] = [255, 255, 255, 255];

/// A device-pixel region, inclusive-exclusive: `x0..x1`, `y0..y1`.
#[derive(Clone, Copy, Debug)]
pub struct Region {
    pub x0: u32,
    pub y0: u32,
    pub x1: u32,
    pub y1: u32,
}

impl Region {
    pub fn whole(fb: &Framebuffer) -> Self {
        Region {
            x0: 0,
            y0: 0,
            x1: fb.width(),
            y1: fb.height(),
        }
    }

    /// The device pixels a logical rect covers at the current device scale.
    pub fn of_logical(r: Rect, fb: &Framebuffer) -> Self {
        let s = device_scale();
        Region {
            x0: (r.x * s).floor().max(0.0) as u32,
            y0: (r.y * s).floor().max(0.0) as u32,
            x1: ((r.x + r.width) * s).ceil().min(fb.width() as f64) as u32,
            y1: ((r.y + r.height) * s).ceil().min(fb.height() as f64) as u32,
        }
    }
}

/// C#'s `ExtentOfPaintedPixels`: the width and height of everything in
/// `region` that differs from `background` (inclusive pixel indices, so the
/// extent is one more than the span), or (0, 0) when nothing is painted.
pub fn painted_extent(fb: &Framebuffer, region: Region, background: [u8; 4]) -> (f64, f64) {
    let (mut left, mut right, mut bottom, mut top) = (u32::MAX, 0u32, u32::MAX, 0u32);
    let mut any = false;
    for y in region.y0..region.y1 {
        for x in region.x0..region.x1 {
            if pixel(fb, x, y) != background {
                any = true;
                left = left.min(x);
                right = right.max(x);
                bottom = bottom.min(y);
                top = top.max(y);
            }
        }
    }
    if !any {
        return (0.0, 0.0);
    }
    ((right - left + 1) as f64, (top - bottom + 1) as f64)
}

/// C#'s `LeftmostPaintedColumnOf`: the first column with a non-white pixel,
/// or -1.
pub fn leftmost_painted_column(fb: &Framebuffer) -> f64 {
    for x in 0..fb.width() {
        for y in 0..fb.height() {
            if pixel(fb, x, y) != WHITE {
                return x as f64;
            }
        }
    }
    -1.0
}

/// The colour most of `region` is painted: the background a mark sits on.
pub fn dominant_color(fb: &Framebuffer, region: Region) -> [u8; 4] {
    let mut counts: Vec<([u8; 4], usize)> = Vec::new();
    for y in region.y0..region.y1 {
        for x in region.x0..region.x1 {
            let p = pixel(fb, x, y);
            match counts.iter_mut().find(|(c, _)| *c == p) {
                Some((_, n)) => *n += 1,
                None => counts.push((p, 1)),
            }
        }
    }
    counts
        .into_iter()
        .max_by_key(|(_, n)| *n)
        .map_or(WHITE, |(c, _)| c)
}
