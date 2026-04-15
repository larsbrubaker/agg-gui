//! Test window implementations for all 11 egui test windows.
//!
//! These are diagnostic/test widgets that verify framework behaviour.  Where
//! native capabilities (clipboard, OS cursors, SVG) are not yet wired up, a
//! clear informational placeholder is shown instead of broken code.

use std::sync::Arc;

use agg_gui::{
    Color, Container, DrawCtx, Event, EventResult,
    FlexColumn, FlexRow, Font, Label,
    Point, Rect, Separator,
    Size, SizedBox, TextField, Widget,
};
use agg_gui::widget::paint_subtree;

// ---------------------------------------------------------------------------
// Clipboard Test
// ---------------------------------------------------------------------------

/// Build the Clipboard Test — two side-by-side TextFields for copy/paste.
pub fn clipboard_test(font: Arc<Font>) -> Box<dyn Widget> {
    let mut col = FlexColumn::new()
        .with_gap(14.0)
        .with_padding(16.0)
        .with_panel_bg();

    col.push(Box::new(Label::new("Clipboard test", Arc::clone(&font))
        .with_font_size(12.0)), 0.0);

    let row = FlexRow::new().with_gap(10.0)
        .add_flex(Box::new(FlexColumn::new().with_gap(6.0)
            .add(Box::new(Label::new("Copy from:", Arc::clone(&font)).with_font_size(11.5)))
            .add(Box::new(SizedBox::new().with_height(32.0).with_child(Box::new(
                TextField::new(Arc::clone(&font))
                    .with_font_size(13.0).with_text("Select and copy me")
            ))))), 1.0)
        .add_flex(Box::new(FlexColumn::new().with_gap(6.0)
            .add(Box::new(Label::new("Paste into:", Arc::clone(&font)).with_font_size(11.5)))
            .add(Box::new(SizedBox::new().with_height(32.0).with_child(Box::new(
                TextField::new(Arc::clone(&font))
                    .with_font_size(13.0).with_placeholder("Ctrl+V here")
            ))))), 1.0);
    col.push(Box::new(row), 0.0);

    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(Box::new(Label::new(
        "Ctrl+C / Ctrl+X — copy or cut selected text\n\
         Ctrl+V           — paste from clipboard\n\
         Ctrl+A           — select all",
        Arc::clone(&font),
    ).with_font_size(11.5)), 0.0);

    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);
    Box::new(col)
}

// ---------------------------------------------------------------------------
// Cursor Test
// ---------------------------------------------------------------------------

/// A labeled box showing a cursor-shape name.
struct CursorBox {
    bounds:   Rect,
    children: Vec<Box<dyn Widget>>,
    label_widget: Label,
    hovered:  bool,
}

impl CursorBox {
    fn new(name: &'static str, font: Arc<Font>) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            label_widget: Label::new(name, font).with_font_size(11.0),
            hovered: false,
        }
    }
}

impl Widget for CursorBox {
    fn type_name(&self) -> &'static str { "CursorBox" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, _available: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, 90.0, 50.0);
        let ls = self.label_widget.layout(Size::new(90.0, 50.0));
        // Center label within the 90×50 box.
        let lx = (90.0 - ls.width) * 0.5;
        let ly = (50.0 - ls.height) * 0.5;
        self.label_widget.set_bounds(Rect::new(lx, ly, ls.width, ls.height));
        Size::new(90.0, 50.0)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let bg = if self.hovered { v.widget_bg_hovered } else { v.widget_bg };
        ctx.set_fill_color(bg);
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, 90.0, 50.0, 5.0);
        ctx.fill();
        ctx.set_stroke_color(v.widget_stroke);
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, 90.0, 50.0, 5.0);
        ctx.stroke();

        // Paint label via backbuffered Label child.
        self.label_widget.set_color(v.text_color);
        let lb = self.label_widget.bounds();
        ctx.save();
        ctx.translate(lb.x, lb.y);
        paint_subtree(&mut self.label_widget, ctx);
        ctx.restore();
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseMove { pos } => {
                let was = self.hovered;
                self.hovered = pos.x >= 0.0 && pos.x <= 90.0
                    && pos.y >= 0.0 && pos.y <= 50.0;
                if self.hovered != was { EventResult::Consumed } else { EventResult::Ignored }
            }
            _ => EventResult::Ignored,
        }
    }

    fn hit_test(&self, p: Point) -> bool {
        p.x >= 0.0 && p.x <= 90.0 && p.y >= 0.0 && p.y <= 50.0
    }
}

/// Build the Cursor Test — a grid of cursor-shape name boxes.
pub fn cursor_test(font: Arc<Font>) -> Box<dyn Widget> {
    let cursor_names = [
        "Arrow", "Text", "Hand", "Crosshair",
        "Move", "NResize", "EResize", "Wait",
        "Progress", "NotAllowed", "ZoomIn", "ZoomOut",
    ];

    let mut col = FlexColumn::new()
        .with_gap(12.0)
        .with_padding(14.0)
        .with_panel_bg();

    col.push(Box::new(Label::new(
        "Cursor shapes (hover to see name)",
        Arc::clone(&font),
    ).with_font_size(12.0)), 0.0);

    // 4-column grid.
    for chunk in cursor_names.chunks(4) {
        let mut row = FlexRow::new().with_gap(8.0);
        for &name in chunk {
            row.push(Box::new(CursorBox::new(name, Arc::clone(&font))), 0.0);
        }
        col.push(Box::new(row), 0.0);
    }

    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(Box::new(Label::new(
        "Custom cursor shape API not yet wired to the OS layer.",
        Arc::clone(&font),
    ).with_font_size(11.0)), 0.0);

    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);
    Box::new(col)
}

// ---------------------------------------------------------------------------
// Grid Test
// ---------------------------------------------------------------------------

/// A custom-painted grid of colored cells showing alignment.
struct GridPainter {
    bounds:   Rect,
    children: Vec<Box<dyn Widget>>,
    font:     Arc<Font>,
    cols:     usize,
    rows:     usize,
}

impl Widget for GridPainter {
    fn type_name(&self) -> &'static str { "GridPainter" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
        available
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v   = ctx.visuals();
        let cw  = self.bounds.width  / self.cols as f64;
        let ch  = self.bounds.height / self.rows as f64;
        let n   = self.cols * self.rows;

        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(9.5);

        for idx in 0..n {
            let gc = idx % self.cols;
            let gr = idx / self.cols;
            // Y-up: row 0 is at the bottom.
            let x = gc as f64 * cw;
            let y = (self.rows - 1 - gr) as f64 * ch;

            let t = idx as f64 / n as f64;
            let r = (0.35 + t * 0.35) as f32;
            let g = (0.55 - t * 0.20) as f32;
            let b = (0.65 + t * 0.30) as f32;
            ctx.set_fill_color(Color::rgba(r, g, b, 0.60));
            ctx.begin_path();
            ctx.rect(x, y, cw - 1.0, ch - 1.0);
            ctx.fill();

            let label = format!("{},{}", gc, gr);
            ctx.set_fill_color(v.text_color);
            ctx.fill_text(&label, x + 3.0, y + ch * 0.35 + 4.0);
        }
    }

    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}

/// Build the Grid Test — an 8×6 colored grid with coordinate labels.
pub fn grid_test(font: Arc<Font>) -> Box<dyn Widget> {
    let mut col = FlexColumn::new()
        .with_gap(10.0)
        .with_padding(12.0)
        .with_panel_bg();

    col.push(Box::new(Label::new("8 × 6 alignment grid", Arc::clone(&font))
        .with_font_size(12.0)), 0.0);

    col.push(Box::new(GridPainter {
        bounds: Rect::default(), children: Vec::new(),
        font: Arc::clone(&font), cols: 8, rows: 6,
    }), 1.0);

    Box::new(col)
}

// ---------------------------------------------------------------------------
// Id Test
// ---------------------------------------------------------------------------

/// Build the Id Test — static informational display of widget type names.
pub fn id_test(font: Arc<Font>) -> Box<dyn Widget> {
    let types = [
        ("Button",         "btn_primary"),
        ("Checkbox",       "cb_feature_a"),
        ("Slider",         "slider_val_0"),
        ("TextField",      "tf_search"),
        ("Label",          "lbl_title"),
        ("FlexColumn",     "col_root"),
        ("FlexRow",        "row_buttons"),
        ("Container",      "container_panel"),
        ("ScrollView",     "scroll_main"),
        ("ProgressBar",    "pb_loading"),
    ];

    let mut col = FlexColumn::new()
        .with_gap(8.0)
        .with_padding(14.0)
        .with_panel_bg();

    col.push(Box::new(Label::new("Widget type → generated ID", Arc::clone(&font))
        .with_font_size(12.0)), 0.0);

    col.push(Box::new(Separator::horizontal()), 0.0);

    for (ty, id) in types {
        let row = FlexRow::new().with_gap(8.0)
            .add(Box::new(SizedBox::new().with_width(120.0).with_child(Box::new(
                Label::new(ty, Arc::clone(&font)).with_font_size(12.5)
            ))))
            .add(Box::new(Label::new(id, Arc::clone(&font))
                .with_font_size(12.0)
                .with_color(Color::rgb(0.22, 0.45, 0.88))));
        col.push(Box::new(row), 0.0);
    }

    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(Box::new(Label::new(
        "IDs are hashed from the widget type name + call-site path.",
        Arc::clone(&font),
    ).with_font_size(11.0)), 0.0);

    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);
    Box::new(col)
}

// ---------------------------------------------------------------------------
// Input Event History
// ---------------------------------------------------------------------------

/// Records the last N events and renders them as a scrollable list.
struct EventHistoryWidget {
    bounds:   Rect,
    children: Vec<Box<dyn Widget>>,
    font:     Arc<Font>,
    events:   Vec<String>,
    max:      usize,
}

impl EventHistoryWidget {
    fn new(font: Arc<Font>) -> Self {
        Self {
            bounds: Rect::default(), children: Vec::new(),
            font, events: Vec::new(), max: 20,
        }
    }

    fn push_event(&mut self, s: String) {
        self.events.insert(0, s);
        self.events.truncate(self.max);
    }
}

impl Widget for EventHistoryWidget {
    fn type_name(&self) -> &'static str { "EventHistoryWidget" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
        available
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v    = ctx.visuals();
        let w    = self.bounds.width;
        let h    = self.bounds.height;
        let line = 18.0_f64;

        ctx.set_fill_color(v.widget_bg);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, w, h);
        ctx.fill();
        ctx.set_stroke_color(v.widget_stroke);
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, w, h);
        ctx.stroke();

        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(11.0);

        for (i, ev) in self.events.iter().enumerate() {
            let y = h - (i as f64 + 1.0) * line;
            if y < 0.0 { break; }
            let alpha = 1.0 - i as f64 * 0.045;
            ctx.set_fill_color(Color::rgba(
                v.text_color.r, v.text_color.g, v.text_color.b, alpha as f32,
            ));
            ctx.fill_text(ev, 6.0, y + 4.0);
        }

        if self.events.is_empty() {
            ctx.set_fill_color(v.text_dim);
            ctx.set_font_size(11.0);
            ctx.fill_text("Interact to record events…", 8.0, h * 0.45 + 4.0);
        }
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        let desc = match event {
            Event::MouseMove { pos } =>
                format!("MouseMove ({:.0}, {:.0})", pos.x, pos.y),
            Event::MouseDown { pos, button, .. } =>
                format!("MouseDown {:?} ({:.0},{:.0})", button, pos.x, pos.y),
            Event::MouseUp { button, .. } =>
                format!("MouseUp {:?}", button),
            Event::KeyDown { key, .. } =>
                format!("KeyDown {:?}", key),
            Event::KeyUp { key, .. } =>
                format!("KeyUp {:?}", key),
            Event::MouseWheel { delta_y, .. } =>
                format!("MouseWheel {:.1}", delta_y),
            _ => return EventResult::Ignored,
        };
        self.push_event(desc);
        EventResult::Consumed
    }

    fn hit_test(&self, p: Point) -> bool {
        p.x >= 0.0 && p.x <= self.bounds.width
            && p.y >= 0.0 && p.y <= self.bounds.height
    }
}

/// Build the Input Event History — records and displays the last 20 events.
pub fn input_event_history(font: Arc<Font>) -> Box<dyn Widget> {
    let mut col = FlexColumn::new()
        .with_gap(8.0)
        .with_padding(10.0)
        .with_panel_bg();

    col.push(Box::new(Label::new(
        "Interact inside the box to record events (last 20)",
        Arc::clone(&font),
    ).with_font_size(11.5)), 0.0);

    col.push(Box::new(EventHistoryWidget::new(Arc::clone(&font))), 1.0);
    Box::new(col)
}

// ---------------------------------------------------------------------------
// Input Test
// ---------------------------------------------------------------------------

/// Records the last-pressed key name and mouse position.
struct InputStateWidget {
    bounds:      Rect,
    children:    Vec<Box<dyn Widget>>,
    font:        Arc<Font>,
    last_key:    Option<String>,
    mouse_pos:   Point,
}

impl Widget for InputStateWidget {
    fn type_name(&self) -> &'static str { "InputStateWidget" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size {
        let h = 100.0_f64.min(available.height);
        self.bounds = Rect::new(0.0, 0.0, available.width, h);
        Size::new(available.width, h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let w = self.bounds.width;
        let h = self.bounds.height;

        ctx.set_fill_color(v.widget_bg);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, w, h);
        ctx.fill();
        ctx.set_stroke_color(v.widget_stroke);
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, w, h);
        ctx.stroke();

        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(12.0);
        ctx.set_fill_color(v.text_color);

        let key_str = self.last_key.as_deref().unwrap_or("—");
        ctx.fill_text(&format!("Last key:   {}", key_str), 10.0, h - 20.0);
        ctx.fill_text(
            &format!("Mouse pos:  ({:.0}, {:.0})", self.mouse_pos.x, self.mouse_pos.y),
            10.0, h - 44.0,
        );
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseMove { pos } => {
                self.mouse_pos = *pos;
                EventResult::Consumed
            }
            Event::KeyDown { key, .. } => {
                self.last_key = Some(format!("{:?}", key));
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    fn hit_test(&self, p: Point) -> bool {
        p.x >= 0.0 && p.x <= self.bounds.width
            && p.y >= 0.0 && p.y <= self.bounds.height
    }
}

/// Build the Input Test — shows last key pressed and current mouse position.
pub fn input_test(font: Arc<Font>) -> Box<dyn Widget> {
    let mut col = FlexColumn::new()
        .with_gap(12.0)
        .with_padding(12.0)
        .with_panel_bg();

    col.push(Box::new(Label::new(
        "Move the mouse or press keys inside the status box",
        Arc::clone(&font),
    ).with_font_size(11.5)), 0.0);

    col.push(Box::new(InputStateWidget {
        bounds: Rect::default(), children: Vec::new(),
        font: Arc::clone(&font),
        last_key: None,
        mouse_pos: Point { x: 0.0, y: 0.0 },
    }), 0.0);

    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);
    Box::new(col)
}

// ---------------------------------------------------------------------------
// Layout Test
// ---------------------------------------------------------------------------

/// Build the Layout Test — colored boxes with alignment labels.
pub fn layout_test(font: Arc<Font>) -> Box<dyn Widget> {
    let labels = ["Left", "Center", "Right", "Stretch"];
    let colors = [
        Color::rgba(0.22, 0.45, 0.88, 0.25),
        Color::rgba(0.18, 0.72, 0.42, 0.25),
        Color::rgba(0.88, 0.25, 0.18, 0.25),
        Color::rgba(0.86, 0.78, 0.40, 0.25),
    ];

    let mut col = FlexColumn::new()
        .with_gap(12.0)
        .with_padding(14.0)
        .with_panel_bg();

    col.push(Box::new(Label::new("Alignment examples", Arc::clone(&font))
        .with_font_size(12.0)), 0.0);

    for (i, (&lbl, &bg)) in labels.iter().zip(colors.iter()).enumerate() {
        let box_w = match i {
            0 => 80.0,
            1 => 120.0,
            2 => 100.0,
            _ => 0.0, // stretch — use flex
        };

        let cell = Container::new()
            .with_background(bg)
            .with_border(Color::rgba(0.0, 0.0, 0.0, 0.15), 1.0)
            .with_padding(6.0)
            .add(Box::new(Label::new(lbl, Arc::clone(&font)).with_font_size(12.0)));

        if i == 3 {
            // Stretch row.
            let row = FlexRow::new().add_flex(Box::new(cell), 1.0);
            col.push(Box::new(row), 0.0);
        } else {
            let row = FlexRow::new()
                .add(Box::new(SizedBox::new().with_width(box_w).with_child(Box::new(cell))));
            col.push(Box::new(row), 0.0);
        }
    }

    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(Box::new(Label::new(
        "FlexRow / FlexColumn control alignment.\n\
         add() = fixed-size child, add_flex() = fills remaining space.",
        Arc::clone(&font),
    ).with_font_size(11.0)), 0.0);

    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);
    Box::new(col)
}

// ---------------------------------------------------------------------------
// Manual Layout Test
// ---------------------------------------------------------------------------

/// A custom-painted widget showing absolutely-positioned boxes with corner labels.
struct ManualLayoutWidget {
    bounds:   Rect,
    children: Vec<Box<dyn Widget>>,
    font:     Arc<Font>,
}

impl Widget for ManualLayoutWidget {
    fn type_name(&self) -> &'static str { "ManualLayoutWidget" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
        available
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let w = self.bounds.width;
        let h = self.bounds.height;

        // Background.
        ctx.set_fill_color(v.bg_color);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, w, h);
        ctx.fill();

        ctx.set_font(Arc::clone(&self.font));

        // Absolutely-positioned boxes.
        let boxes: &[(f64, f64, f64, f64, Color, &str)] = &[
            (10.0,  h - 60.0, 80.0, 40.0, Color::rgba(0.22, 0.45, 0.88, 0.25), "TL"),
            (w - 90.0, h - 60.0, 80.0, 40.0, Color::rgba(0.18, 0.72, 0.42, 0.25), "TR"),
            (10.0,  20.0, 80.0, 40.0, Color::rgba(0.88, 0.25, 0.18, 0.25), "BL"),
            (w - 90.0, 20.0, 80.0, 40.0, Color::rgba(0.86, 0.78, 0.40, 0.25), "BR"),
            ((w - 100.0) * 0.5, (h - 50.0) * 0.5, 100.0, 50.0,
             Color::rgba(0.60, 0.25, 0.88, 0.20), "Center"),
        ];

        for &(bx, by, bw, bh, bg, label) in boxes {
            ctx.set_fill_color(bg);
            ctx.begin_path();
            ctx.rounded_rect(bx, by, bw, bh, 4.0);
            ctx.fill();
            ctx.set_stroke_color(v.widget_stroke);
            ctx.set_line_width(1.0);
            ctx.begin_path();
            ctx.rounded_rect(bx, by, bw, bh, 4.0);
            ctx.stroke();
            ctx.set_font_size(11.0);
            ctx.set_fill_color(v.text_color);
            ctx.fill_text(label, bx + 5.0, by + bh * 0.4 + 4.0);
            // Coordinate label.
            ctx.set_font_size(8.5);
            ctx.set_fill_color(v.text_dim);
            ctx.fill_text(&format!("({:.0},{:.0})", bx, by), bx + 5.0, by + 9.0);
        }
    }

    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}

/// Build the Manual Layout Test — five absolutely positioned boxes.
pub fn manual_layout_test(font: Arc<Font>) -> Box<dyn Widget> {
    let mut col = FlexColumn::new()
        .with_gap(8.0)
        .with_padding(8.0)
        .with_panel_bg();

    col.push(Box::new(Label::new(
        "Absolutely-positioned boxes with coordinate labels",
        Arc::clone(&font),
    ).with_font_size(11.5)), 0.0);

    col.push(Box::new(ManualLayoutWidget {
        bounds: Rect::default(), children: Vec::new(), font,
    }), 1.0);

    Box::new(col)
}

// ---------------------------------------------------------------------------
// SVG Test
// ---------------------------------------------------------------------------

/// Build the SVG Test — informational placeholder with a drawn rectangle.
pub fn svg_test(font: Arc<Font>) -> Box<dyn Widget> {
    let mut col = FlexColumn::new()
        .with_gap(12.0)
        .with_padding(16.0)
        .with_panel_bg();

    col.push(Box::new(Label::new(
        "SVG rendering not yet implemented.",
        Arc::clone(&font),
    ).with_font_size(13.0)), 0.0);

    col.push(Box::new(Label::new(
        "agg-gui uses a raster-based renderer (Anti-Grain Geometry).  SVG\n\
         support would require a full SVG parse + rasterize pipeline.\n\n\
         Placeholder shape below:",
        Arc::clone(&font),
    ).with_font_size(12.0)), 0.0);

    // A simple drawn placeholder.
    col.push(Box::new(SvgPlaceholder::new(Arc::clone(&font))), 0.0);

    Box::new(col)
}

/// A visual placeholder for SVG rendering (not yet implemented).
///
/// The "SVG placeholder" text is a backbuffered Label child.
struct SvgPlaceholder {
    bounds:   Rect,
    children: Vec<Box<dyn Widget>>,
    label:    Label,
}

impl SvgPlaceholder {
    fn new(font: Arc<Font>) -> Self {
        Self {
            bounds:   Rect::default(),
            children: Vec::new(),
            label:    Label::new("SVG placeholder", font).with_font_size(10.0),
        }
    }
}

impl Widget for SvgPlaceholder {
    fn type_name(&self) -> &'static str { "SvgPlaceholder" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size {
        let h = 80.0_f64.min(available.height);
        self.bounds = Rect::new(0.0, 0.0, available.width, h);

        // Position label at 12px from left, vertically near center.
        let ls = self.label.layout(Size::new(available.width - 16.0, 16.0));
        let ly = (h - ls.height) * 0.5;
        self.label.set_bounds(Rect::new(12.0, ly, ls.width, ls.height));

        Size::new(available.width, h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let w = self.bounds.width;
        let h = self.bounds.height;

        ctx.set_fill_color(v.widget_bg);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, w, h);
        ctx.fill();
        ctx.set_stroke_color(Color::rgba(0.22, 0.45, 0.88, 0.60));
        ctx.set_line_width(1.5);
        ctx.begin_path();
        ctx.rect(8.0, 8.0, w - 16.0, h - 16.0);
        ctx.stroke();
        // Diagonal cross.
        ctx.begin_path();
        ctx.move_to(8.0, 8.0);
        ctx.line_to(w - 8.0, h - 8.0);
        ctx.move_to(w - 8.0, 8.0);
        ctx.line_to(8.0, h - 8.0);
        ctx.stroke();

        // Paint label via backbuffered child.
        self.label.set_color(v.text_dim);
        let lb = self.label.bounds();
        ctx.save(); ctx.translate(lb.x, lb.y);
        paint_subtree(&mut self.label, ctx);
        ctx.restore();
    }

    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}

// ---------------------------------------------------------------------------
// Tessellation Test
// ---------------------------------------------------------------------------

/// A custom-painted widget showing circle approximations with N segments.
struct TessellationWidget {
    bounds:   Rect,
    children: Vec<Box<dyn Widget>>,
    font:     Arc<Font>,
}

impl Widget for TessellationWidget {
    fn type_name(&self) -> &'static str { "TessellationWidget" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
        available
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v   = ctx.visuals();
        let w   = self.bounds.width;
        let h   = self.bounds.height;

        ctx.set_fill_color(v.bg_color);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, w, h);
        ctx.fill();

        // 5 polygons: 3, 6, 12, 24, 48 segments.
        let segments_list = [3_usize, 6, 12, 24, 48];
        let count = segments_list.len() as f64;
        let r     = (h * 0.30).min(w / count / 2.0 - 10.0);
        let cy    = h * 0.55;

        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(10.0);

        for (i, &n) in segments_list.iter().enumerate() {
            let cx = w * (i as f64 + 0.5) / count;

            let t = i as f64 / (segments_list.len() - 1) as f64;
            let fill = Color::rgba(
                (0.3 + t * 0.4) as f32,
                (0.6 - t * 0.2) as f32,
                (0.9 - t * 0.4) as f32,
                0.70,
            );

            ctx.set_fill_color(fill);
            ctx.begin_path();
            for k in 0..n {
                let angle = k as f64 * std::f64::consts::TAU / n as f64
                    - std::f64::consts::FRAC_PI_2;
                let px = cx + r * angle.cos();
                let py = cy + r * angle.sin();
                if k == 0 { ctx.move_to(px, py); } else { ctx.line_to(px, py); }
            }
            ctx.fill();
            ctx.set_stroke_color(v.widget_stroke);
            ctx.set_line_width(1.0);
            ctx.begin_path();
            for k in 0..n {
                let angle = k as f64 * std::f64::consts::TAU / n as f64
                    - std::f64::consts::FRAC_PI_2;
                let px = cx + r * angle.cos();
                let py = cy + r * angle.sin();
                if k == 0 { ctx.move_to(px, py); } else { ctx.line_to(px, py); }
            }
            ctx.stroke();

            // Label.
            ctx.set_fill_color(v.text_dim);
            let label = format!("n={}", n);
            ctx.fill_text(&label, cx - 10.0, cy - r - 6.0);
        }
    }

    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}

/// Build the Tessellation Test — circle approximations with increasing segment counts.
pub fn tessellation_test(font: Arc<Font>) -> Box<dyn Widget> {
    let mut col = FlexColumn::new()
        .with_gap(8.0)
        .with_padding(8.0)
        .with_panel_bg();

    col.push(Box::new(Label::new(
        "Circle approximations: n = 3, 6, 12, 24, 48 segments",
        Arc::clone(&font),
    ).with_font_size(11.5)), 0.0);

    col.push(Box::new(TessellationWidget {
        bounds: Rect::default(), children: Vec::new(), font,
    }), 1.0);

    Box::new(col)
}

// ---------------------------------------------------------------------------
// Window Resize Test
// ---------------------------------------------------------------------------

/// Build the Window Resize Test — shows static size info and resize options.
pub fn window_resize_test(font: Arc<Font>) -> Box<dyn Widget> {
    use std::cell::Cell;
    use std::rc::Rc;
    use agg_gui::Checkbox;

    let min_size = Rc::new(Cell::new(true));
    let max_size = Rc::new(Cell::new(false));

    let mut col = FlexColumn::new()
        .with_gap(14.0)
        .with_padding(16.0)
        .with_panel_bg();

    col.push(Box::new(Label::new("Window resize test", Arc::clone(&font))
        .with_font_size(12.0)), 0.0);

    col.push(Box::new(Label::new(
        "Current size:  360 \u{00d7} 290\nMin size:      200 \u{00d7} 120\nMax size:      none",
        Arc::clone(&font),
    ).with_font_size(12.5)), 0.0);

    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(Box::new(Label::new("Constraints", Arc::clone(&font))
        .with_font_size(12.0)), 0.0);

    {
        let v = Rc::clone(&min_size);
        col.push(Box::new(Checkbox::new("Enforce minimum size (200 × 120)",
            Arc::clone(&font), min_size.get())
            .with_font_size(13.0).on_change(move |b| v.set(b))), 0.0);
    }
    {
        let v = Rc::clone(&max_size);
        col.push(Box::new(Checkbox::new("Enforce maximum size (720 × 540)",
            Arc::clone(&font), max_size.get())
            .with_font_size(13.0).on_change(move |b| v.set(b))), 0.0);
    }

    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(Box::new(Label::new(
        "Drag the window title bar edge to resize.\n\
         Min/max enforcement is not yet wired to the Window widget.",
        Arc::clone(&font),
    ).with_font_size(11.0)), 0.0);

    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);
    Box::new(col)
}
