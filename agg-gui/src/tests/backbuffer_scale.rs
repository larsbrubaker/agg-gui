//! Regression test: CPU-backbuffered widgets must rasterise at the current
//! CTM scale, not the bare device-pixel ratio.
//!
//! Mobile platforms set a UX zoom (`ux_scale ≈ 1.7`) on top of the device
//! pixel ratio, and `App::paint` bakes the combined `effective_scale` into the
//! CTM before walking the tree.  The CPU backbuffer path
//! (`paint_subtree_backbuffered`) used to size its offscreen bitmap from
//! `device_scale()` alone, leaving every cached widget (the menu bar, Labels)
//! rendered at `1/ux_scale` of its layout slot while GL-FBO widgets (Windows),
//! which allocate from the CTM, scaled correctly.  This test pins the bitmap
//! resolution to the CTM scale so the regression can't silently return.

use super::*;

use crate::draw_ctx::{FillRule, GlPaint, LinearGradientPaint};
use crate::text::{Font, TextMetrics};
use crate::widget::{paint_subtree, BackbufferCache, BackbufferMode};
use crate::{Color, DrawCtx, Event, EventResult, Rect};
use agg_rust::comp_op::CompOp;
use agg_rust::math_stroke::{LineCap, LineJoin};
use agg_rust::trans_affine::TransAffine;
use std::sync::Arc;

/// Minimal widget that opts into a CPU (Rgba) backbuffer and paints an opaque
/// fill over its bounds.
struct CpuBackbufferProbe {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    cache: BackbufferCache,
}

impl CpuBackbufferProbe {
    fn new(bounds: Rect) -> Self {
        Self {
            bounds,
            children: Vec::new(),
            cache: BackbufferCache::new(),
        }
    }
}

impl Widget for CpuBackbufferProbe {
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }
    fn layout(&mut self, available: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
        available
    }
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        ctx.set_fill_color(Color::rgb(0.1, 0.2, 0.3));
        ctx.begin_path();
        ctx.rect(0.0, 0.0, self.bounds.width, self.bounds.height);
        ctx.fill();
    }
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
    fn backbuffer_cache_mut(&mut self) -> Option<&mut BackbufferCache> {
        Some(&mut self.cache)
    }
    fn backbuffer_mode(&self) -> BackbufferMode {
        BackbufferMode::Rgba
    }
}

/// Records the dimensions of the last `draw_image_rgba_arc` blit and carries a
/// scriptable CTM so the test can simulate `App::paint`'s `ctx.scale(...)`.
struct RecordingCtx {
    transform: TransAffine,
    stack: Vec<TransAffine>,
    last_blit: Option<BlitRecord>,
}

#[derive(Clone, Copy)]
struct BlitRecord {
    img_w: u32,
    img_h: u32,
    dst_w: f64,
    dst_h: f64,
}

impl RecordingCtx {
    fn with_scale(scale: f64) -> Self {
        Self {
            transform: TransAffine::new_scaling(scale, scale),
            stack: Vec::new(),
            last_blit: None,
        }
    }
}

impl DrawCtx for RecordingCtx {
    fn set_fill_color(&mut self, _color: Color) {}
    fn set_stroke_color(&mut self, _color: Color) {}
    fn set_fill_linear_gradient(&mut self, _gradient: LinearGradientPaint) {}
    fn set_fill_radial_gradient(&mut self, _gradient: crate::draw_ctx::RadialGradientPaint) {}
    fn set_line_width(&mut self, _w: f64) {}
    fn set_line_join(&mut self, _join: LineJoin) {}
    fn set_line_cap(&mut self, _cap: LineCap) {}
    fn set_miter_limit(&mut self, _limit: f64) {}
    fn set_line_dash(&mut self, _dashes: &[f64], _offset: f64) {}
    fn set_blend_mode(&mut self, _mode: CompOp) {}
    fn set_global_alpha(&mut self, _alpha: f64) {}
    fn set_fill_rule(&mut self, _rule: FillRule) {}
    fn set_font(&mut self, _font: Arc<Font>) {}
    fn set_font_size(&mut self, _size: f64) {}
    fn clip_rect(&mut self, _x: f64, _y: f64, _w: f64, _h: f64) {}
    fn reset_clip(&mut self) {}
    fn clear(&mut self, _color: Color) {}
    fn begin_path(&mut self) {}
    fn move_to(&mut self, _x: f64, _y: f64) {}
    fn line_to(&mut self, _x: f64, _y: f64) {}
    fn cubic_to(&mut self, _cx1: f64, _cy1: f64, _cx2: f64, _cy2: f64, _x: f64, _y: f64) {}
    fn quad_to(&mut self, _cx: f64, _cy: f64, _x: f64, _y: f64) {}
    fn arc_to(&mut self, _cx: f64, _cy: f64, _r: f64, _s: f64, _e: f64, _ccw: bool) {}
    fn circle(&mut self, _cx: f64, _cy: f64, _r: f64) {}
    fn rect(&mut self, _x: f64, _y: f64, _w: f64, _h: f64) {}
    fn rounded_rect(&mut self, _x: f64, _y: f64, _w: f64, _h: f64, _r: f64) {}
    fn close_path(&mut self) {}
    fn fill(&mut self) {}
    fn stroke(&mut self) {}
    fn fill_and_stroke(&mut self) {}
    fn draw_triangles_aa(&mut self, _vertices: &[[f32; 3]], _indices: &[u32], _color: Color) {}
    fn fill_text(&mut self, _text: &str, _x: f64, _y: f64) {}
    fn fill_text_gsv(&mut self, _text: &str, _x: f64, _y: f64, _size: f64) {}
    fn measure_text(&self, _text: &str) -> Option<TextMetrics> {
        None
    }
    fn transform(&self) -> TransAffine {
        self.transform
    }
    fn save(&mut self) {
        self.stack.push(self.transform);
    }
    fn restore(&mut self) {
        if let Some(t) = self.stack.pop() {
            self.transform = t;
        }
    }
    fn translate(&mut self, tx: f64, ty: f64) {
        self.transform
            .premultiply(&TransAffine::new_translation(tx, ty));
    }
    fn rotate(&mut self, radians: f64) {
        self.transform
            .premultiply(&TransAffine::new_rotation(radians));
    }
    fn scale(&mut self, sx: f64, sy: f64) {
        self.transform.premultiply(&TransAffine::new_scaling(sx, sy));
    }
    fn set_transform(&mut self, m: TransAffine) {
        self.transform = m;
    }
    fn reset_transform(&mut self) {
        self.transform = TransAffine::new();
    }
    fn draw_image_rgba_arc(
        &mut self,
        _data: &Arc<Vec<u8>>,
        img_w: u32,
        img_h: u32,
        _dst_x: f64,
        _dst_y: f64,
        dst_w: f64,
        dst_h: f64,
    ) {
        self.last_blit = Some(BlitRecord {
            img_w,
            img_h,
            dst_w,
            dst_h,
        });
    }
    fn gl_paint(&mut self, _screen_rect: Rect, _painter: &mut dyn GlPaint) {}
}

#[test]
fn cpu_backbuffer_rasterises_at_ctm_scale() {
    // Device scale stays at the test default of 1.0; the 3.0 here stands in
    // for `effective_scale = device_scale × ux_scale` the way `App::paint`
    // applies it before walking the widget tree.
    const SCALE: f64 = 3.0;
    let mut widget = CpuBackbufferProbe::new(Rect::new(0.0, 0.0, 100.0, 20.0));
    let mut ctx = RecordingCtx::with_scale(SCALE);

    paint_subtree(&mut widget, &mut ctx);

    // The offscreen bitmap must be sized to the widget's ON-SCREEN footprint
    // (bounds × CTM scale), so the subsequent blit is texel-for-pixel.
    assert_eq!(
        widget.cache.width, 300,
        "bitmap width must follow the CTM scale (100 × 3), not device_scale"
    );
    assert_eq!(
        widget.cache.height, 60,
        "bitmap height must follow the CTM scale (20 × 3), not device_scale"
    );

    let blit = ctx.last_blit.expect("widget must blit its cached bitmap");
    assert_eq!((blit.img_w, blit.img_h), (300, 60));
    // The destination rect is logical (bitmap / scale); the CTM scales it back
    // up so dst × scale == bitmap size — a 1:1 blit with no up/downscale.
    assert!((blit.dst_w * SCALE - blit.img_w as f64).abs() < 1.0);
    assert!((blit.dst_h * SCALE - blit.img_h as f64).abs() < 1.0);
}
