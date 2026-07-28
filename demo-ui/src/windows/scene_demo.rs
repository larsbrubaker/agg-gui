//! Scene demo — a pan/zoom canvas built on the framework's [`Scene`] container.
//!
//! Mirrors egui's `scene.rs` demo: a `Scene` with the default `0.1..=2.0` zoom
//! range hosts mixed interactive content (buttons, labels, and a text field)
//! alongside a few painted shapes, a live `scene_rect` readout, a "Reset view"
//! button, and double-click-to-reset on the background.  Replaces the old
//! static hover-highlight `SceneWidget` that lived in `interaction.rs`.
//!
//! The content lives in a small [`SceneCanvas`] container that positions its
//! child widgets at explicit scene-space coordinates and paints backdrop
//! shapes; `Scene` injects the pan/zoom through the framework's
//! `Widget::child_transform` hook, so the hosted widgets stay clickable —
//! and keyboard-focusable — at any zoom (the text field accepts typing after a
//! click or Tab).

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{
    Button, Color, DrawCtx, Event, EventResult, FlexColumn, FlexRow, Font, Label, Point, Rect,
    Scene, Separator, Size, SizedBox, TextField, Widget,
};

/// Fixed scene-space extent of the demo content.
const CONTENT_W: f64 = 420.0;
const CONTENT_H: f64 = 300.0;

/// Build the Scene demo window content.
pub fn scene_demo(font: Arc<Font>) -> Box<dyn Widget> {
    let scene_rect_cell: Rc<Cell<Rect>> = Rc::new(Cell::new(Rect::new(0.0, 0.0, 0.0, 0.0)));
    let reset_cell: Rc<Cell<bool>> = Rc::new(Cell::new(false));
    let counter: Rc<Cell<i32>> = Rc::new(Cell::new(0));

    let canvas = SceneCanvas::new(Arc::clone(&font), Rc::clone(&counter));
    let scene = Scene::new(Box::new(canvas))
        .with_zoom_range(0.1, 2.0)
        .with_content_size(Size::new(CONTENT_W, CONTENT_H))
        .with_default_scene_rect(Rect::new(0.0, 0.0, CONTENT_W, CONTENT_H))
        .with_scene_rect_cell(Rc::clone(&scene_rect_cell))
        .with_reset_cell(Rc::clone(&reset_cell));

    // ── Toolbar row: reset button + live scene_rect readout ──
    let reset_for_btn = Rc::clone(&reset_cell);
    let reset_btn = SizedBox::new().with_height(28.0).with_child(Box::new(
        Button::new("\u{F021} Reset view", Arc::clone(&font))
            .with_font_size(12.0)
            .on_click(move || {
                reset_for_btn.set(true);
                agg_gui::animation::request_draw();
            }),
    ));

    let readout_cell = Rc::clone(&scene_rect_cell);
    let readout = CellLabel::new(Arc::clone(&font), 11.5, move || {
        let r = readout_cell.get();
        format!(
            "scene_rect: [x {:.0}, y {:.0}, w {:.0}, h {:.0}]",
            r.x, r.y, r.width, r.height
        )
    });

    let mut toolbar = FlexRow::new().with_gap(10.0);
    toolbar.push(Box::new(reset_btn), 0.0);
    toolbar.push(Box::new(readout), 1.0);

    // ── Assemble the window column ──
    let mut col = FlexColumn::new()
        .with_gap(8.0)
        .with_padding(8.0)
        .with_panel_bg();
    col.push(
        Box::new(
            Label::new(
                "Pan: drag background or middle-drag.  Zoom: scroll wheel (anchored on cursor).",
                Arc::clone(&font),
            )
            .with_font_size(11.5),
        ),
        0.0,
    );
    col.push(
        Box::new(
            Label::new(
                "Double-click the background to reset the view.  Buttons stay interactive at any zoom.",
                Arc::clone(&font),
            )
            .with_font_size(11.0),
        ),
        0.0,
    );
    col.push(Box::new(toolbar), 0.0);
    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(Box::new(scene), 1.0);
    Box::new(col)
}

// ---------------------------------------------------------------------------
// SceneCanvas — the scene-space content: positioned widgets + painted shapes.
// ---------------------------------------------------------------------------

/// Container that places its child widgets at explicit scene-space coordinates
/// and paints a few backdrop shapes.  Reports a fixed natural size so the
/// hosting [`Scene`] has a stable rect to fit / reset to.
///
/// Children live in the normal `children` vec, so within this subtree they use
/// the framework's ordinary translation-based paint + hit-test — the pan/zoom
/// scale is already baked into the `DrawCtx` (for paint) and into the scene
/// coordinates (for input) by the parent `Scene`.
struct SceneCanvas {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    /// Scene-space bottom-left placement for each child, parallel to `children`.
    positions: Vec<Point>,
    /// Backdrop rectangles: `(x, y, w, h)` in scene coords.
    rects: Vec<(f64, f64, f64, f64)>,
    /// Backdrop circles: `(cx, cy, r)` in scene coords.
    circles: Vec<(f64, f64, f64)>,
}

impl SceneCanvas {
    fn new(font: Arc<Font>, counter: Rc<Cell<i32>>) -> Self {
        let mut children: Vec<Box<dyn Widget>> = Vec::new();
        let mut positions: Vec<Point> = Vec::new();

        // Title label.
        children.push(Box::new(
            Label::new("Scene content", Arc::clone(&font)).with_font_size(16.0),
        ));
        positions.push(Point::new(20.0, CONTENT_H - 34.0));

        // A button that increments a shared counter — proves interactivity
        // survives the pan/zoom transform.
        let inc = Rc::clone(&counter);
        children.push(Box::new(
            SizedBox::new()
                .with_width(130.0)
                .with_height(30.0)
                .with_child(Box::new(
                    Button::new("\u{F067} Increment", Arc::clone(&font))
                        .with_font_size(13.0)
                        .on_click(move || {
                            inc.set(inc.get() + 1);
                            agg_gui::animation::request_draw();
                        }),
                )),
        ));
        positions.push(Point::new(20.0, CONTENT_H - 80.0));

        // A button that resets the counter.
        let reset = Rc::clone(&counter);
        children.push(Box::new(
            SizedBox::new()
                .with_width(90.0)
                .with_height(30.0)
                .with_child(Box::new(
                    Button::new("\u{F0E2} Zero", Arc::clone(&font))
                        .with_font_size(13.0)
                        .on_click(move || {
                            reset.set(0);
                            agg_gui::animation::request_draw();
                        }),
                )),
        ));
        positions.push(Point::new(165.0, CONTENT_H - 80.0));

        // Live counter readout.
        let count_src = Rc::clone(&counter);
        children.push(Box::new(CellLabel::new(
            Arc::clone(&font),
            13.0,
            move || format!("Clicks: {}", count_src.get()),
        )));
        positions.push(Point::new(20.0, CONTENT_H - 120.0));

        // A text field — proves keyboard focus reaches inside the Scene: click
        // it (or Tab to it) at any zoom and typed characters land here, because
        // the content is a first-class framework child under the Scene's
        // pan/zoom transform.
        children.push(Box::new(
            SizedBox::new()
                .with_width(200.0)
                .with_height(30.0)
                .with_child(Box::new(
                    TextField::new(Arc::clone(&font))
                        .with_font_size(13.0)
                        .with_placeholder("Type here (focus works!)"),
                )),
        ));
        positions.push(Point::new(20.0, CONTENT_H - 160.0));

        // A hint label near the shapes.
        children.push(Box::new(
            Label::new("Drag empty space to pan", Arc::clone(&font)).with_font_size(11.0),
        ));
        positions.push(Point::new(20.0, 24.0));

        Self {
            bounds: Rect::default(),
            children,
            positions,
            rects: vec![(250.0, 150.0, 90.0, 60.0), (300.0, 60.0, 70.0, 40.0)],
            circles: vec![
                (300.0, 200.0, 34.0),
                (360.0, 120.0, 22.0),
                (250.0, 90.0, 16.0),
            ],
        }
    }
}

impl Widget for SceneCanvas {
    fn type_name(&self) -> &'static str {
        "SceneCanvas"
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
        // Fixed scene extent regardless of the slot the Scene offers.
        self.bounds = Rect::new(0.0, 0.0, CONTENT_W, CONTENT_H);
        for i in 0..self.children.len() {
            let pos = self.positions[i];
            let natural = self.children[i].layout(Size::new(180.0, 40.0));
            self.children[i].set_bounds(Rect::new(pos.x, pos.y, natural.width, natural.height));
        }
        Size::new(CONTENT_W, CONTENT_H)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();

        // Canvas panel + border so the scene extent is visible while panning.
        ctx.set_fill_color(v.panel_fill);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, CONTENT_W, CONTENT_H);
        ctx.fill();
        ctx.set_stroke_color(v.separator);
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, CONTENT_W, CONTENT_H);
        ctx.stroke();

        // Backdrop rectangles.
        for &(x, y, w, h) in &self.rects {
            ctx.set_fill_color(v.widget_bg);
            ctx.begin_path();
            ctx.rounded_rect(x, y, w, h, 4.0);
            ctx.fill();
            ctx.set_stroke_color(v.widget_stroke);
            ctx.begin_path();
            ctx.rounded_rect(x, y, w, h, 4.0);
            ctx.stroke();
        }

        // Backdrop circles (accent-tinted).
        for &(cx, cy, r) in &self.circles {
            ctx.set_fill_color(Color::rgba(v.accent.r, v.accent.g, v.accent.b, 0.55));
            ctx.begin_path();
            ctx.circle(cx, cy, r);
            ctx.fill();
        }
    }

    fn on_event(&mut self, _event: &Event) -> EventResult {
        // Children handle their own events via the framework traversal the
        // parent Scene drives; the canvas itself is inert (empty space pans).
        EventResult::Ignored
    }
}

// ---------------------------------------------------------------------------
// CellLabel — a Label whose text is recomputed from a closure every layout.
// ---------------------------------------------------------------------------

/// One-child wrapper around a [`Label`] that refreshes its text from `source`
/// on each layout pass, updating the label only when the string changes (so the
/// backbuffer cache invalidates just once per real change).  Used for the live
/// `scene_rect` and click-counter readouts.
struct CellLabel {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    source: Box<dyn Fn() -> String>,
    last: String,
}

impl CellLabel {
    fn new(font: Arc<Font>, font_size: f64, source: impl Fn() -> String + 'static) -> Self {
        let initial = source();
        let label = Label::new(initial.clone(), font).with_font_size(font_size);
        Self {
            bounds: Rect::default(),
            children: vec![Box::new(label)],
            source: Box::new(source),
            last: initial,
        }
    }
}

impl Widget for CellLabel {
    fn type_name(&self) -> &'static str {
        "CellLabel"
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
        let text = (self.source)();
        if text != self.last {
            self.children[0].set_label_text(&text);
            self.last = text;
        }
        let sz = self.children[0].layout(available);
        self.children[0].set_bounds(Rect::new(0.0, 0.0, sz.width, sz.height));
        self.bounds = Rect::new(0.0, 0.0, sz.width, sz.height);
        sz
    }

    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}

    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}
