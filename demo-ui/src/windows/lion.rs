//! Lion demo — renders the classic AGG lion via the halo-AA pipeline.
//!
//! # Proof of halo-AA correctness + tess2 numerical stability
//!
//! The lion's ~130 coloured polygons go through the full path every frame:
//! the raw polygon coords are rotated / scaled / skewed, fed to
//! `tessellate_path_aa` (which runs tess2 fresh each frame), and the
//! resulting triangles + edge-flag halo strips are submitted to the AA
//! solid shader.  MSAA is explicitly **disabled** on the GL context (no
//! `with_multisampling` in `demo-native/src/main.rs`), so every smooth
//! silhouette pixel you see is coming from the halo strips — analytic
//! edge-coverage, not hardware supersampling.
//!
//! Per-frame re-tessellation is the way libtess2 was designed to be used
//! — that's SGI's whole point, numerically-stable triangulation across
//! arbitrary transforms.  The `tess2-rust` rotation-stability tests
//! (`tests/lion_polygons.rs`) pin that down so dragging never flips the
//! polygon topology.
//!
//! # Interaction
//!
//! Matches the C++ `agg/examples/lion.cpp` reference exactly:
//!   - **Left-drag**: rotate + scale (angle = `atan2(dy, dx)`,
//!     scale = distance / 100).
//!   - **Right-drag**: skew (skew_x = cursor.x, skew_y = cursor.y, divided
//!     by 1000 before entering the affine).
//!   - **Alpha slider** above the lion.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{
    Color, DrawCtx, Event, EventResult, Font, Label, MouseButton, Point, Rect,
    Size, Slider, Widget,
};
use agg_gui::layout_props::{HAnchor, VAnchor, WidgetBase};

// ── Path data ────────────────────────────────────────────────────────────────

/// One coloured sub-polygon in local (mirrored) lion coords.
#[derive(Clone)]
struct LionPath {
    verts: Vec<[f64; 2]>,
    color: Color,
}

/// Parse the AGG lion data blob into a list of coloured sub-paths.
///
/// `lion.txt` is SVG-style Y-down with a horizontal mirror implicitly
/// performed by the C demo's `rotate(angle + PI)` — we bake both transforms
/// into the parsed coordinates so the rest of the widget can treat the data
/// as straightforward Y-up local coords.
fn parse_lion() -> (Vec<LionPath>, (f64, f64, f64, f64)) {
    const DATA: &str = include_str!("lion.txt");
    let mut out: Vec<LionPath> = Vec::new();
    let mut cur_color = Color::black();
    let mut min_x =  f64::INFINITY;
    let mut min_y =  f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;

    for raw in DATA.lines() {
        let line = raw.trim();
        if line.is_empty() { continue; }

        if line.len() == 6 && line.chars().all(|c| c.is_ascii_hexdigit()) {
            let v = u32::from_str_radix(line, 16).unwrap_or(0);
            let r = ((v >> 16) & 0xFF) as f32 / 255.0;
            let g = ((v >>  8) & 0xFF) as f32 / 255.0;
            let b = ( v        & 0xFF) as f32 / 255.0;
            cur_color = Color::rgb(r, g, b);
            continue;
        }

        if line.starts_with('M') {
            let mut verts: Vec<[f64; 2]> = Vec::new();
            for tok in line.split_whitespace() {
                if tok == "M" || tok == "L" { continue; }
                if let Some((x, y)) = parse_coord(tok) {
                    verts.push([x, y]);
                    if x < min_x { min_x = x; }
                    if y < min_y { min_y = y; }
                    if x > max_x { max_x = x; }
                    if y > max_y { max_y = y; }
                }
            }
            if verts.len() >= 3 {
                out.push(LionPath { verts, color: cur_color });
            }
        }
    }

    // Horizontal mirror + Y-up flip (mirror about the bounding-box midpoint).
    let mid_x = (min_x + max_x) * 0.5;
    let mid_y = (min_y + max_y) * 0.5;
    for p in &mut out {
        for v in &mut p.verts {
            v[0] = 2.0 * mid_x - v[0];
            v[1] = 2.0 * mid_y - v[1];
        }
    }

    (out, (min_x, min_y, max_x, max_y))
}

fn parse_coord(s: &str) -> Option<(f64, f64)> {
    let mut it = s.split(',');
    let x: f64 = it.next()?.parse().ok()?;
    let y: f64 = it.next()?.parse().ok()?;
    Some((x, y))
}

// ── Widget ───────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq)]
enum Drag { None, Rotate, Skew }

struct LionView {
    bounds:   Rect,
    children: Vec<Box<dyn Widget>>,
    base:     WidgetBase,

    paths:    Vec<LionPath>,
    bbox:     (f64, f64, f64, f64),

    angle:    f64,
    mouse_scale: f64,
    skew_x:   f64,
    skew_y:   f64,
    alpha:    Rc<Cell<f64>>,
    drag:     Drag,
}

impl LionView {
    fn new(alpha: Rc<Cell<f64>>) -> Self {
        let (paths, bbox) = parse_lion();
        Self {
            bounds:   Rect::default(),
            children: Vec::new(),
            base:     WidgetBase::new(),
            paths,
            bbox,
            angle:    0.0,
            mouse_scale: 1.0,
            skew_x:   0.0,
            skew_y:   0.0,
            alpha,
            drag:     Drag::None,
        }
    }

    fn fit_scale(&self, w: f64, h: f64) -> f64 {
        let (min_x, min_y, max_x, max_y) = self.bbox;
        let lw = (max_x - min_x).max(1e-6);
        let lh = (max_y - min_y).max(1e-6);
        let pad = 10.0;
        let sx = (w - pad * 2.0) / lw;
        let sy = (h - pad * 2.0) / lh;
        sx.min(sy).max(0.01)
    }

    fn apply_rotate(&mut self, pos: Point) {
        let cx = self.bounds.width  * 0.5;
        let cy = self.bounds.height * 0.5;
        let dx = pos.x - cx;
        let dy = pos.y - cy;
        self.angle = dy.atan2(dx);
        self.mouse_scale = (dx * dx + dy * dy).sqrt() / 100.0;
    }

    fn apply_skew(&mut self, pos: Point) {
        self.skew_x = pos.x;
        self.skew_y = pos.y;
    }
}

impl Widget for LionView {
    fn type_name(&self) -> &'static str { "LionView" }
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
        self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
        available
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let w = self.bounds.width;
        let h = self.bounds.height;
        if w < 4.0 || h < 4.0 { return; }

        // Background card.
        let v = ctx.visuals();
        ctx.set_fill_color(v.panel_fill);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, w, h);
        ctx.fill();

        let (min_x, min_y, max_x, max_y) = self.bbox;
        let cx_lion = (min_x + max_x) * 0.5;
        let cy_lion = (min_y + max_y) * 0.5;
        let cx_widget = w * 0.5;
        let cy_widget = h * 0.5;

        let base_scale = self.fit_scale(w, h);
        let scale      = base_scale * self.mouse_scale;
        let (sin_a, cos_a) = self.angle.sin_cos();
        let skew_x = self.skew_x / 1000.0;
        let skew_y = self.skew_y / 1000.0;
        let alpha = self.alpha.get().clamp(0.0, 1.0);
        ctx.set_global_alpha(alpha);

        // Fresh tessellation every frame: emit each polygon through the
        // path API, let `do_fill` route it through `tessellate_path_aa`.
        // This is the load tess2 was designed for — running on rotated
        // floats every frame and producing topologically identical output.
        for path in &self.paths {
            ctx.set_fill_color(path.color);
            ctx.begin_path();
            let mut first = true;
            for &[x0, y0] in &path.verts {
                let px = (x0 - cx_lion) * scale;
                let py = (y0 - cy_lion) * scale;
                let rx = px * cos_a - py * sin_a;
                let ry = px * sin_a + py * cos_a;
                let sx = rx + ry * skew_x;
                let sy = ry + rx * skew_y;
                let fx = sx + cx_widget;
                let fy = sy + cy_widget;
                if first { ctx.move_to(fx, fy); first = false; }
                else     { ctx.line_to(fx, fy); }
            }
            ctx.close_path();
            ctx.fill();
        }

        ctx.set_global_alpha(1.0);
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseDown { button, pos, .. } => {
                match button {
                    MouseButton::Left  => { self.drag = Drag::Rotate; self.apply_rotate(*pos); }
                    MouseButton::Right => { self.drag = Drag::Skew;   self.apply_skew(*pos); }
                    _ => return EventResult::Ignored,
                }
                agg_gui::animation::request_tick();
                EventResult::Consumed
            }
            Event::MouseMove { pos } => {
                match self.drag {
                    Drag::Rotate => { self.apply_rotate(*pos); }
                    Drag::Skew   => { self.apply_skew(*pos); }
                    Drag::None   => return EventResult::Ignored,
                }
                agg_gui::animation::request_tick();
                EventResult::Consumed
            }
            Event::MouseUp { .. } => {
                let was = self.drag != Drag::None;
                self.drag = Drag::None;
                if was { EventResult::Consumed } else { EventResult::Ignored }
            }
            _ => EventResult::Ignored,
        }
    }
}

// ── Demo window entry point ──────────────────────────────────────────────────

pub fn lion_demo(font: Arc<Font>) -> Box<dyn Widget> {
    use agg_gui::FlexColumn;

    let alpha = Rc::new(Cell::new(1.0f64));
    let alp_c = Rc::clone(&alpha);
    let alp_slider = Slider::new(1.0, 0.0, 1.0, Arc::clone(&font))
        .on_change(move |v| alp_c.set(v));

    let alp_label = Label::new("Alpha", Arc::clone(&font)).with_font_size(12.0);
    let note = Label::new(
        "Left-drag: rotate + scale.  Right-drag: skew.  MSAA is off; \
         smooth silhouette = halo-AA edges; fresh tess2 every frame.",
        Arc::clone(&font)
    ).with_font_size(11.0);

    let view = LionView::new(alpha);

    Box::new(
        FlexColumn::new()
            .with_gap(6.0)
            .with_padding(8.0)
            .add(Box::new(alp_label))
            .add(Box::new(alp_slider))
            .add(Box::new(note))
            .add_flex(Box::new(view), 1.0)
    )
}
