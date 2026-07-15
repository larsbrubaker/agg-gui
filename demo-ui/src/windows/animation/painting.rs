//! The "Painting" freehand-drawing demo, matching egui's `Painting`
//! (`painting.rs`).
//!
//! Part of the [`super`] animation demo group.  Strokes are stored in
//! resolution-independent normalized coordinates (egui's `RectTransform` with
//! `square_proportions`) so the painting rescales when the window resizes.  A
//! stroke editor row (width + color swatches) and a Clear button mirror
//! egui's `ui_control`.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{
    Button, Color, DragValue, DrawCell, DrawCtx, Event, EventResult, FlexColumn, FlexRow, Font,
    Label, MouseButton, Point, Rect, Separator, Size, SizedBox, Widget,
};

// ---------------------------------------------------------------------------
// Coordinate normalization (egui `RectTransform` + `square_proportions`)
// ---------------------------------------------------------------------------
//
// egui stores paint strokes in a normalized space whose extent is the canvas
// rect's `square_proportions`: the larger axis spans 0..1 and the smaller axis
// spans 0..(aspect ratio).  Because that extent depends only on the aspect
// ratio (not the pixel size), a stored point maps to the same *fractional*
// screen location at any window size — strokes rescale on resize.

/// The normalized-space extent for a canvas of the given pixel size.
///
/// Returns `(sx, sy)` where the larger of `w`/`h` maps to `1.0` and the
/// smaller maps to its fraction — exactly egui's `Vec2::square_proportions`.
fn square_proportions(w: f64, h: f64) -> (f64, f64) {
    if w <= 0.0 || h <= 0.0 {
        (1.0, 1.0)
    } else if w > h {
        (w / h, 1.0)
    } else {
        (1.0, h / w)
    }
}

/// Map a normalized point into screen (local canvas) pixels.
fn to_screen(n: (f64, f64), w: f64, h: f64) -> (f64, f64) {
    let (sx, sy) = square_proportions(w, h);
    (n.0 / sx * w, n.1 / sy * h)
}

/// Map a screen (local canvas) pixel into normalized coordinates.
fn from_screen(p: (f64, f64), w: f64, h: f64) -> (f64, f64) {
    let (sx, sy) = square_proportions(w, h);
    (p.0 / w * sx, p.1 / h * sy)
}

// ---------------------------------------------------------------------------
// PaintCanvas
// ---------------------------------------------------------------------------

/// A freehand drawing canvas.
///
/// Each mouse-drag gesture creates a new stroke stored as a list of points in
/// normalized coordinates (see the module notes).  On paint, every stroke is
/// mapped back to screen pixels and replayed as a connected path using the
/// shared stroke width/color.
struct PaintCanvas {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    /// Completed and in-progress strokes, each a sequence of normalized
    /// (0..extent) points — resolution-independent so they rescale on resize.
    strokes: Vec<Vec<(f64, f64)>>,
    /// Whether the left mouse button is currently held inside the canvas.
    painting: bool,
    /// Shared stroke width in pixels (driven by the DragValue editor).
    /// `DrawCell` so the editor's `on_change` auto-requests a repaint on
    /// change — no manual `request_draw` needed in the closure.
    stroke_width: Rc<DrawCell<f64>>,
    /// Shared stroke color (driven by the swatch buttons).
    stroke_color: Rc<Cell<Color>>,
}

impl PaintCanvas {
    fn new(stroke_width: Rc<DrawCell<f64>>, stroke_color: Rc<Cell<Color>>) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            strokes: Vec::new(),
            painting: false,
            stroke_width,
            stroke_color,
        }
    }

    fn clear(&mut self) {
        self.strokes.clear();
        self.painting = false;
    }
}

impl Widget for PaintCanvas {
    fn type_name(&self) -> &'static str {
        "PaintCanvas"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
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
        let v = ctx.visuals();
        let w = self.bounds.width;
        let h = self.bounds.height;

        // Background.
        ctx.set_fill_color(v.widget_bg);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, w, h);
        ctx.fill();

        // Thin border.
        ctx.set_stroke_color(v.widget_stroke);
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, w, h);
        ctx.stroke();

        // Draw all strokes, mapping normalized points back to screen pixels.
        ctx.set_stroke_color(self.stroke_color.get());
        ctx.set_line_width(self.stroke_width.get().max(0.5));
        for stroke in &self.strokes {
            if stroke.len() < 2 {
                continue;
            }
            ctx.begin_path();
            let (fx, fy) = to_screen(stroke[0], w, h);
            ctx.move_to(fx, fy);
            for &p in &stroke[1..] {
                let (px, py) = to_screen(p, w, h);
                ctx.line_to(px, py);
            }
            ctx.stroke();
        }
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        let w = self.bounds.width;
        let h = self.bounds.height;
        match event {
            Event::MouseDown {
                pos,
                button: MouseButton::Left,
                ..
            } => {
                self.painting = true;
                self.strokes.push(vec![from_screen((pos.x, pos.y), w, h)]);
                agg_gui::animation::request_draw();
                EventResult::Consumed
            }
            Event::MouseMove { pos } => {
                if self.painting {
                    let n = from_screen((pos.x, pos.y), w, h);
                    if let Some(stroke) = self.strokes.last_mut() {
                        // Skip duplicate consecutive points, like egui.
                        if stroke.last() != Some(&n) {
                            stroke.push(n);
                            agg_gui::animation::request_draw();
                        }
                    }
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            Event::MouseUp {
                button: MouseButton::Left,
                ..
            } => {
                if self.painting {
                    self.painting = false;
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            _ => EventResult::Ignored,
        }
    }

    fn hit_test(&self, local_pos: Point) -> bool {
        local_pos.x >= 0.0
            && local_pos.x <= self.bounds.width
            && local_pos.y >= 0.0
            && local_pos.y <= self.bounds.height
    }
}

// ---------------------------------------------------------------------------
// ColorSwatch — a small clickable color square for the stroke editor
// ---------------------------------------------------------------------------

/// A compact color swatch button.  Clicking it writes `color` into the shared
/// stroke-color cell; the currently selected swatch draws a highlight border.
struct ColorSwatch {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    color: Color,
    selected: Rc<Cell<Color>>,
}

impl ColorSwatch {
    const SIZE: f64 = 20.0;

    fn new(color: Color, selected: Rc<Cell<Color>>) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            color,
            selected,
        }
    }
}

impl Widget for ColorSwatch {
    fn type_name(&self) -> &'static str {
        "ColorSwatch"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }

    fn layout(&mut self, _available: Size) -> Size {
        let s = Size::new(Self::SIZE, Self::SIZE);
        self.bounds = Rect::new(0.0, 0.0, s.width, s.height);
        s
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let w = self.bounds.width;
        let h = self.bounds.height;

        ctx.set_fill_color(self.color);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, w, h);
        ctx.fill();

        let selected = self.selected.get() == self.color;
        ctx.set_stroke_color(if selected { v.accent } else { v.widget_stroke });
        ctx.set_line_width(if selected { 2.5 } else { 1.0 });
        ctx.begin_path();
        ctx.rect(0.0, 0.0, w, h);
        ctx.stroke();
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        if let Event::MouseDown {
            button: MouseButton::Left,
            ..
        } = event
        {
            self.selected.set(self.color);
            agg_gui::animation::request_draw();
            EventResult::Consumed
        } else {
            EventResult::Ignored
        }
    }

    fn hit_test(&self, local_pos: Point) -> bool {
        local_pos.x >= 0.0
            && local_pos.x <= self.bounds.width
            && local_pos.y >= 0.0
            && local_pos.y <= self.bounds.height
    }
}

// ---------------------------------------------------------------------------
// PaintingRoot — wraps the canvas with the stroke editor toolbar
// ---------------------------------------------------------------------------

/// Top-level widget for the Painting demo.
///
/// Holds the toolbar (index 0) and the `PaintCanvas` (index 1), positioning
/// the toolbar at the top and the canvas below it (Y-up: canvas at y=0).
struct PaintingRoot {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>, // [0] = toolbar, [1] = PaintCanvas
}

impl PaintingRoot {
    fn new(toolbar: Box<dyn Widget>, canvas: Box<dyn Widget>) -> Self {
        Self {
            bounds: Rect::default(),
            children: vec![toolbar, canvas],
        }
    }
}

impl Widget for PaintingRoot {
    fn type_name(&self) -> &'static str {
        "PaintingRoot"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }

    fn layout(&mut self, available: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
        // Toolbar gets its natural height; canvas fills the rest.
        let toolbar_size = self.children[0].layout(Size::new(available.width, 80.0));
        let canvas_h = (available.height - toolbar_size.height).max(0.0);
        let canvas_size = self.children[1].layout(Size::new(available.width, canvas_h));
        // Position toolbar at top, canvas below (Y-up: canvas at y=0, toolbar above).
        self.children[1].set_bounds(Rect::new(0.0, 0.0, canvas_size.width, canvas_size.height));
        self.children[0].set_bounds(Rect::new(
            0.0,
            canvas_size.height,
            toolbar_size.width,
            toolbar_size.height,
        ));
        available
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        ctx.set_fill_color(v.panel_fill);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, self.bounds.width, self.bounds.height);
        ctx.fill();
    }

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// Canvas that polls the Clear flag on every layout, then delegates to the
/// inner `PaintCanvas`.  Because `PaintCanvas` owns its stroke list, the Clear
/// button toggles a shared flag that the canvas observes on the next pass.
struct ClearablePaintCanvas {
    inner: PaintCanvas,
    flag: Rc<Cell<bool>>,
}

impl Widget for ClearablePaintCanvas {
    fn type_name(&self) -> &'static str {
        "ClearablePaintCanvas"
    }
    fn bounds(&self) -> Rect {
        self.inner.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        self.inner.bounds = b;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.inner.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.inner.children
    }

    fn layout(&mut self, available: Size) -> Size {
        if self.flag.get() {
            self.inner.clear();
            self.flag.set(false);
        }
        self.inner.layout(available)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        self.inner.paint(ctx)
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        self.inner.on_event(event)
    }

    fn hit_test(&self, local_pos: Point) -> bool {
        self.inner.hit_test(local_pos)
    }
}

/// Build the Painting demo — a stroke editor row (width + color swatches, a
/// separator, and a Clear button) above the "Paint with your mouse/touch!"
/// hint and the canvas.
pub fn painting(font: Arc<Font>) -> Box<dyn Widget> {
    let clear_flag = Rc::new(Cell::new(false));
    // egui default stroke: width 1.0, color rgb(25, 200, 100).
    let default_color = Color::rgb(25.0 / 255.0, 200.0 / 255.0, 100.0 / 255.0);
    let stroke_width = Rc::new(DrawCell::new(1.0_f64));
    let stroke_color = Rc::new(Cell::new(default_color));

    // A small palette of stroke colors for the swatch row.
    let palette = [
        default_color,
        Color::rgb(200.0 / 255.0, 50.0 / 255.0, 50.0 / 255.0), // red
        Color::rgb(50.0 / 255.0, 110.0 / 255.0, 220.0 / 255.0), // blue
        Color::rgb(240.0 / 255.0, 200.0 / 255.0, 40.0 / 255.0), // yellow
        Color::rgb(240.0 / 255.0, 240.0 / 255.0, 240.0 / 255.0), // near-white
    ];

    // Stroke editor row: "Stroke:" | width | swatches | separator | Clear.
    let mut stroke_row = FlexRow::new()
        .with_gap(8.0)
        .with_padding(6.0)
        .add(Box::new(
            Label::new("Stroke:", Arc::clone(&font)).with_font_size(12.0),
        ))
        .add(Box::new(
            SizedBox::new()
                .with_width(70.0)
                .with_height(26.0)
                .with_child(Box::new(
                    DragValue::new(stroke_width.get(), 1.0, 20.0, Arc::clone(&font))
                        .with_decimals(0)
                        .with_step(1.0)
                        // `stroke_width` is a `DrawCell`, so `set` already
                        // requests a repaint when the value changes — the
                        // closure no longer needs a manual `request_draw`.
                        .on_change({
                            let stroke_width = Rc::clone(&stroke_width);
                            move |x| stroke_width.set(x)
                        }),
                )),
        ));
    for &c in &palette {
        stroke_row = stroke_row.add(Box::new(ColorSwatch::new(c, Rc::clone(&stroke_color))));
    }
    let stroke_row = stroke_row
        .add(Box::new(Separator::vertical()))
        .add(Box::new(
            SizedBox::new().with_height(26.0).with_child(Box::new(
                Button::new("Clear Painting", Arc::clone(&font))
                    .with_font_size(12.0)
                    .on_click({
                        let flag_for_btn = Rc::clone(&clear_flag);
                        move || {
                            flag_for_btn.set(true);
                            agg_gui::animation::request_draw();
                        }
                    }),
            )),
        ));

    let toolbar = Box::new(
        FlexColumn::new()
            .with_gap(6.0)
            .add(Box::new(stroke_row))
            .add(Box::new(
                Label::new("Paint with your mouse/touch!", Arc::clone(&font)).with_font_size(12.0),
            )),
    ) as Box<dyn Widget>;

    let canvas = Box::new(ClearablePaintCanvas {
        inner: PaintCanvas::new(Rc::clone(&stroke_width), Rc::clone(&stroke_color)),
        flag: Rc::clone(&clear_flag),
    });

    Box::new(PaintingRoot::new(toolbar, canvas))
}

#[cfg(test)]
mod painting_tests {
    use super::*;

    /// `to_screen` must invert `from_screen` for arbitrary canvas sizes.
    #[test]
    fn normalize_round_trip() {
        for &(w, h) in &[(300.0, 300.0), (512.0, 384.0), (200.0, 500.0)] {
            for &(x, y) in &[(0.0, 0.0), (50.0, 120.0), (w, h), (w * 0.3, h * 0.7)] {
                let n = from_screen((x, y), w, h);
                let (sx, sy) = to_screen(n, w, h);
                assert!(
                    (sx - x).abs() < 1e-9 && (sy - y).abs() < 1e-9,
                    "round-trip failed for ({x},{y}) at {w}x{h}: got ({sx},{sy})"
                );
            }
        }
    }

    /// A screen point normalized at one size must map back to the same
    /// *fractional* location after an aspect-preserving resize.
    #[test]
    fn resize_invariance() {
        let (w1, h1) = (300.0, 200.0);
        let point = (90.0, 60.0); // 30% across, 30% down
        let n = from_screen(point, w1, h1);

        // Scale the canvas by 2x (aspect ratio preserved).
        let (w2, h2) = (600.0, 400.0);
        let (sx, sy) = to_screen(n, w2, h2);
        assert!(
            (sx - 180.0).abs() < 1e-9 && (sy - 120.0).abs() < 1e-9,
            "expected (180,120) after 2x resize, got ({sx},{sy})"
        );

        // The normalized coordinate itself is scale-independent for the same
        // aspect ratio: normalizing the scaled point yields the same value.
        let n2 = from_screen((180.0, 120.0), w2, h2);
        assert!((n2.0 - n.0).abs() < 1e-9 && (n2.1 - n.1).abs() < 1e-9);
    }

    /// The larger axis of `square_proportions` is always 1.0.
    #[test]
    fn square_proportions_larger_axis_is_one() {
        assert_eq!(square_proportions(400.0, 200.0), (2.0, 1.0));
        assert_eq!(square_proportions(200.0, 400.0), (1.0, 2.0));
        assert_eq!(square_proportions(300.0, 300.0), (1.0, 1.0));
    }
}
