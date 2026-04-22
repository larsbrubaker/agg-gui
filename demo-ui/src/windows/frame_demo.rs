//! Frame Demo — mirrors egui `FrameDemo`.
//!
//! Reproduces egui's Frame inspector:
//!   - `Inner margin`  : "same" checkbox + DragValue, expands to L/R/T/B when off
//!   - `Outer margin`  : same
//!   - `Corner radius` : "same" checkbox + DragValue, expands to NW/NE/SW/SE
//!   - `Shadow`        : x / y drag values (row 1), blur / spread (row 2), colour picker
//!   - `Fill`          : colour picker (with "No Color (Pass Through)")
//!   - `Stroke`        : width DragValue + colour picker
//!   - `Reset`         : restores every value to egui defaults.
//!
//! Layout:
//!   ┌──── controls ────┐  ┌── preview ──┐
//!   │ …control rows…   │  │  [frame]    │
//!   └──────────────────┘  └─────────────┘
//!
//! The parent `Window` is built with `.with_auto_size(true)` so expanding a
//! "same" checkbox or opening a colour picker grows the window downward.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{
    Button, Checkbox, Color, ColorPicker, DragValue, DrawCtx, Event, EventResult,
    FlexColumn, FlexRow, Font, Label, Rect, Size, Widget,
};
use agg_gui::layout_props::{HAnchor, VAnchor};
use agg_gui::widget::paint_subtree;


// ── egui FrameDemo defaults ──────────────────────────────────────────────────

const DEF_CORNER_R:   f64 = 14.0;
const DEF_INNER_M:    f64 = 12.0;
const DEF_OUTER_M:    f64 = 24.0;
const DEF_STROKE_W:   f64 = 1.0;

const DEF_SHADOW_DX:    f64 = 8.0;
const DEF_SHADOW_DY:    f64 = 12.0;
const DEF_SHADOW_BLUR:  f64 = 16.0;
const DEF_SHADOW_SPREAD:f64 = 0.0;

fn def_fill()        -> Color { Color::rgba(97.0/255.0, 0.0, 1.0, 128.0/255.0) }
fn def_stroke_col()  -> Color { Color::rgb(0.5, 0.5, 0.5) }
fn def_shadow_col()  -> Color { Color::rgba(0.0, 0.0, 0.0, 180.0/255.0) }

const PREVIEW_W: f64 = 160.0;
const PREVIEW_H: f64 = 140.0;
const CONTROLS_W: f64 = 360.0;
const LABEL_W: f64 = 104.0;
const FIELD_W: f64 = 200.0;

// Number of stacked layers used to approximate a Gaussian blur falloff.
const SHADOW_STEPS: usize = 12;

// ── Shared state ─────────────────────────────────────────────────────────────

struct FourVal {
    /// a, b, c, d — for margins: [left, right, top, bottom], for corner: [nw, ne, sw, se].
    vals: [Rc<Cell<f64>>; 4],
    same: Rc<Cell<bool>>,
}

impl FourVal {
    fn uniform(v: f64) -> Self {
        Self {
            vals: [Rc::new(Cell::new(v)), Rc::new(Cell::new(v)),
                   Rc::new(Cell::new(v)), Rc::new(Cell::new(v))],
            same: Rc::new(Cell::new(true)),
        }
    }
    fn set_all(&self, v: f64) { for c in &self.vals { c.set(v); } self.same.set(true); }
    fn get(&self, i: usize) -> f64 { self.vals[i].get() }
}

struct FrameState {
    inner_m:    FourVal,          // [L, R, T, B]
    outer_m:    FourVal,
    corner_r:   FourVal,          // [NW, NE, SW, SE]
    shadow_dx:  Rc<Cell<f64>>,
    shadow_dy:  Rc<Cell<f64>>,
    shadow_blur:Rc<Cell<f64>>,
    shadow_spread: Rc<Cell<f64>>,
    shadow_col: Rc<Cell<Color>>,
    fill:       Rc<Cell<Color>>,
    stroke_w:   Rc<Cell<f64>>,
    stroke_col: Rc<Cell<Color>>,
}

impl FrameState {
    fn defaults() -> Self {
        Self {
            inner_m:    FourVal::uniform(DEF_INNER_M),
            outer_m:    FourVal::uniform(DEF_OUTER_M),
            corner_r:   FourVal::uniform(DEF_CORNER_R),
            shadow_dx:  Rc::new(Cell::new(DEF_SHADOW_DX)),
            shadow_dy:  Rc::new(Cell::new(DEF_SHADOW_DY)),
            shadow_blur:Rc::new(Cell::new(DEF_SHADOW_BLUR)),
            shadow_spread: Rc::new(Cell::new(DEF_SHADOW_SPREAD)),
            shadow_col: Rc::new(Cell::new(def_shadow_col())),
            fill:       Rc::new(Cell::new(def_fill())),
            stroke_w:   Rc::new(Cell::new(DEF_STROKE_W)),
            stroke_col: Rc::new(Cell::new(def_stroke_col())),
        }
    }

    fn reset(&self) {
        self.inner_m.set_all(DEF_INNER_M);
        self.outer_m.set_all(DEF_OUTER_M);
        self.corner_r.set_all(DEF_CORNER_R);
        self.shadow_dx.set(DEF_SHADOW_DX);
        self.shadow_dy.set(DEF_SHADOW_DY);
        self.shadow_blur.set(DEF_SHADOW_BLUR);
        self.shadow_spread.set(DEF_SHADOW_SPREAD);
        self.shadow_col.set(def_shadow_col());
        self.fill.set(def_fill());
        self.stroke_w.set(DEF_STROKE_W);
        self.stroke_col.set(def_stroke_col());
    }
}

// ── Drawing helpers ──────────────────────────────────────────────────────────

/// Cubic-Bezier approximation of a quarter-circle corner.
/// k = 4/3 · (√2 − 1) — standard "kappa" constant.
const KAPPA: f64 = 0.5522847498307933;

/// Build a rounded-rect path with **four distinct corner radii**.
///
/// `ctx` must have `begin_path()` called before this; the caller does `fill()`
/// or `stroke()` after.  Winding is clockwise starting from the bottom-left
/// corner (Y-up).
///
/// Each `r_*` is clamped against half the shorter side so extreme values
/// don't produce kinks.
fn rounded_rect_4(
    ctx: &mut dyn DrawCtx,
    x: f64, y: f64, w: f64, h: f64,
    r_nw: f64, r_ne: f64, r_sw: f64, r_se: f64,
) {
    let max_r = (w.min(h)) * 0.5;
    let r_nw = r_nw.clamp(0.0, max_r);
    let r_ne = r_ne.clamp(0.0, max_r);
    let r_sw = r_sw.clamp(0.0, max_r);
    let r_se = r_se.clamp(0.0, max_r);
    let k = KAPPA;

    // Start bottom edge just past the SW corner going right.
    ctx.move_to(x + r_sw, y);
    // Bottom edge → SE corner start.
    ctx.line_to(x + w - r_se, y);
    // SE corner: bottom-right, curve up to right edge.
    ctx.cubic_to(
        x + w - r_se + k * r_se, y,
        x + w,                   y + r_se - k * r_se,
        x + w,                   y + r_se,
    );
    // Right edge → NE corner start.
    ctx.line_to(x + w, y + h - r_ne);
    // NE corner: top-right, curve left to top edge.
    ctx.cubic_to(
        x + w,                   y + h - r_ne + k * r_ne,
        x + w - r_ne + k * r_ne, y + h,
        x + w - r_ne,            y + h,
    );
    // Top edge → NW corner start.
    ctx.line_to(x + r_nw, y + h);
    // NW corner: top-left, curve down to left edge.
    ctx.cubic_to(
        x + r_nw - k * r_nw, y + h,
        x,                   y + h - r_nw + k * r_nw,
        x,                   y + h - r_nw,
    );
    // Left edge → SW corner start.
    ctx.line_to(x, y + r_sw);
    // SW corner: bottom-left, curve right to bottom edge.
    ctx.cubic_to(
        x,                   y + r_sw - k * r_sw,
        x + r_sw - k * r_sw, y,
        x + r_sw,            y,
    );
    ctx.close_path();
}

// ── IntrinsicRow — horizontal container that reports sum-of-children width ──
//
// `FlexRow::layout` returns `available.width` as its natural width, which is
// fine for flex-filling layouts but defeats auto-sizing: the demo's root
// window can't know how wide the content wants to be.  `IntrinsicRow` lays out
// two fixed children (controls + preview) side-by-side and returns the
// ACTUAL sum of their widths, so `Window::with_auto_size` can grow/shrink
// the window as the preview grows/shrinks with outer_margin.

struct IntrinsicRow {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    base: agg_gui::WidgetBase,
    gap: f64,
    padding: f64,
}

impl IntrinsicRow {
    fn new(gap: f64, padding: f64, children: Vec<Box<dyn Widget>>) -> Self {
        Self {
            bounds: Rect::default(),
            children,
            base: agg_gui::WidgetBase::new(),
            gap, padding,
        }
    }
}

impl Widget for IntrinsicRow {
    fn type_name(&self) -> &'static str { "IntrinsicRow" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn margin(&self)   -> agg_gui::Insets  { self.base.margin }
    fn h_anchor(&self) -> HAnchor { self.base.h_anchor }
    fn v_anchor(&self) -> VAnchor { self.base.v_anchor }
    fn min_size(&self) -> Size    { self.base.min_size }
    fn max_size(&self) -> Size    { self.base.max_size }

    fn layout(&mut self, available: Size) -> Size {
        let n = self.children.len();
        if n == 0 { return Size::new(0.0, 0.0); }
        let inner_h = (available.height - self.padding * 2.0).max(0.0);

        // Measure each child at its natural size.  For children whose layout
        // returns `available.width` (flex containers), we pass the child's own
        // `max_size.width` so they self-cap.
        let mut widths  = vec![0.0f64; n];
        let mut heights = vec![0.0f64; n];
        for i in 0..n {
            let max_w = self.children[i].max_size().width;
            let child_avail_w = if max_w.is_finite() { max_w } else { available.width };
            let sz = self.children[i].layout(Size::new(child_avail_w, inner_h));
            widths[i]  = sz.width .clamp(self.children[i].min_size().width,
                                         self.children[i].max_size().width);
            heights[i] = sz.height.clamp(self.children[i].min_size().height,
                                         self.children[i].max_size().height);
        }

        let total_w: f64 = widths.iter().sum::<f64>()
            + self.gap * (n.saturating_sub(1)) as f64
            + self.padding * 2.0;
        let max_h: f64 = heights.iter().cloned().fold(0.0f64, f64::max)
            + self.padding * 2.0;

        // Place.
        let mut cursor_x = self.padding;
        for i in 0..n {
            let w = widths[i];
            let h = heights[i];
            // Top-align each child within the row (Y-up → high y).
            let y = max_h - self.padding - h;
            self.children[i].set_bounds(Rect::new(cursor_x, y, w, h));
            cursor_x += w + self.gap;
        }

        self.bounds = Rect::new(0.0, 0.0, total_w, max_h);
        Size::new(total_w, max_h)
    }

    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}

// ── Reactive preview widget ──────────────────────────────────────────────────

/// Paints the configured frame + shadow + centered "Content" label.  Reads
/// entirely from shared state cells each paint pass so the preview updates
/// live as the user drags any control.
struct FramePreview {
    bounds:   Rect,
    children: Vec<Box<dyn Widget>>,
    st:       Rc<FrameState>,
    content:  Label,
}

impl FramePreview {
    /// Compute the preview (wrapper) size from the current state:
    ///   wrapper = frame + outer_margin, frame = content + inner_margin.
    ///
    /// Content natural size is the "Content" label measured at infinite
    /// available size.  Caller is expected to have a live font context when
    /// this is called; `Label::layout` does not require a `DrawCtx`.
    fn preview_size(&mut self) -> (f64, f64) {
        let st = &self.st;
        let om_l = st.outer_m.get(0);
        let om_r = st.outer_m.get(1);
        let om_t = st.outer_m.get(2);
        let om_b = st.outer_m.get(3);
        let im_l = st.inner_m.get(0);
        let im_r = st.inner_m.get(1);
        let im_t = st.inner_m.get(2);
        let im_b = st.inner_m.get(3);

        let csz = self.content.layout(Size::new(f64::MAX, f64::MAX));
        let content_w = csz.width.max(1.0);
        let content_h = csz.height.max(1.0);

        let frame_w = content_w + im_l + im_r;
        let frame_h = content_h + im_t + im_b;
        let wrap_w  = (frame_w + om_l + om_r).max(PREVIEW_W);
        let wrap_h  = (frame_h + om_t + om_b).max(PREVIEW_H);
        (wrap_w, wrap_h)
    }
}

impl Widget for FramePreview {
    fn type_name(&self) -> &'static str { "FramePreview" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size {
        // Preview size is derived from content + inner_margin (frame) +
        // outer_margin (wrapper).  This way `outer_margin` grows the wrapper
        // outward, and Window::with_auto_size propagates that growth up to the
        // window frame itself — exactly what the user asked for.
        let (w, h) = self.preview_size();
        self.bounds = Rect::new(0.0, 0.0, w, h);
        let _ = self.content.layout(Size::new(available.width.max(1.0), available.height.max(1.0)));
        Size::new(w, h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v  = ctx.visuals();
        let w  = self.bounds.width;
        let h  = self.bounds.height;
        let st = &self.st;

        let om_l = st.outer_m.get(0);
        let _om_r = st.outer_m.get(1);
        let om_t = st.outer_m.get(2);
        let _om_b = st.outer_m.get(3);
        let im_l = st.inner_m.get(0);
        let im_r = st.inner_m.get(1);
        let im_t = st.inner_m.get(2);
        let im_b = st.inner_m.get(3);
        let r_nw = st.corner_r.get(0);
        let r_ne = st.corner_r.get(1);
        let r_sw = st.corner_r.get(2);
        let r_se = st.corner_r.get(3);

        let sw   = st.stroke_w.get();
        let fill = st.fill.get();
        let stroke_col = st.stroke_col.get();
        let shadow_col = st.shadow_col.get();
        let sdx  = st.shadow_dx.get();
        let sdy  = st.shadow_dy.get();
        let sblur= st.shadow_blur.get();
        let sspread = st.shadow_spread.get();

        // Outer wrapper frame (thin noninteractive border at the edge of the
        // preview area — `outer_margin` = the gap between wrapper edge and
        // the purple frame, so growing outer_margin grows this wrapper too).
        ctx.set_stroke_color(v.widget_stroke);
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, w, h, 4.0);
        ctx.stroke();

        // Measure the content and compute frame (purple) size.  Content stays
        // at its natural size; inner_margin grows the FRAME around it.
        let csz = self.content.layout(Size::new(f64::MAX, f64::MAX));
        let content_w = csz.width.max(1.0);
        let content_h = csz.height.max(1.0);
        let iw = (content_w + im_l + im_r).max(4.0);
        let ih = (content_h + im_t + im_b).max(4.0);
        // Frame pinned at outer_margin offset from the wrapper's top-left
        // corner (Y-up: top edge of wrapper = h, so iy = h - om_t - ih).
        let ix = om_l;
        let iy = h - om_t - ih;

        // Drop shadow — stacked rounded rects, offset + inflate per layer.
        // Shadow offset in egui is [dx, dy] Y-down → here we subtract dy.
        let sx_base = ix + sdx - sspread;
        let sy_base = iy - sdy - sspread;
        let sw_sh = iw + 2.0 * sspread;
        let sh_sh = ih + 2.0 * sspread;
        for i in (0..SHADOW_STEPS).rev() {
            let t     = i as f64 / SHADOW_STEPS as f64;
            let infl  = t * sblur;
            let falloff = (1.0 - t).powi(2) as f32;
            let alpha = shadow_col.a * falloff / SHADOW_STEPS as f32 * 6.0;
            ctx.set_fill_color(Color::rgba(
                shadow_col.r, shadow_col.g, shadow_col.b, alpha));
            ctx.begin_path();
            rounded_rect_4(ctx,
                sx_base - infl, sy_base - infl,
                sw_sh + 2.0 * infl, sh_sh + 2.0 * infl,
                r_nw + infl, r_ne + infl, r_sw + infl, r_se + infl);
            ctx.fill();
        }

        // Fill.
        ctx.set_fill_color(fill);
        ctx.begin_path();
        rounded_rect_4(ctx, ix, iy, iw, ih, r_nw, r_ne, r_sw, r_se);
        ctx.fill();

        // Stroke.
        if sw > 0.0 && stroke_col.a > 0.0 {
            ctx.set_stroke_color(stroke_col);
            ctx.set_line_width(sw);
            ctx.begin_path();
            rounded_rect_4(ctx, ix, iy, iw, ih, r_nw, r_ne, r_sw, r_se);
            ctx.stroke();
        }

        // Paint the content at its natural size, centered inside the frame's
        // inner margin rect (so the inner-margin gap is equal on all sides
        // when the four values are equal).
        let cx = ix + im_l;
        let cy = iy + im_b;

        self.content.set_bounds(Rect::new(0.0, 0.0, content_w, content_h));
        ctx.save();
        ctx.translate(cx, cy);
        paint_subtree(&mut self.content, ctx);
        ctx.restore();
    }

    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}

// ── FourValueField — "same" checkbox with 1 or 4 DragValues ──────────────────

/// Compact field that mirrors egui's `Margin` / `CornerRadius` inspector:
/// a `[same] [value]` row when all four components are equal; expands
/// vertically into four labeled rows when the checkbox is unticked.
///
/// Rebuilds its children whenever `same_cell` flips.
struct FourValueField {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    font: Arc<Font>,
    font_size: f64,
    labels: [&'static str; 4],
    four: Rc<FourValInner>,
    min: f64,
    max: f64,
    speed: f64,
    /// Tracks the last `same` state we built children for — rebuild when this
    /// no longer matches `four.same.get()`.
    last_built_same: Cell<Option<bool>>,
}

/// Internal alias — a FourVal isn't `Clone`, so we wrap it in an `Rc` so both
/// the field and the caller can read/write the same values without taking
/// ownership.
struct FourValInner {
    vals: [Rc<Cell<f64>>; 4],
    same: Rc<Cell<bool>>,
}

impl FourValueField {
    fn new(four: &FourVal, labels: [&'static str; 4], font: Arc<Font>,
           min: f64, max: f64, speed: f64) -> Self {
        let inner = Rc::new(FourValInner {
            vals: [Rc::clone(&four.vals[0]), Rc::clone(&four.vals[1]),
                   Rc::clone(&four.vals[2]), Rc::clone(&four.vals[3])],
            same: Rc::clone(&four.same),
        });
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            font,
            font_size: 13.0,
            labels,
            four: inner,
            min, max, speed,
            last_built_same: Cell::new(None),
        }
    }

    fn rebuild(&mut self) {
        self.children.clear();
        let same = self.four.same.get();

        // Row 1 always: [checkbox "same"] [primary drag when same=true]
        let mut top_row = FlexRow::new().with_gap(6.0);
        let four_for_set = Rc::clone(&self.four);
        // Cap the checkbox width — its `layout` returns the full `available`
        // width, which would otherwise starve the DragValue (flex child) of
        // space when they share a FlexRow.
        let same_cb = Checkbox::new("same", Arc::clone(&self.font), same)
            .with_font_size(self.font_size)
            .with_max_size(Size::new(70.0, f64::MAX))
            .with_state_cell(Rc::clone(&self.four.same))
            .on_change(move |now_same| {
                if now_same {
                    // Collapse to the average of the four values.
                    let avg = (four_for_set.vals[0].get()
                             + four_for_set.vals[1].get()
                             + four_for_set.vals[2].get()
                             + four_for_set.vals[3].get()) / 4.0;
                    for c in &four_for_set.vals { c.set(avg); }
                }
            });
        top_row = top_row.add(Box::new(same_cb));

        if same {
            let v0 = Rc::clone(&self.four.vals[0]);
            let all = [Rc::clone(&self.four.vals[0]), Rc::clone(&self.four.vals[1]),
                       Rc::clone(&self.four.vals[2]), Rc::clone(&self.four.vals[3])];
            let dv = DragValue::new(v0.get(), self.min, self.max, Arc::clone(&self.font))
                .with_speed(self.speed)
                .with_decimals(0)
                .with_min_size(Size::new(70.0, 22.0))
                .on_change(move |v| {
                    for c in &all { c.set(v); }
                });
            top_row = top_row.add_flex(Box::new(dv), 1.0);
        }
        // When `same` is off the top row is just the checkbox — the four
        // component rows follow below.  (Intentionally no trailing `Spacer`
        // here: `Spacer::layout` returns the full `available` size, which
        // inflates FlexRow's cross-axis measurement and squishes sibling rows.)

        let mut col = FlexColumn::new().with_gap(4.0);
        col = col.add(Box::new(top_row));

        if !same {
            // Expanded: 4 rows, one per component.  Indent via Label's left
            // margin (don't use Spacer — see note above).
            for i in 0..4 {
                let cell = Rc::clone(&self.four.vals[i]);
                let dv = DragValue::new(cell.get(), self.min, self.max, Arc::clone(&self.font))
                    .with_speed(self.speed)
                    .with_decimals(0)
                    .with_min_size(Size::new(70.0, 22.0))
                    .on_change(move |v| cell.set(v));

                let lbl = Label::new(self.labels[i], Arc::clone(&self.font))
                    .with_font_size(self.font_size)
                    .with_margin(agg_gui::Insets { left: 20.0, right: 0.0, top: 0.0, bottom: 0.0 })
                    .with_min_size(Size::new(46.0, 0.0))
                    .with_max_size(Size::new(46.0, f64::MAX));
                let row = FlexRow::new()
                    .with_gap(6.0)
                    .add(Box::new(lbl))
                    .add_flex(Box::new(dv), 1.0);
                col = col.add(Box::new(row));
            }
        }

        self.children.push(Box::new(col));
        self.last_built_same.set(Some(same));
    }
}

impl Widget for FourValueField {
    fn type_name(&self) -> &'static str { "FourValueField" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size {
        // Rebuild if `same` changed (or first-layout).
        let cur = self.four.same.get();
        if self.last_built_same.get() != Some(cur) {
            self.rebuild();
        }

        if let Some(ch) = self.children.first_mut() {
            let s = ch.layout(available);
            ch.set_bounds(Rect::new(0.0, 0.0, s.width, s.height));
            self.bounds = Rect::new(0.0, 0.0, s.width, s.height);
            return s;
        }
        Size::new(0.0, 0.0)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        if let Some(ch) = self.children.first_mut() {
            let b = ch.bounds();
            ctx.save();
            ctx.translate(b.x, b.y);
            paint_subtree(ch.as_mut(), ctx);
            ctx.restore();
        }
    }

    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}

// ── Shadow editor — mini grid of 4 DragValues + colour picker ────────────────

fn shadow_editor(st: &Rc<FrameState>, font: Arc<Font>) -> Box<dyn Widget> {
    let dx_c = Rc::clone(&st.shadow_dx);
    let dy_c = Rc::clone(&st.shadow_dy);
    let bl_c = Rc::clone(&st.shadow_blur);
    let sp_c = Rc::clone(&st.shadow_spread);

    let dx = labeled_drag("x:",      dx_c.clone(), -100.0, 100.0, 1.0, 0, Arc::clone(&font));
    let dy = labeled_drag("y:",      dy_c.clone(), -100.0, 100.0, 1.0, 0, Arc::clone(&font));
    let bl = labeled_drag("blur:",   bl_c.clone(),    0.0, 100.0, 1.0, 0, Arc::clone(&font));
    let sp = labeled_drag("spread:", sp_c.clone(),    0.0, 100.0, 1.0, 0, Arc::clone(&font));

    let col_cell = Rc::clone(&st.shadow_col);
    let col_pick = ColorPicker::new(col_cell, Arc::clone(&font))
        .with_allow_none(false)
        .with_font_size(12.0);

    // `add_flex(1.0)` so x & y (and blur & spread) share the row's width
    // equally instead of each one claiming the full width.
    let row1 = FlexRow::new().with_gap(6.0).add_flex(dx, 1.0).add_flex(dy, 1.0);
    let row2 = FlexRow::new().with_gap(6.0).add_flex(bl, 1.0).add_flex(sp, 1.0);
    let col  = FlexColumn::new().with_gap(4.0)
        .add(Box::new(row1))
        .add(Box::new(row2))
        .add(Box::new(col_pick));

    Box::new(col)
}

fn labeled_drag(
    prefix: &'static str,
    cell:   Rc<Cell<f64>>,
    min:    f64,
    max:    f64,
    speed:  f64,
    decimals: usize,
    font:   Arc<Font>,
) -> Box<dyn Widget> {
    let c = Rc::clone(&cell);
    let row = FlexRow::new()
        .with_gap(4.0)
        .add(Box::new(
            Label::new(prefix, Arc::clone(&font))
                .with_font_size(12.0)
                .with_min_size(Size::new(40.0, 0.0))
                .with_max_size(Size::new(40.0, f64::MAX))
        ))
        .add_flex(Box::new(
            DragValue::new(cell.get(), min, max, font)
                .with_speed(speed)
                .with_decimals(decimals)
                .with_min_size(Size::new(60.0, 22.0))
                .on_change(move |v| c.set(v))
        ), 1.0);
    Box::new(row)
}

// ── Stroke editor: width + colour picker ─────────────────────────────────────

fn stroke_editor(st: &Rc<FrameState>, font: Arc<Font>) -> Box<dyn Widget> {
    let w_cell = Rc::clone(&st.stroke_w);
    let col_cell = Rc::clone(&st.stroke_col);
    let w_c = Rc::clone(&w_cell);
    // Width DragValue — max_size caps the preferred width so the FlexRow's
    // clamped_w math gives it a sane size instead of gobbling the full row.
    let dv = DragValue::new(w_cell.get(), 0.0, 20.0, Arc::clone(&font))
        .with_speed(0.1)
        .with_decimals(1)
        .with_min_size(Size::new(60.0, 22.0))
        .with_max_size(Size::new(70.0, f64::MAX))
        .on_change(move |v| w_c.set(v));
    let cp = ColorPicker::new(col_cell, Arc::clone(&font))
        .with_allow_none(false)
        .with_font_size(12.0);
    // Width on the left, colour picker (flex) takes the rest of the row.
    Box::new(FlexRow::new().with_gap(6.0)
        .add(Box::new(dv))
        .add_flex(Box::new(cp), 1.0))
}

// ── Full row: [label][field] ────────────────────────────────────────────────

fn labeled_row(label: &'static str, field: Box<dyn Widget>, font: Arc<Font>)
    -> Box<dyn Widget>
{
    Box::new(
        FlexRow::new()
            .with_gap(8.0)
            .add(Box::new(
                Label::new(label, Arc::clone(&font))
                    .with_font_size(13.0)
                    .with_min_size(Size::new(LABEL_W, 0.0))
                    .with_max_size(Size::new(LABEL_W, f64::MAX))
            ))
            .add_flex(field, 1.0)
    )
}

fn field_row(f: Box<dyn Widget>) -> Box<dyn Widget> {
    Box::new(FlexRow::new().add_flex(f, 1.0)
        .with_min_size(Size::new(FIELD_W, 0.0)))
}

// ── Main builder ─────────────────────────────────────────────────────────────

/// Build the Frame demo content widget (public entry point).
pub fn frame_demo(font: Arc<Font>) -> Box<dyn Widget> {
    let st = Rc::new(FrameState::defaults());

    // ── Left column: controls ───────────────────────────────────────────────
    let inner = FourValueField::new(&st.inner_m,
        ["Left", "Right", "Top", "Bottom"], Arc::clone(&font),
        0.0, 100.0, 1.0);
    let outer = FourValueField::new(&st.outer_m,
        ["Left", "Right", "Top", "Bottom"], Arc::clone(&font),
        0.0, 100.0, 1.0);
    let radius = FourValueField::new(&st.corner_r,
        ["NW", "NE", "SW", "SE"], Arc::clone(&font),
        0.0, 100.0, 1.0);

    let fill_pick = ColorPicker::new(Rc::clone(&st.fill), Arc::clone(&font))
        .with_font_size(12.0);

    let st_reset = Rc::clone(&st);
    let reset = Button::new("Reset", Arc::clone(&font))
        .on_click(move || st_reset.reset());

    let controls = FlexColumn::new()
        .with_gap(8.0)
        .with_padding(8.0)
        .with_min_size(Size::new(CONTROLS_W, 0.0))
        .with_max_size(Size::new(CONTROLS_W, f64::MAX))
        .with_v_anchor(VAnchor::FIT)
        .add(labeled_row("Inner margin",  field_row(Box::new(inner)),  Arc::clone(&font)))
        .add(labeled_row("Outer margin",  field_row(Box::new(outer)),  Arc::clone(&font)))
        .add(labeled_row("Corner radius", field_row(Box::new(radius)), Arc::clone(&font)))
        .add(labeled_row("Shadow",        field_row(shadow_editor(&st, Arc::clone(&font))), Arc::clone(&font)))
        .add(labeled_row("Fill",          field_row(Box::new(fill_pick)), Arc::clone(&font)))
        .add(labeled_row("Stroke",        field_row(stroke_editor(&st, Arc::clone(&font))), Arc::clone(&font)))
        .add(Box::new(reset));

    // ── Right column: live preview ──────────────────────────────────────────
    let preview = FramePreview {
        bounds:   Rect::default(),
        children: Vec::new(),
        st:       Rc::clone(&st),
        content:  Label::new("Content", Arc::clone(&font))
                      .with_font_size(13.0)
                      .with_color(Color::white()),
    };

    // ── Main row ────────────────────────────────────────────────────────────
    //
    // `IntrinsicRow` reports the SUM of its children widths (not `available`),
    // so `Window::with_auto_size` can grow / shrink the window to match the
    // preview's outer_margin size.
    Box::new(
        IntrinsicRow::new(8.0, 8.0, vec![Box::new(controls), Box::new(preview)])
    )
}
