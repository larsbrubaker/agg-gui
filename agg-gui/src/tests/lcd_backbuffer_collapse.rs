//! Regression test for the **default** [`DrawCtx::draw_lcd_backbuffer_arc`]
//! collapse — the software path a nested `BackbufferMode::LcdCoverage` widget
//! takes when its cached two-plane backbuffer is blitted into a parent that is
//! itself an LCD backbuffer.
//!
//! `paint_subtree_backbuffered` calls `draw_lcd_backbuffer_arc` on whatever ctx
//! it is handed.  The wgpu backend overrides it with the two-texture
//! `lcb_flatten` shader, but `LcdGfxCtx` does not, so this default body is live
//! CPU code — see the workaround note on `menu/widget/labels.rs::make_bar_label`,
//! which disables a child Label's backbuffer specifically to avoid it.
//!
//! It has to collapse the per-channel planes the same way
//! [`crate::lcd_coverage::LcdBuffer::to_rgba8_top_down_collapsed`] and the
//! `lcb_flatten` shader do; a third, divergent copy of that math is exactly how
//! the "text in windows is bolder when LCD is on" bug survived in one path
//! after being fixed in another.  Both now delegate to
//! [`crate::lcd_coverage::collapse_lcd_pixel`], and this test pins the wiring.

use crate::draw_ctx::{FillRule, GlPaint, LinearGradientPaint};
use crate::lcd_coverage::collapse_lcd_pixel;
use crate::text::{Font, TextMetrics};
use crate::{Color, DrawCtx, Rect};
use agg_rust::comp_op::CompOp;
use agg_rust::math_stroke::{LineCap, LineJoin};
use agg_rust::trans_affine::TransAffine;
use std::sync::Arc;

/// Captures the RGBA image the default collapse forwards to `draw_image_rgba`.
#[derive(Default)]
struct ImageCaptureCtx {
    last_image: Option<Vec<u8>>,
    last_dims: Option<(u32, u32)>,
}

impl DrawCtx for ImageCaptureCtx {
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
        TransAffine::new()
    }
    fn save(&mut self) {}
    fn restore(&mut self) {}
    fn translate(&mut self, _tx: f64, _ty: f64) {}
    fn rotate(&mut self, _radians: f64) {}
    fn scale(&mut self, _sx: f64, _sy: f64) {}
    fn set_transform(&mut self, _m: TransAffine) {}
    fn reset_transform(&mut self) {}
    fn draw_image_rgba(
        &mut self,
        data: &[u8],
        img_w: u32,
        img_h: u32,
        _dst_x: f64,
        _dst_y: f64,
        _dst_w: f64,
        _dst_h: f64,
    ) {
        self.last_image = Some(data.to_vec());
        self.last_dims = Some((img_w, img_h));
    }
    fn gl_paint(&mut self, _screen_rect: Rect, _painter: &mut dyn GlPaint) {}
}

/// The same unequal-alpha probe pixels the `lcd_coverage` collapse tests use.
const COVS: [[u8; 3]; 4] = [[85, 200, 85], [50, 120, 230], [255, 255, 255], [0, 0, 0]];

/// Build top-down `(color, alpha)` planes for [`COVS`] with premultiplied
/// colour `level * coverage` — the shape a real cached LCD backbuffer has.
fn probe_planes(level: f64) -> (Arc<Vec<u8>>, Arc<Vec<u8>>) {
    let mut color = Vec::new();
    let mut alpha = Vec::new();
    for cov in COVS {
        for c in 0..3 {
            color.push((cov[c] as f64 * level).round() as u8);
            alpha.push(cov[c]);
        }
    }
    (Arc::new(color), Arc::new(alpha))
}

/// The default `draw_lcd_backbuffer_arc` must collapse through the SHARED
/// [`collapse_lcd_pixel`], not a private copy of the math.
///
/// Asserting equality against the shared helper (rather than re-deriving the
/// expected bytes here) is deliberate: it makes this a wiring test that cannot
/// drift out of sync with the collapse rule, while
/// `lcd_coverage::tests::collapsed_backbuffer_luminance_matches_per_channel_composite`
/// independently pins what that rule must actually compute.  A regression to
/// `max`, or a copy that forgets the light-text alpha lift, fails here.
#[test]
fn default_draw_lcd_backbuffer_arc_uses_shared_weighted_collapse() {
    // Both polarities: dark text exercises the weighted mean, light text
    // exercises the lift that keeps the unpremultiply from clamping.
    for level in [0.1, 0.9] {
        let (color, alpha) = probe_planes(level);
        let w = COVS.len() as u32;

        let mut ctx = ImageCaptureCtx::default();
        ctx.draw_lcd_backbuffer_arc(&color, &alpha, 0, w, 1, 0.0, 0.0, w as f64, 1.0);

        let img = ctx
            .last_image
            .expect("default collapse must forward to draw_image_rgba");
        assert_eq!(
            ctx.last_dims,
            Some((w, 1)),
            "blit dimensions must round-trip"
        );
        assert_eq!(
            img.len(),
            (w as usize) * 4,
            "one RGBA8 pixel per source pixel"
        );

        for x in 0..COVS.len() {
            let expect = collapse_lcd_pixel(
                [color[x * 3], color[x * 3 + 1], color[x * 3 + 2]],
                [alpha[x * 3], alpha[x * 3 + 1], alpha[x * 3 + 2]],
            );
            let got = [img[x * 4], img[x * 4 + 1], img[x * 4 + 2], img[x * 4 + 3]];
            assert_eq!(
                got, expect,
                "level {level} pixel {x} (coverage {:?}): default \
                 draw_lcd_backbuffer_arc diverged from collapse_lcd_pixel",
                COVS[x],
            );
        }
    }
}
