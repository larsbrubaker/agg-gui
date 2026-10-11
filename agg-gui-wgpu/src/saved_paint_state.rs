//! The paint state `DrawCtx::save()` pushes and `restore()` pops on the wgpu
//! backend — everything the software `GfxCtx` keeps in its `GfxState` apart
//! from the transform and clip, which live in `state_stack` beside it
//! (see `draw_ctx_impl.rs`'s `save`/`restore`).
//!
//! Saving the whole state, as the software renderer and canvas do, keeps a
//! `save(); set_global_alpha(..); draw; restore()` block from leaking its
//! alpha (or colour, line width, font, blend mode) into later draws.

use super::*;

/// One `save()`'s copy of the paint state.
#[derive(Clone)]
pub(crate) struct SavedPaintState {
    fill_color: Color,
    fill_linear_gradient: Option<LinearGradientPaint>,
    fill_radial_gradient: Option<RadialGradientPaint>,
    stroke_color: Color,
    stroke_linear_gradient: Option<LinearGradientPaint>,
    stroke_radial_gradient: Option<RadialGradientPaint>,
    line_width: f64,
    line_join: LineJoin,
    line_cap: LineCap,
    fill_rule: FillRule,
    miter_limit: f64,
    line_dash: Vec<f64>,
    dash_offset: f64,
    global_alpha: f64,
    blend_mode: CompOp,
    font: Option<Arc<Font>>,
    font_size: f64,
}

impl WgpuGfxCtx {
    /// The current paint state, as `save()` pushes it.
    pub(crate) fn paint_state(&self) -> SavedPaintState {
        SavedPaintState {
            fill_color: self.fill_color,
            fill_linear_gradient: self.fill_linear_gradient.clone(),
            fill_radial_gradient: self.fill_radial_gradient.clone(),
            stroke_color: self.stroke_color,
            stroke_linear_gradient: self.stroke_linear_gradient.clone(),
            stroke_radial_gradient: self.stroke_radial_gradient.clone(),
            line_width: self.line_width,
            line_join: self.line_join,
            line_cap: self.line_cap,
            fill_rule: self.fill_rule,
            miter_limit: self.miter_limit,
            line_dash: self.line_dash.clone(),
            dash_offset: self.dash_offset,
            global_alpha: self.global_alpha,
            blend_mode: self.blend_mode,
            font: self.font.clone(),
            font_size: self.font_size,
        }
    }

    /// Make `s` the current paint state, as `restore()` pops it.
    pub(crate) fn set_paint_state(&mut self, s: SavedPaintState) {
        self.fill_color = s.fill_color;
        self.fill_linear_gradient = s.fill_linear_gradient;
        self.fill_radial_gradient = s.fill_radial_gradient;
        self.stroke_color = s.stroke_color;
        self.stroke_linear_gradient = s.stroke_linear_gradient;
        self.stroke_radial_gradient = s.stroke_radial_gradient;
        self.line_width = s.line_width;
        self.line_join = s.line_join;
        self.line_cap = s.line_cap;
        self.fill_rule = s.fill_rule;
        self.miter_limit = s.miter_limit;
        self.line_dash = s.line_dash;
        self.dash_offset = s.dash_offset;
        self.global_alpha = s.global_alpha;
        self.blend_mode = s.blend_mode;
        self.font = s.font;
        self.font_size = s.font_size;
    }
}
