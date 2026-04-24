//! Test window implementations for all 11 egui test windows.
//!
//! These are diagnostic/test widgets that verify framework behaviour.  Where
//! native capabilities (clipboard, OS cursors, SVG) are not yet wired up, a
//! clear informational placeholder is shown instead of broken code.

use std::sync::Arc;

use agg_gui::{
    Color, Container, CursorIcon, DrawCtx, Event, EventResult,
    FlexColumn, FlexRow, Font, Hyperlink, Label,
    Point, Rect, Resize, ScrollView, Separator, set_cursor_icon,
    Size, SizedBox, TextArea, TextField, Widget,
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
    ).with_font_size(11.5).with_wrap(true)), 0.0);

    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);
    Box::new(col)
}

// ---------------------------------------------------------------------------
// Cursor Test
// ---------------------------------------------------------------------------

/// All cursor icons in display order — mirrors egui's `CursorIcon::ALL`.
const ALL_CURSORS: &[(CursorIcon, &str)] = &[
    (CursorIcon::Default,          "Default"),
    (CursorIcon::None,             "None"),
    (CursorIcon::ContextMenu,      "ContextMenu"),
    (CursorIcon::Help,             "Help"),
    (CursorIcon::PointingHand,     "PointingHand"),
    (CursorIcon::Progress,         "Progress"),
    (CursorIcon::Wait,             "Wait"),
    (CursorIcon::Cell,             "Cell"),
    (CursorIcon::Crosshair,        "Crosshair"),
    (CursorIcon::Text,             "Text"),
    (CursorIcon::VerticalText,     "VerticalText"),
    (CursorIcon::Alias,            "Alias"),
    (CursorIcon::Copy,             "Copy"),
    (CursorIcon::Move,             "Move"),
    (CursorIcon::NoDrop,           "NoDrop"),
    (CursorIcon::NotAllowed,       "NotAllowed"),
    (CursorIcon::Grab,             "Grab"),
    (CursorIcon::Grabbing,         "Grabbing"),
    (CursorIcon::AllScroll,        "AllScroll"),
    (CursorIcon::ResizeHorizontal, "ResizeHorizontal"),
    (CursorIcon::ResizeNeSw,       "ResizeNeSw"),
    (CursorIcon::ResizeNwSe,       "ResizeNwSe"),
    (CursorIcon::ResizeVertical,   "ResizeVertical"),
    (CursorIcon::ResizeEast,       "ResizeEast"),
    (CursorIcon::ResizeSouthEast,  "ResizeSouthEast"),
    (CursorIcon::ResizeSouth,      "ResizeSouth"),
    (CursorIcon::ResizeSouthWest,  "ResizeSouthWest"),
    (CursorIcon::ResizeWest,       "ResizeWest"),
    (CursorIcon::ResizeNorthWest,  "ResizeNorthWest"),
    (CursorIcon::ResizeNorth,      "ResizeNorth"),
    (CursorIcon::ResizeNorthEast,  "ResizeNorthEast"),
    (CursorIcon::ResizeColumn,     "ResizeColumn"),
    (CursorIcon::ResizeRow,        "ResizeRow"),
    (CursorIcon::ZoomIn,           "ZoomIn"),
    (CursorIcon::ZoomOut,          "ZoomOut"),
];

/// Full-width row button that sets the OS cursor to `icon` on hover.
struct CursorRow {
    bounds:  Rect,
    children: Vec<Box<dyn Widget>>,
    icon:    CursorIcon,
    hovered: bool,
    label:   Label,
}

impl CursorRow {
    const H: f64 = 24.0;

    fn new(icon: CursorIcon, name: &'static str, font: Arc<Font>) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            icon,
            hovered: false,
            label: Label::new(name, font).with_font_size(12.0),
        }
    }
}

impl Widget for CursorRow {
    fn type_name(&self) -> &'static str { "CursorRow" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, available.width, Self::H);
        let ls = self.label.layout(Size::new(available.width, Self::H));
        self.label.set_bounds(Rect::new(0.0, 0.0, ls.width, ls.height));
        Size::new(available.width, Self::H)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let bg = if self.hovered { v.widget_bg_hovered } else { v.widget_bg };
        ctx.set_fill_color(bg);
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, self.bounds.width, Self::H, 3.0);
        ctx.fill();

        // Center label.
        self.label.set_color(v.text_color);
        let lw = self.label.bounds().width;
        let lh = self.label.bounds().height;
        let lx = (self.bounds.width - lw) * 0.5;
        let ly = (Self::H - lh) * 0.5;
        self.label.set_bounds(Rect::new(lx, ly, lw, lh));
        ctx.save();
        ctx.translate(lx, ly);
        paint_subtree(&mut self.label, ctx);
        ctx.restore();
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseMove { pos } => {
                let in_bounds = pos.x >= 0.0 && pos.x <= self.bounds.width
                    && pos.y >= 0.0 && pos.y <= Self::H;
                if in_bounds {
                    set_cursor_icon(self.icon);
                }
                let was = self.hovered;
                self.hovered = in_bounds;
                if self.hovered != was { EventResult::Consumed } else { EventResult::Ignored }
            }
            _ => EventResult::Ignored,
        }
    }

    fn hit_test(&self, p: Point) -> bool {
        p.x >= 0.0 && p.x <= self.bounds.width && p.y >= 0.0 && p.y <= Self::H
    }
}

/// Build the Cursor Test — 2-column layout showing all cursor icons.
///
/// Splits ALL_CURSORS into two halves side-by-side so the window stays compact.
/// Hovering each row sets the OS cursor.
pub fn cursor_test(font: Arc<Font>) -> Box<dyn Widget> {
    let half = ALL_CURSORS.len() / 2;
    let left_cursors  = &ALL_CURSORS[..half];
    let right_cursors = &ALL_CURSORS[half..];

    let mut left_col = FlexColumn::new().with_gap(2.0).with_padding(0.0);
    for &(icon, name) in left_cursors {
        left_col.push(Box::new(CursorRow::new(icon, name, Arc::clone(&font))), 0.0);
    }

    let mut right_col = FlexColumn::new().with_gap(2.0).with_padding(0.0);
    for &(icon, name) in right_cursors {
        right_col.push(Box::new(CursorRow::new(icon, name, Arc::clone(&font))), 0.0);
    }

    let cols_row = FlexRow::new()
        .with_gap(4.0)
        .add_flex(Box::new(left_col), 1.0)
        .add_flex(Box::new(right_col), 1.0);

    let mut col = FlexColumn::new()
        .with_gap(4.0)
        .with_padding(8.0)
        .with_panel_bg();

    col.push(Box::new(
        Label::new("Hover to switch cursor icon:", Arc::clone(&font))
            .with_font_size(13.0)
    ), 0.0);
    col.push(Box::new(cols_row), 0.0);
    col.push(Box::new(SizedBox::new().with_height(4.0)), 0.0);
    // Flex fill so panel_bg covers full window content area.
    col.push(Box::new(SizedBox::new()), 1.0);

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
    ).with_font_size(11.0).with_wrap(true)), 0.0);

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
    ).with_font_size(11.5).with_wrap(true)), 0.0);

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
    ).with_font_size(11.5).with_wrap(true)), 0.0);

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
    ).with_font_size(11.0).with_wrap(true)), 0.0);

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
    ).with_font_size(11.5).with_wrap(true)), 0.0);

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
    ).with_font_size(12.0).with_wrap(true)), 0.0);

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
    ).with_font_size(11.5).with_wrap(true)), 0.0);

    col.push(Box::new(TessellationWidget {
        bounds: Rect::default(), children: Vec::new(), font,
    }), 1.0);

    Box::new(col)
}

// ---------------------------------------------------------------------------
// Window Resize Test
// ---------------------------------------------------------------------------

// Short and long Lorem Ipsum strings — mirrors the egui reference constants.
const LOREM_IPSUM: &str = "Lorem ipsum dolor sit amet, consectetur adipiscing \
elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim \
ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea \
commodo consequat.";

const LOREM_IPSUM_LONG: &str = "Lorem ipsum dolor sit amet, consectetur \
adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. \
Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip \
ex ea commodo consequat. Duis aute irure dolor in reprehenderit in voluptate velit \
esse cillum dolore eu fugiat nulla pariatur. Excepteur sint occaecat cupidatat non \
proident, sunt in culpa qui officia deserunt mollit anim id est laborum.\n\n\
Sed ut perspiciatis unde omnis iste natus error sit voluptatem accusantium \
doloremque laudantium, totam rem aperiam, eaque ipsa quae ab illo inventore \
veritatis et quasi architecto beatae vitae dicta sunt explicabo. Nemo enim ipsam \
voluptatem quia voluptas sit aspernatur aut odit aut fugit, sed quia consequuntur \
magni dolores eos qui ratione voluptatem sequi nesciunt.\n\n\
At vero eos et accusamus et iusto odio dignissimos ducimus qui blanditiis \
praesentium voluptatum deleniti atque corrupti quos dolores et quas molestias \
excepturi sint occaecati cupiditate non provident, similique sunt in culpa qui \
officia deserunt mollitia animi, id est laborum et dolorum fuga.";

/// One entry returned from [`window_resize_sub_windows`].  The caller
/// wraps `content` in a `Window` and applies the flags to its builder
/// (`with_auto_size`, `with_resizable` / `with_resizable_axes`) so each
/// sub-window demonstrates the exact egui behaviour it's named after.
pub struct ResizeTestWindow {
    pub title:        String,
    pub content:      Box<dyn Widget>,
    pub initial_rect: Rect,
    /// Window fits tightly to its content; ignores `resizable_*`.
    pub auto_size:    bool,
    /// Master user-resize toggle.  `false` → no handles active.
    pub resizable:    bool,
    /// Axis-specific locks (only consulted when `resizable` is `true`).
    pub resizable_h:  bool,
    pub resizable_v:  bool,
    /// Wrap content in a built-in vertical `ScrollView` at window
    /// build time.  Matches egui's `Window::vscroll(true)`.
    pub vscroll:      bool,
    /// Resize floor + ceiling follow content natural height.
    /// Matches egui's no-scroll-no-clip-no-whitespace contract for
    /// W4 (window snaps to content height in both directions).
    pub tight_fit:    bool,
    /// Resize FLOOR only follows content height; user can pull the
    /// window taller (whitespace below).  Used for W5 where a
    /// flex-fill `TextArea` absorbs extra space.
    pub floor_fit:    bool,
}

impl ResizeTestWindow {
    fn new(title: &str, content: Box<dyn Widget>, initial_rect: Rect) -> Self {
        Self {
            title: title.into(),
            content,
            initial_rect,
            auto_size:   false,
            resizable:   true,
            resizable_h: true,
            resizable_v: true,
            vscroll:     false,
            tight_fit:   false,
            floor_fit:   false,
        }
    }
    fn auto_sized(mut self) -> Self { self.auto_size = true; self.resizable = false; self }
    fn with_vscroll(mut self) -> Self { self.vscroll = true; self }
    fn with_tight_fit(mut self) -> Self { self.tight_fit = true; self }
    fn with_floor_fit(mut self) -> Self { self.floor_fit = true; self }
}

/// URL of the source file containing the six Window Resize Test
/// sub-window builders — surfaced via the "(source code)" footer
/// link on each window so developers can see exactly how each
/// layout was assembled, matching egui's `egui_github_link_file!`
/// pattern in the original demo.
const RESIZE_TEST_SOURCE_URL: &str =
    "https://github.com/larsbrubaker/agg-gui/blob/main/demo-ui/src/windows/tests.rs";

/// Helper: a small "(source code)" hyperlink that opens the test
/// source file in a browser.  Callers push this as the final child
/// of each sub-window's root column, just like egui's demo.
fn source_link(font: Arc<Font>) -> Box<dyn Widget> {
    Box::new(
        Hyperlink::new("(source code)", font)
            .with_font_size(11.0)
            .on_click(|| crate::url::open_url(RESIZE_TEST_SOURCE_URL))
    )
}

/// Build the six sub-windows for the Window Resize Test, mirroring
/// egui's `crates/egui_demo_lib/src/demo/tests/window_resize_test.rs`
/// one-for-one.  Each window demonstrates a specific resize + scroll +
/// content-fill combination; the caller applies the returned flags to
/// its `Window` wrapper so those behaviours surface correctly.
pub fn window_resize_sub_windows(font: Arc<Font>) -> Vec<ResizeTestWindow> {
    // Initial rects in Y-up canvas coordinates (default_canvas_h ≈ 720).
    // Staggered 3 × 2 so the windows are visible on a 1280×720 screen.
    // Ordering matches egui's source, not layout order on screen.
    let rects: &[Rect] = &[
        Rect::new( 30.0, 100.0, 360.0, 240.0), // 1. ↔ auto-sized
        Rect::new(410.0, 100.0, 300.0, 290.0), // 2. ↔ resizable + scroll
        Rect::new(730.0, 100.0, 300.0, 290.0), // 3. ↔ resizable + embedded scroll
        Rect::new( 30.0, 410.0, 300.0, 290.0), // 4. ↔ resizable without scroll
        Rect::new(410.0, 410.0, 300.0, 290.0), // 5. ↔ resizable with TextEdit
        Rect::new(730.0, 410.0, 250.0, 150.0), // 6. ↔ freely resized
    ];

    let mut out: Vec<ResizeTestWindow> = Vec::new();

    // ── 1. ↔ auto-sized ──────────────────────────────────────────────────────
    //
    // Outer window is `auto_sized()`, so it fits its content each
    // frame and disables its own user-drag resize.  The inner area is
    // the Stage-3 `Resize` widget — a user-draggable nested region.
    // Dragging its SE grip:
    //   * Grows the Resize past its content's natural size (the
    //     `Resize` widget enforces content-natural as a min, so it
    //     can never shrink past what fits).
    //   * Pushes the surrounding Window wider when the Resize
    //     demands more width than the current window inner area —
    //     via `FlexColumn::with_fit_width(true)` reporting the
    //     widest child's natural size up through `Window::auto_size`.
    //
    // Styling: `Resize` already draws its own rounded outline; no
    // `Container` wrapper needed (previously we had both, giving a
    // visible double outline).
    {
        let mut root = FlexColumn::new()
            .with_gap(6.0).with_padding(10.0).with_panel_bg()
            .with_fit_width(true);
        // Outer labels are NOT wrapped: in an auto-sized window,
        // wrap=true Labels would claim the full current slot width
        // and prevent the window from shrinking back down when the
        // inner Resize narrows.  Non-wrapped Labels report their
        // single-line natural width, which becomes the stable
        // minimum the window tracks to — same pattern egui's
        // auto-sized demo uses.
        root.push(Box::new(Label::new(
            "This window will auto-size based on its contents.",
            Arc::clone(&font),
        ).with_font_size(12.0)), 0.0);
        root.push(Box::new(Label::new(
            "Resize this area:",
            Arc::clone(&font),
        ).with_font_size(14.0)), 0.0);
        // The lorem ipsum INSIDE the Resize widget still wraps so it
        // reshapes as the user narrows / widens the Resize.  The
        // Resize widget enforces a content-natural minimum so the
        // wrapped text can never be clipped.  `top_anchor` keeps the
        // text at the top of the Resize frame when the user pulls it
        // taller — without this, FlexColumn's default natural-anchor
        // would leave the text pinned to the BOTTOM of the frame
        // with whitespace above (the bug visible in image #24).
        let mut inner = FlexColumn::new()
            .with_gap(4.0).with_padding(8.0)
            .with_fit_width(true)
            .with_top_anchor(true);
        inner.push(Box::new(Label::new(LOREM_IPSUM, Arc::clone(&font))
            .with_font_size(11.5)
            .with_wrap(true)), 0.0);
        // No explicit max_size_hint here — we want the user to be
        // able to drag the inner Resize all the way to the canvas
        // extent, letting the outer auto-sized Window grow with it.
        // The `Window::auto_size` clamp to `available.width` caps
        // final growth at the surrounding layout's inner width.
        root.push(Box::new(
            Resize::new(Box::new(inner))
                .with_default_size(Size::new(320.0, 120.0))
                .with_min_size_hint(Size::new(120.0, 60.0))
                .with_max_size_hint(Size::new(4000.0, 3000.0)),
        ), 0.0);
        root.push(Box::new(Label::new(
            "Resize the above area!",
            Arc::clone(&font),
        ).with_font_size(14.0)), 0.0);
        root.push(source_link(Arc::clone(&font)), 0.0);
        out.push(ResizeTestWindow::new(
            "↔ auto-sized", Box::new(root), rects[0],
        ).auto_sized());
    }

    // ── 2. ↔ resizable + scroll ──────────────────────────────────────────────
    //
    // Window-level vscroll (egui's `.vscroll(true)`).  No manual
    // ScrollView in the content tree — the `Window::with_vscroll(true)`
    // call in `lib.rs` (Stage 2) wraps `root` itself in a vertical
    // ScrollView at builder time.  The inner content is a single
    // overflowing FlexColumn so the scroll bar has range.
    {
        let mut root = FlexColumn::new().with_gap(8.0).with_padding(10.0).with_panel_bg();
        root.push(Box::new(Label::new(
            "This window is resizable and has a scroll area. You can shrink it \
             to any size.",
            Arc::clone(&font),
        ).with_font_size(12.0).with_wrap(true)), 0.0);
        root.push(Box::new(Separator::horizontal()), 0.0);
        root.push(Box::new(Label::new(LOREM_IPSUM_LONG, Arc::clone(&font))
            .with_font_size(11.5)
            .with_wrap(true)), 0.0);
        root.push(source_link(Arc::clone(&font)), 0.0);
        out.push(ResizeTestWindow::new(
            "↔ resizable + scroll", Box::new(root), rects[1],
        ).with_vscroll());
    }

    // ── 3. ↔ resizable + embedded scroll ────────────────────────────────────
    {
        let mut root = FlexColumn::new().with_gap(8.0).with_padding(10.0).with_panel_bg();
        root.push(Box::new(Label::new(
            "This window is resizable but has no built-in scroll area.",
            Arc::clone(&font),
        ).with_font_size(12.0).with_wrap(true)), 0.0);
        root.push(Box::new(Label::new(
            "However, we have a sub-region with a scroll bar:",
            Arc::clone(&font),
        ).with_font_size(12.0).with_wrap(true)), 0.0);
        root.push(Box::new(Separator::horizontal()), 0.0);
        let long2 = format!("{}\n\n{}", LOREM_IPSUM_LONG, LOREM_IPSUM_LONG);
        let mut inner = FlexColumn::new().with_gap(4.0).with_padding(4.0);
        inner.push(Box::new(Label::new(&long2, Arc::clone(&font))
            .with_font_size(11.5)
            .with_wrap(true)), 0.0);
        root.push(Box::new(ScrollView::new(Box::new(inner))), 1.0);
        root.push(source_link(Arc::clone(&font)), 0.0);
        out.push(ResizeTestWindow::new(
            "↔ resizable + embedded scroll", Box::new(root), rects[2],
        ));
    }

    // ── 4. ↔ resizable without scroll ───────────────────────────────────────
    //
    // egui never clips window content and has no whitespace to add, so the
    // user can only shrink down to a size that still fits all content.
    // Stage 5 enforces that at the library level: `with_tight_content_fit`
    // makes the resize clamp floor honour the content's natural height
    // observed in the last layout.
    {
        let mut root = FlexColumn::new().with_gap(8.0).with_padding(10.0).with_panel_bg();
        root.push(Box::new(Label::new(
            "This window is resizable but has no scroll area. This means it \
             can only be resized to a size where all the contents is visible.",
            Arc::clone(&font),
        ).with_font_size(12.0).with_wrap(true)), 0.0);
        root.push(Box::new(Label::new(
            "agg-gui will not clip the contents of a window, nor add \
             whitespace to it.",
            Arc::clone(&font),
        ).with_font_size(12.0).with_wrap(true)), 0.0);
        root.push(Box::new(Separator::horizontal()), 0.0);
        root.push(Box::new(Label::new(LOREM_IPSUM, Arc::clone(&font))
            .with_font_size(11.5)
            .with_wrap(true)), 0.0);
        root.push(source_link(Arc::clone(&font)), 0.0);
        out.push(ResizeTestWindow::new(
            "↔ resizable without scroll", Box::new(root), rects[3],
        ).with_tight_fit());
    }

    // ── 5. ↔ resizable with TextEdit ────────────────────────────────────────
    //
    // Stage-4 multiline `TextArea` fills the remaining space — so as
    // the user resizes the window, the editor follows both axes.
    // Pre-seeded with lorem ipsum so wrap + selection are immediately
    // demonstrable.  `tight_fit` enforces the egui contract: window
    // height ≥ TextArea content height, so wrapping text never falls
    // off-screen.
    {
        let mut root = FlexColumn::new().with_gap(8.0).with_padding(10.0).with_panel_bg();
        root.push(Box::new(Label::new(
            "Shows how you can fill an area with a widget.",
            Arc::clone(&font),
        ).with_font_size(12.0).with_wrap(true)), 0.0);
        root.push(Box::new(
            TextArea::new(Arc::clone(&font))
                .with_font_size(12.5)
                .with_text(LOREM_IPSUM),
        ), 1.0);
        root.push(source_link(Arc::clone(&font)), 0.0);
        out.push(ResizeTestWindow::new(
            "↔ resizable with TextEdit", Box::new(root), rects[4],
        ).with_floor_fit());
    }

    // ── 6. ↔ freely resized ─────────────────────────────────────────────────
    {
        let mut root = FlexColumn::new().with_gap(8.0).with_padding(10.0).with_panel_bg();
        root.push(Box::new(Label::new(
            "This window has empty space that fills up the available space, \
             preventing auto-shrink.",
            Arc::clone(&font),
        ).with_font_size(12.0).with_wrap(true)), 0.0);
        root.push(source_link(Arc::clone(&font)), 0.0);
        root.push(Box::new(SizedBox::new()), 1.0);
        out.push(ResizeTestWindow::new(
            "↔ freely resized", Box::new(root), rects[5],
        ));
    }

    out
}
