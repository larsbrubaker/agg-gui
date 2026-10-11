//! Shared helpers for the agg-sharp layout ports: the demo font, fixed-size
//! leaves, laying a root out at a size, painting a tree into a white CPU
//! framebuffer and reading its pixels (Y-up, row 0 at the bottom, as C#'s
//! `ImageBuffer`), and a lock for tests that change the process-wide device
//! scale.

use std::sync::{Arc, Mutex, MutexGuard};

use agg_gui::text::Font;
use agg_gui::widget::paint_subtree;
use agg_gui::{Color, Framebuffer, GfxCtx, Rect, Size, SizedBox, Widget};

static REGULAR: &[u8] = include_bytes!("../../../demo/assets/Nunito_Regular.ttf");

pub fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(REGULAR).expect("demo font"))
}

/// A fixed-size leaf (C# `new GuiWidget(w, h)`).
pub fn sized(w: f64, h: f64) -> SizedBox {
    SizedBox::new().with_width(w).with_height(h)
}

/// Lay `root` out at `size` and place it at the origin, as a parent
/// `GuiWidget` of that size would.
pub fn layout_at(root: &mut dyn Widget, size: Size) -> Size {
    let got = root.layout(size);
    root.set_bounds(Rect::new(0.0, 0.0, got.width, got.height));
    got
}

/// Bounds of the descendant at `path` (child indices from `root`).
pub fn bounds_at(root: &dyn Widget, path: &[usize]) -> Rect {
    let mut w = root;
    for &i in path {
        w = w.children()[i].as_ref();
    }
    w.bounds()
}

/// Paint `root` (already laid out) into a `w` x `h` framebuffer cleared to
/// white (C# `graphics2D.Clear(Color.White)` then `OnDraw`).
pub fn render(root: &mut dyn Widget, w: u32, h: u32) -> Framebuffer {
    let mut fb = Framebuffer::new(w, h);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::rgba(1.0, 1.0, 1.0, 1.0));
        paint_subtree(root, &mut ctx);
    }
    fb
}

/// RGBA of pixel (`x`, `y`), Y-up.
pub fn pixel(fb: &Framebuffer, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * fb.width() + x) * 4) as usize;
    let p = fb.pixels();
    [p[i], p[i + 1], p[i + 2], p[i + 3]]
}

pub const RED: [u8; 4] = [255, 0, 0, 255];
pub const BLUE: [u8; 4] = [0, 0, 255, 255];

/// Serializes the tests that set the process-wide device scale (C#
/// `[NotInParallel]`), restoring it when dropped.
pub struct ScaleGuard {
    saved: f64,
    _lock: MutexGuard<'static, ()>,
}

static SCALE_LOCK: Mutex<()> = Mutex::new(());

impl ScaleGuard {
    pub fn set(scale: f64) -> Self {
        let lock = SCALE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let saved = agg_gui::device_scale();
        agg_gui::set_device_scale(scale);
        Self { saved, _lock: lock }
    }
}

impl Drop for ScaleGuard {
    fn drop(&mut self) {
        agg_gui::set_device_scale(self.saved);
    }
}
