//! Frame Demo — mirrors egui `FrameDemo`.
//!
//! Constructed via parent/child composition (MatterCAD / agg-sharp pattern):
//! every node is either a primitive (`Label`, `DragValue`) or a layout
//! container (`FlexRow`, `FlexColumn`) configured via the fluent `add(...)` /
//! `with_*(...)` API.  The only custom leaf is [`FramePreview`], which paints
//! the configured frame + shadow + centered "Content" label reactively from
//! shared state cells.
//!
//! Layout matches the egui demo:
//!   ┌──── controls (fixed 260 px wide) ────┐  ┌── preview (flex 1) ──┐
//!   │ Inner margin   [12]                  │  │                      │
//!   │ Outer margin   [24]                  │  │   [purple frame]     │
//!   │ Corner radius  [14]                  │  │   [  shadow      ]   │
//!   │ Fill opacity   [0.5]                 │  │   [   "Content"  ]   │
//!   │ Stroke width   [1.0]                 │  │                      │
//!   │ [ Reset ]                            │  │                      │
//!   └──────────────────────────────────────┘  └──────────────────────┘
//!
//! Defaults match egui exactly:
//!   inner_margin=12, outer_margin=24, corner_radius=14,
//!   fill=rgba(97, 0, 255, 128), stroke=(1.0, GRAY),
//!   shadow offset=[8, 12], blur=16, alpha=180/255.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{
    Button, Color, DragValue, DrawCtx, Event, EventResult,
    FlexColumn, FlexRow, Font, Label, Rect, ScrollView,
    Size, Widget,
};
use agg_gui::layout_props::{HAnchor, VAnchor};
use agg_gui::widget::paint_subtree;

// ── egui FrameDemo defaults ──────────────────────────────────────────────────

const DEF_CORNER_R:   f64 = 14.0;
const DEF_INNER_M:    f64 = 12.0;
const DEF_OUTER_M:    f64 = 24.0;
const DEF_FILL_ALPHA: f64 = 128.0 / 255.0;
const DEF_STROKE_W:   f64 = 1.0;

// Fill: egui purple rgba(97, 0, 255, α).
const FILL_R: f32 = 97.0 / 255.0;
const FILL_G: f32 = 0.0;
const FILL_B: f32 = 1.0;

// Shadow parameters (egui: offset [8, 12], color black α=180, blur=16).
const SHADOW_DX:    f64 = 8.0;
const SHADOW_DY:    f64 = 12.0;
const SHADOW_ALPHA: f32 = 180.0 / 255.0;
const SHADOW_BLUR:  f64 = 16.0;
// Number of stacked layers used to approximate a Gaussian blur falloff.
// Each layer inflates outward by BLUR/STEPS and fades with a quadratic
// (1 - t)² curve, which visually approximates a 2-σ Gaussian.
const SHADOW_STEPS: usize = 12;

const PREVIEW_H: f64 = 200.0;
const CONTROLS_W: f64 = 260.0;

// ── Reactive preview widget ──────────────────────────────────────────────────

/// The only custom leaf widget in this file.  Holds shared state cells and
/// a single `Label` child ("Content"); paints the outer wrapper frame, the
/// shadow, and the configured inner frame before the framework recurses into
/// the label.
struct FramePreview {
    bounds:   Rect,
    children: Vec<Box<dyn Widget>>,
    corner_r: Rc<Cell<f64>>,
    inner_m:  Rc<Cell<f64>>,
    outer_m:  Rc<Cell<f64>>,
    fill_a:   Rc<Cell<f64>>,
    stroke_w: Rc<Cell<f64>>,
    content:  Label,
}

impl Widget for FramePreview {
    fn type_name(&self) -> &'static str { "FramePreview" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size {
        let w = available.width;
        let h = PREVIEW_H.min(available.height.max(PREVIEW_H));
        self.bounds = Rect::new(0.0, 0.0, w, h);
        // Lay out the content label so we know its natural size for centring.
        let _ = self.content.layout(Size::new(w, h));
        Size::new(w, h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v  = ctx.visuals();
        let w  = self.bounds.width;
        let h  = self.bounds.height;

        let cr = self.corner_r.get();
        let im = self.inner_m.get();
        let om = self.outer_m.get();
        let fa = self.fill_a.get();
        let sw = self.stroke_w.get();

        // Outer wrapper frame (thin widget_stroke border, like egui's demo wrapper).
        ctx.set_stroke_color(v.widget_stroke);
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, w, h, 4.0);
        ctx.stroke();

        // Inner frame geometry (inset from all sides by outer_margin).
        let ix = om;
        let iy = om;
        let iw = (w - om * 2.0).max(4.0);
        let ih = (h - om * 2.0).max(4.0);

        // Drop shadow — approximates a Gaussian blur by stacking inflated
        // rounded rects with decreasing alpha.  Layers are drawn outside-in so
        // the opaque core sits on top of the softer outer halo.
        //
        // Y-up: shadow offset is +x right, −y down; so we subtract SHADOW_DY
        // from the y coordinate.
        let sx = ix + SHADOW_DX;
        let sy = iy - SHADOW_DY;
        for i in (0..SHADOW_STEPS).rev() {
            let t     = i as f64 / SHADOW_STEPS as f64;        // 0..<1
            let infl  = t * SHADOW_BLUR;
            let falloff = (1.0 - t).powi(2) as f32;             // quadratic
            let alpha = SHADOW_ALPHA * falloff / SHADOW_STEPS as f32 * 6.0;
            ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, alpha));
            ctx.begin_path();
            ctx.rounded_rect(
                sx - infl,
                sy - infl,
                iw + 2.0 * infl,
                ih + 2.0 * infl,
                cr + infl,
            );
            ctx.fill();
        }

        // Purple fill.
        ctx.set_fill_color(Color::rgba(FILL_R, FILL_G, FILL_B, fa as f32));
        ctx.begin_path();
        ctx.rounded_rect(ix, iy, iw, ih, cr);
        ctx.fill();

        // Gray stroke.
        if sw > 0.0 {
            ctx.set_stroke_color(Color::rgb(0.5, 0.5, 0.5));
            ctx.set_line_width(sw);
            ctx.begin_path();
            ctx.rounded_rect(ix, iy, iw, ih, cr);
            ctx.stroke();
        }

        // Centre the "Content" label inside the padded inner frame.
        //
        // Must re-measure via `layout()` every paint — `bounds()` only reflects
        // what the parent last called `set_bounds` with, which stays at (0,0,0,0)
        // otherwise.
        let avail_w = (iw - im * 2.0).max(0.0);
        let avail_h = (ih - im * 2.0).max(0.0);
        let csz = self.content.layout(Size::new(avail_w.max(1.0), avail_h.max(1.0)));
        let cx = ix + im + (avail_w - csz.width)  * 0.5;
        let cy = iy + im + (avail_h - csz.height) * 0.5;

        self.content.set_bounds(Rect::new(0.0, 0.0, csz.width, csz.height));
        ctx.save();
        ctx.translate(cx, cy);
        paint_subtree(&mut self.content, ctx);
        ctx.restore();
    }

    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}

// ── Builders (all composition — no other custom widgets) ─────────────────────

/// One labeled row: `[Label (fixed 110 px)] [DragValue (flex 1)]`.
fn labeled_row(
    label:  &'static str,
    cell:   Rc<Cell<f64>>,
    min:    f64,
    max:    f64,
    speed:  f64,
    font:   Arc<Font>,
) -> Box<dyn Widget>
{
    let c = Rc::clone(&cell);
    Box::new(
        FlexRow::new()
            .with_gap(8.0)
            .add(Box::new(
                Label::new(label, Arc::clone(&font))
                    .with_font_size(12.0)
                    .with_min_size(Size::new(110.0, 0.0))
                    .with_max_size(Size::new(110.0, f64::MAX))
            ))
            .add_flex(Box::new(
                DragValue::new(cell.get(), min, max, font)
                    .with_speed(speed)
                    .with_decimals(1)
                    .on_change(move |v| c.set(v))
                    .with_min_size(Size::new(60.0, 22.0))
            ), 1.0)
    )
}

/// Reset button — restores all cells to the egui defaults.
fn reset_button(
    font: Arc<Font>,
    corner_r: Rc<Cell<f64>>,
    inner_m:  Rc<Cell<f64>>,
    outer_m:  Rc<Cell<f64>>,
    fill_a:   Rc<Cell<f64>>,
    stroke_w: Rc<Cell<f64>>,
) -> Box<dyn Widget>
{
    let b = Button::new("Reset", font)
        .on_click(move || {
            corner_r.set(DEF_CORNER_R);
            inner_m .set(DEF_INNER_M);
            outer_m .set(DEF_OUTER_M);
            fill_a  .set(DEF_FILL_ALPHA);
            stroke_w.set(DEF_STROKE_W);
        });
    Box::new(b)
}

/// Build the Frame demo content widget (public entry point).
pub fn frame_demo(font: Arc<Font>) -> Box<dyn Widget> {
    // Shared state cells.
    let corner_r = Rc::new(Cell::new(DEF_CORNER_R));
    let inner_m  = Rc::new(Cell::new(DEF_INNER_M));
    let outer_m  = Rc::new(Cell::new(DEF_OUTER_M));
    let fill_a   = Rc::new(Cell::new(DEF_FILL_ALPHA));
    let stroke_w = Rc::new(Cell::new(DEF_STROKE_W));

    // ── Left column: controls ───────────────────────────────────────────────
    // Width is pinned via min/max so the FlexRow's flex child (preview) gets
    // the remainder — otherwise FlexColumn reports the full row width as
    // "natural" and the preview collapses to zero.
    let controls = FlexColumn::new()
        .with_gap(6.0)
        .with_padding(8.0)
        .with_min_size(Size::new(CONTROLS_W, 0.0))
        .with_max_size(Size::new(CONTROLS_W, f64::MAX))
        .with_v_anchor(VAnchor::FIT)
        .add(labeled_row("Inner margin",  Rc::clone(&inner_m),  0.0, 50.0, 1.0,  Arc::clone(&font)))
        .add(labeled_row("Outer margin",  Rc::clone(&outer_m),  0.0, 50.0, 1.0,  Arc::clone(&font)))
        .add(labeled_row("Corner radius", Rc::clone(&corner_r), 0.0, 50.0, 0.5,  Arc::clone(&font)))
        .add(labeled_row("Fill opacity",  Rc::clone(&fill_a),   0.0, 1.0,  0.01, Arc::clone(&font)))
        .add(labeled_row("Stroke width",  Rc::clone(&stroke_w), 0.0, 10.0, 0.5,  Arc::clone(&font)))
        .add(reset_button(
            Arc::clone(&font),
            Rc::clone(&corner_r),
            Rc::clone(&inner_m),
            Rc::clone(&outer_m),
            Rc::clone(&fill_a),
            Rc::clone(&stroke_w),
        ));

    // ── Right column: live preview ──────────────────────────────────────────
    let preview = FramePreview {
        bounds:   Rect::default(),
        children: Vec::new(),
        corner_r: Rc::clone(&corner_r),
        inner_m:  Rc::clone(&inner_m),
        outer_m:  Rc::clone(&outer_m),
        fill_a:   Rc::clone(&fill_a),
        stroke_w: Rc::clone(&stroke_w),
        content:  Label::new("Content", Arc::clone(&font))
                      .with_font_size(13.0)
                      .with_color(Color::white()),
    };

    // ── Main row ────────────────────────────────────────────────────────────
    Box::new(
        ScrollView::new(Box::new(
            FlexRow::new()
                .with_gap(8.0)
                .with_padding(8.0)
                .with_h_anchor(HAnchor::STRETCH)
                .add(Box::new(controls))
                .add_flex(Box::new(preview), 1.0)
        ))
    )
}
