//! Test window implementations for egui-inspired diagnostic windows.
//!
//! These are diagnostic/test widgets that verify framework behaviour.  Where
//! native capabilities (clipboard, OS cursors, SVG) are not yet wired up, a
//! clear informational placeholder is shown instead of broken code.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::framebuffer::unpremultiply_rgba_inplace;
use agg_gui::widget::paint_subtree;
use agg_gui::{
    render_svg_at_size, render_svg_to_framebuffer_at_size, render_svg_to_lcd_buffer_at_size,
    set_cursor_icon, Color, Container, CursorIcon, DrawCtx, Event, EventResult, FlexColumn,
    FlexRow, Font, Hyperlink, Label, MouseButton, Point, Rect, Resize, ScrollBarVisibility,
    ScrollView, Separator, Size, SizedBox, TextArea, TextField, Visuals, Widget,
};

// ---------------------------------------------------------------------------
// Clipboard Test
// ---------------------------------------------------------------------------

/// Build the Clipboard Test — two side-by-side TextFields for copy/paste.
pub fn clipboard_test(font: Arc<Font>) -> Box<dyn Widget> {
    let mut col = FlexColumn::new()
        .with_gap(14.0)
        .with_padding(16.0)
        .with_panel_bg();

    col.push(
        Box::new(Label::new("Clipboard test", Arc::clone(&font)).with_font_size(12.0)),
        0.0,
    );

    let row = FlexRow::new()
        .with_gap(10.0)
        .add_flex(
            Box::new(
                FlexColumn::new()
                    .with_gap(6.0)
                    .add(Box::new(
                        Label::new("Copy from:", Arc::clone(&font)).with_font_size(11.5),
                    ))
                    .add(Box::new(
                        SizedBox::new().with_height(32.0).with_child(Box::new(
                            TextField::new(Arc::clone(&font))
                                .with_font_size(13.0)
                                .with_text("Select and copy me"),
                        )),
                    )),
            ),
            1.0,
        )
        .add_flex(
            Box::new(
                FlexColumn::new()
                    .with_gap(6.0)
                    .add(Box::new(
                        Label::new("Paste into:", Arc::clone(&font)).with_font_size(11.5),
                    ))
                    .add(Box::new(
                        SizedBox::new().with_height(32.0).with_child(Box::new(
                            TextField::new(Arc::clone(&font))
                                .with_font_size(13.0)
                                .with_placeholder("Ctrl+V here"),
                        )),
                    )),
            ),
            1.0,
        );
    col.push(Box::new(row), 0.0);

    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(
        Box::new(
            Label::new(
                "Ctrl+C / Ctrl+X — copy or cut selected text\n\
         Ctrl+V           — paste from clipboard\n\
         Ctrl+A           — select all",
                Arc::clone(&font),
            )
            .with_font_size(11.5)
            .with_wrap(true),
        ),
        0.0,
    );

    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);
    Box::new(col)
}

// ---------------------------------------------------------------------------
// Cursor Test
// ---------------------------------------------------------------------------

/// All cursor icons in display order — mirrors egui's `CursorIcon::ALL`.
const ALL_CURSORS: &[(CursorIcon, &str)] = &[
    (CursorIcon::Default, "Default"),
    (CursorIcon::None, "None"),
    (CursorIcon::ContextMenu, "ContextMenu"),
    (CursorIcon::Help, "Help"),
    (CursorIcon::PointingHand, "PointingHand"),
    (CursorIcon::Progress, "Progress"),
    (CursorIcon::Wait, "Wait"),
    (CursorIcon::Cell, "Cell"),
    (CursorIcon::Crosshair, "Crosshair"),
    (CursorIcon::Text, "Text"),
    (CursorIcon::VerticalText, "VerticalText"),
    (CursorIcon::Alias, "Alias"),
    (CursorIcon::Copy, "Copy"),
    (CursorIcon::Move, "Move"),
    (CursorIcon::NoDrop, "NoDrop"),
    (CursorIcon::NotAllowed, "NotAllowed"),
    (CursorIcon::Grab, "Grab"),
    (CursorIcon::Grabbing, "Grabbing"),
    (CursorIcon::AllScroll, "AllScroll"),
    (CursorIcon::ResizeHorizontal, "ResizeHorizontal"),
    (CursorIcon::ResizeNeSw, "ResizeNeSw"),
    (CursorIcon::ResizeNwSe, "ResizeNwSe"),
    (CursorIcon::ResizeVertical, "ResizeVertical"),
    (CursorIcon::ResizeEast, "ResizeEast"),
    (CursorIcon::ResizeSouthEast, "ResizeSouthEast"),
    (CursorIcon::ResizeSouth, "ResizeSouth"),
    (CursorIcon::ResizeSouthWest, "ResizeSouthWest"),
    (CursorIcon::ResizeWest, "ResizeWest"),
    (CursorIcon::ResizeNorthWest, "ResizeNorthWest"),
    (CursorIcon::ResizeNorth, "ResizeNorth"),
    (CursorIcon::ResizeNorthEast, "ResizeNorthEast"),
    (CursorIcon::ResizeColumn, "ResizeColumn"),
    (CursorIcon::ResizeRow, "ResizeRow"),
    (CursorIcon::ZoomIn, "ZoomIn"),
    (CursorIcon::ZoomOut, "ZoomOut"),
];

/// Full-width row button that sets the OS cursor to `icon` on hover.
struct CursorRow {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    icon: CursorIcon,
    hovered: bool,
    label: Label,
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
    fn type_name(&self) -> &'static str {
        "CursorRow"
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
        self.bounds = Rect::new(0.0, 0.0, available.width, Self::H);
        let ls = self.label.layout(Size::new(available.width, Self::H));
        self.label
            .set_bounds(Rect::new(0.0, 0.0, ls.width, ls.height));
        Size::new(available.width, Self::H)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let bg = if self.hovered {
            v.widget_bg_hovered
        } else {
            v.widget_bg
        };
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
                let in_bounds =
                    pos.x >= 0.0 && pos.x <= self.bounds.width && pos.y >= 0.0 && pos.y <= Self::H;
                if in_bounds {
                    set_cursor_icon(self.icon);
                }
                let was = self.hovered;
                self.hovered = in_bounds;
                if self.hovered != was {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
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
    let left_cursors = &ALL_CURSORS[..half];
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

    col.push(
        Box::new(
            Label::new("Hover to switch cursor icon:", Arc::clone(&font)).with_font_size(13.0),
        ),
        0.0,
    );
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
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    font: Arc<Font>,
    cols: usize,
    rows: usize,
}

impl Widget for GridPainter {
    fn type_name(&self) -> &'static str {
        "GridPainter"
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
        let cw = self.bounds.width / self.cols as f64;
        let ch = self.bounds.height / self.rows as f64;
        let n = self.cols * self.rows;

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

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// Build the Grid Test — an 8×6 colored grid with coordinate labels.
pub fn grid_test(font: Arc<Font>) -> Box<dyn Widget> {
    let mut col = FlexColumn::new()
        .with_gap(10.0)
        .with_padding(12.0)
        .with_panel_bg();

    col.push(
        Box::new(Label::new("8 × 6 alignment grid", Arc::clone(&font)).with_font_size(12.0)),
        0.0,
    );

    col.push(
        Box::new(GridPainter {
            bounds: Rect::default(),
            children: Vec::new(),
            font: Arc::clone(&font),
            cols: 8,
            rows: 6,
        }),
        1.0,
    );

    Box::new(col)
}

// ---------------------------------------------------------------------------
// Id Test
// ---------------------------------------------------------------------------

/// Build the Id Test — static informational display of widget type names.
pub fn id_test(font: Arc<Font>) -> Box<dyn Widget> {
    let types = [
        ("Button", "btn_primary"),
        ("Checkbox", "cb_feature_a"),
        ("Slider", "slider_val_0"),
        ("TextField", "tf_search"),
        ("Label", "lbl_title"),
        ("FlexColumn", "col_root"),
        ("FlexRow", "row_buttons"),
        ("Container", "container_panel"),
        ("ScrollView", "scroll_main"),
        ("ProgressBar", "pb_loading"),
    ];

    let mut col = FlexColumn::new()
        .with_gap(8.0)
        .with_padding(14.0)
        .with_panel_bg();

    col.push(
        Box::new(Label::new("Widget type → generated ID", Arc::clone(&font)).with_font_size(12.0)),
        0.0,
    );

    col.push(Box::new(Separator::horizontal()), 0.0);

    for (ty, id) in types {
        let row = FlexRow::new()
            .with_gap(8.0)
            .add(Box::new(SizedBox::new().with_width(120.0).with_child(
                Box::new(Label::new(ty, Arc::clone(&font)).with_font_size(12.5)),
            )))
            .add(Box::new(
                Label::new(id, Arc::clone(&font))
                    .with_font_size(12.0)
                    .with_color(Color::rgb(0.22, 0.45, 0.88)),
            ));
        col.push(Box::new(row), 0.0);
    }

    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(
        Box::new(
            Label::new(
                "IDs are hashed from the widget type name + call-site path.",
                Arc::clone(&font),
            )
            .with_font_size(11.0)
            .with_wrap(true),
        ),
        0.0,
    );

    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);
    Box::new(col)
}

// ---------------------------------------------------------------------------
// Input Event History
// ---------------------------------------------------------------------------

/// Records the last N events and renders them as a scrollable list.
struct EventHistoryWidget {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    font: Arc<Font>,
    events: Vec<String>,
    max: usize,
}

impl EventHistoryWidget {
    fn new(font: Arc<Font>) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            font,
            events: Vec::new(),
            max: 20,
        }
    }

    fn push_event(&mut self, s: String) {
        self.events.insert(0, s);
        self.events.truncate(self.max);
    }
}

impl Widget for EventHistoryWidget {
    fn type_name(&self) -> &'static str {
        "EventHistoryWidget"
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
            if y < 0.0 {
                break;
            }
            let alpha = 1.0 - i as f64 * 0.045;
            ctx.set_fill_color(Color::rgba(
                v.text_color.r,
                v.text_color.g,
                v.text_color.b,
                alpha as f32,
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
            Event::MouseMove { pos } => format!("MouseMove ({:.0}, {:.0})", pos.x, pos.y),
            Event::MouseDown { pos, button, .. } => {
                format!("MouseDown {:?} ({:.0},{:.0})", button, pos.x, pos.y)
            }
            Event::MouseUp { button, .. } => format!("MouseUp {:?}", button),
            Event::KeyDown { key, .. } => format!("KeyDown {:?}", key),
            Event::KeyUp { key, .. } => format!("KeyUp {:?}", key),
            Event::MouseWheel { delta_y, .. } => format!("MouseWheel {:.1}", delta_y),
            _ => return EventResult::Ignored,
        };
        self.push_event(desc);
        EventResult::Consumed
    }

    fn hit_test(&self, p: Point) -> bool {
        p.x >= 0.0 && p.x <= self.bounds.width && p.y >= 0.0 && p.y <= self.bounds.height
    }
}

/// Build the Input Event History — records and displays the last 20 events.
pub fn input_event_history(font: Arc<Font>) -> Box<dyn Widget> {
    let mut col = FlexColumn::new()
        .with_gap(8.0)
        .with_padding(10.0)
        .with_panel_bg();

    col.push(
        Box::new(
            Label::new(
                "Interact inside the box to record events (last 20)",
                Arc::clone(&font),
            )
            .with_font_size(11.5)
            .with_wrap(true),
        ),
        0.0,
    );

    col.push(Box::new(EventHistoryWidget::new(Arc::clone(&font))), 1.0);
    Box::new(col)
}

// ---------------------------------------------------------------------------
// Input Test
// ---------------------------------------------------------------------------

/// Records the last-pressed key name and mouse position.
struct InputStateWidget {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    font: Arc<Font>,
    last_key: Option<String>,
    mouse_pos: Point,
}

impl Widget for InputStateWidget {
    fn type_name(&self) -> &'static str {
        "InputStateWidget"
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
            &format!(
                "Mouse pos:  ({:.0}, {:.0})",
                self.mouse_pos.x, self.mouse_pos.y
            ),
            10.0,
            h - 44.0,
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
        p.x >= 0.0 && p.x <= self.bounds.width && p.y >= 0.0 && p.y <= self.bounds.height
    }
}

/// Build the Input Test — shows last key pressed and current mouse position.
pub fn input_test(font: Arc<Font>) -> Box<dyn Widget> {
    let mut col = FlexColumn::new()
        .with_gap(12.0)
        .with_padding(12.0)
        .with_panel_bg();

    col.push(
        Box::new(
            Label::new(
                "Move the mouse or press keys inside the status box",
                Arc::clone(&font),
            )
            .with_font_size(11.5)
            .with_wrap(true),
        ),
        0.0,
    );

    col.push(
        Box::new(InputStateWidget {
            bounds: Rect::default(),
            children: Vec::new(),
            font: Arc::clone(&font),
            last_key: None,
            mouse_pos: Point { x: 0.0, y: 0.0 },
        }),
        0.0,
    );

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

    col.push(
        Box::new(Label::new("Alignment examples", Arc::clone(&font)).with_font_size(12.0)),
        0.0,
    );

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
            .add(Box::new(
                Label::new(lbl, Arc::clone(&font)).with_font_size(12.0),
            ));

        if i == 3 {
            // Stretch row.
            let row = FlexRow::new().add_flex(Box::new(cell), 1.0);
            col.push(Box::new(row), 0.0);
        } else {
            let row = FlexRow::new().add(Box::new(
                SizedBox::new().with_width(box_w).with_child(Box::new(cell)),
            ));
            col.push(Box::new(row), 0.0);
        }
    }

    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(
        Box::new(
            Label::new(
                "FlexRow / FlexColumn control alignment.\n\
         add() = fixed-size child, add_flex() = fills remaining space.",
                Arc::clone(&font),
            )
            .with_font_size(11.0)
            .with_wrap(true),
        ),
        0.0,
    );

    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);
    Box::new(col)
}

// ---------------------------------------------------------------------------
// Manual Layout Test
// ---------------------------------------------------------------------------

/// A custom-painted widget showing absolutely-positioned boxes with corner labels.
struct ManualLayoutWidget {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    font: Arc<Font>,
}

impl Widget for ManualLayoutWidget {
    fn type_name(&self) -> &'static str {
        "ManualLayoutWidget"
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
        ctx.set_fill_color(v.bg_color);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, w, h);
        ctx.fill();

        ctx.set_font(Arc::clone(&self.font));

        // Absolutely-positioned boxes.
        let boxes: &[(f64, f64, f64, f64, Color, &str)] = &[
            (
                10.0,
                h - 60.0,
                80.0,
                40.0,
                Color::rgba(0.22, 0.45, 0.88, 0.25),
                "TL",
            ),
            (
                w - 90.0,
                h - 60.0,
                80.0,
                40.0,
                Color::rgba(0.18, 0.72, 0.42, 0.25),
                "TR",
            ),
            (
                10.0,
                20.0,
                80.0,
                40.0,
                Color::rgba(0.88, 0.25, 0.18, 0.25),
                "BL",
            ),
            (
                w - 90.0,
                20.0,
                80.0,
                40.0,
                Color::rgba(0.86, 0.78, 0.40, 0.25),
                "BR",
            ),
            (
                (w - 100.0) * 0.5,
                (h - 50.0) * 0.5,
                100.0,
                50.0,
                Color::rgba(0.60, 0.25, 0.88, 0.20),
                "Center",
            ),
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

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// Build the Manual Layout Test — five absolutely positioned boxes.
pub fn manual_layout_test(font: Arc<Font>) -> Box<dyn Widget> {
    let mut col = FlexColumn::new()
        .with_gap(8.0)
        .with_padding(8.0)
        .with_panel_bg();

    col.push(
        Box::new(
            Label::new(
                "Absolutely-positioned boxes with coordinate labels",
                Arc::clone(&font),
            )
            .with_font_size(11.5)
            .with_wrap(true),
        ),
        0.0,
    );

    col.push(
        Box::new(ManualLayoutWidget {
            bounds: Rect::default(),
            children: Vec::new(),
            font,
        }),
        1.0,
    );

    Box::new(col)
}

// ---------------------------------------------------------------------------
// SVG Test
// ---------------------------------------------------------------------------

/// Build the SVG Test — live progress viewer for the library SVG renderer.
pub fn svg_test(font: Arc<Font>) -> Box<dyn Widget> {
    let samples = Arc::new(
        SVG_SAMPLES
            .iter()
            .map(SvgSampleRender::new)
            .collect::<Vec<_>>(),
    );
    let zoom = Rc::new(Cell::new(SVG_DEFAULT_ZOOM));
    let v_offset = Rc::new(Cell::new(0.0));
    let v_max = Rc::new(Cell::new(0.0));
    let h_offset = Rc::new(Cell::new(0.0));
    let h_max = Rc::new(Cell::new(0.0));

    let mut root = FlexColumn::new()
        .with_gap(0.0)
        .with_padding(0.0)
        .with_panel_bg();
    root.push(
        Box::new(SvgProgressHeader::new(
            Arc::clone(&font),
            Arc::clone(&samples),
            Rc::clone(&zoom),
            Rc::clone(&v_offset),
            Rc::clone(&v_max),
            Rc::clone(&h_offset),
            Rc::clone(&h_max),
        )),
        0.0,
    );
    root.push(
        Box::new(
            ScrollView::new(Box::new(SvgProgressBody::new(
                Arc::clone(&samples),
                Rc::clone(&zoom),
                Rc::clone(&v_offset),
                Rc::clone(&v_max),
                Rc::clone(&h_offset),
                Rc::clone(&h_max),
            )))
            .horizontal(true)
            .with_offset_cell(Rc::clone(&v_offset))
            .with_max_scroll_cell(Rc::clone(&v_max))
            .with_h_offset_cell(Rc::clone(&h_offset))
            .with_h_max_scroll_cell(Rc::clone(&h_max))
            .with_bar_visibility(ScrollBarVisibility::AlwaysVisible),
        ),
        1.0,
    );

    Box::new(root)
}

const SVG_HEADER_H: f64 = 124.0;
const SVG_TITLE_H: f64 = 92.0;
const SVG_COLUMN_HEADER_H: f64 = 32.0;
const SVG_PAD: f64 = 8.0;
const SVG_GAP: f64 = 8.0;
const SVG_DEFAULT_ZOOM: f64 = 0.5;
const SVG_MIN_ZOOM: f64 = 0.1;
const SVG_MAX_ZOOM: f64 = 8.0;

struct SvgProgressHeader {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    font: Arc<Font>,
    samples: Arc<Vec<SvgSampleRender>>,
    zoom: Rc<Cell<f64>>,
}

struct SvgProgressBody {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    samples: Arc<Vec<SvgSampleRender>>,
    zoom: Rc<Cell<f64>>,
    v_offset: Rc<Cell<f64>>,
    v_max: Rc<Cell<f64>>,
    h_offset: Rc<Cell<f64>>,
    h_max: Rc<Cell<f64>>,
}

struct SvgZoomButton {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    label: &'static str,
    target_zoom: Option<f64>,
    font: Arc<Font>,
    samples: Arc<Vec<SvgSampleRender>>,
    zoom: Rc<Cell<f64>>,
    v_offset: Rc<Cell<f64>>,
    v_max: Rc<Cell<f64>>,
    h_offset: Rc<Cell<f64>>,
    h_max: Rc<Cell<f64>>,
    pressed: bool,
    hovered: bool,
}

struct SvgSampleRender {
    name: &'static str,
    svg: &'static [u8],
    width: u32,
    height: u32,
    reference: Result<Arc<Vec<u8>>, String>,
    rgba: Result<Arc<Vec<u8>>, String>,
    lcd: Result<SvgLcdPreview, String>,
}

impl SvgZoomButton {
    fn new(
        label: &'static str,
        target_zoom: Option<f64>,
        font: Arc<Font>,
        samples: Arc<Vec<SvgSampleRender>>,
        zoom: Rc<Cell<f64>>,
        v_offset: Rc<Cell<f64>>,
        v_max: Rc<Cell<f64>>,
        h_offset: Rc<Cell<f64>>,
        h_max: Rc<Cell<f64>>,
    ) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            label,
            target_zoom,
            font,
            samples,
            zoom,
            v_offset,
            v_max,
            h_offset,
            h_max,
            pressed: false,
            hovered: false,
        }
    }

    fn active(&self) -> bool {
        match self.target_zoom {
            Some(target) => is_zoom_level(self.zoom.get(), target),
            None => !is_zoom_level(self.zoom.get(), 0.5) && !is_zoom_level(self.zoom.get(), 1.0),
        }
    }

    fn contains(&self, pos: Point) -> bool {
        pos.x >= 0.0 && pos.x <= self.bounds.width && pos.y >= 0.0 && pos.y <= self.bounds.height
    }
}

struct SvgLcdPreview {
    color: Arc<Vec<u8>>,
    alpha: Arc<Vec<u8>>,
}

impl SvgProgressHeader {
    fn new(
        font: Arc<Font>,
        samples: Arc<Vec<SvgSampleRender>>,
        zoom: Rc<Cell<f64>>,
        v_offset: Rc<Cell<f64>>,
        v_max: Rc<Cell<f64>>,
        h_offset: Rc<Cell<f64>>,
        h_max: Rc<Cell<f64>>,
    ) -> Self {
        let mut children: Vec<Box<dyn Widget>> = Vec::new();
        for (label, target_zoom) in [("50%", 0.5), ("100%", 1.0)] {
            children.push(Box::new(SvgZoomButton::new(
                label,
                Some(target_zoom),
                Arc::clone(&font),
                Arc::clone(&samples),
                Rc::clone(&zoom),
                Rc::clone(&v_offset),
                Rc::clone(&v_max),
                Rc::clone(&h_offset),
                Rc::clone(&h_max),
            )));
        }
        children.push(Box::new(SvgZoomButton::new(
            "Custom",
            None,
            Arc::clone(&font),
            Arc::clone(&samples),
            Rc::clone(&zoom),
            Rc::clone(&v_offset),
            Rc::clone(&v_max),
            Rc::clone(&h_offset),
            Rc::clone(&h_max),
        )));
        Self {
            bounds: Rect::default(),
            children,
            font,
            samples,
            zoom,
        }
    }
}

impl SvgProgressBody {
    fn new(
        samples: Arc<Vec<SvgSampleRender>>,
        zoom: Rc<Cell<f64>>,
        v_offset: Rc<Cell<f64>>,
        v_max: Rc<Cell<f64>>,
        h_offset: Rc<Cell<f64>>,
        h_max: Rc<Cell<f64>>,
    ) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            samples,
            zoom,
            v_offset,
            v_max,
            h_offset,
            h_max,
        }
    }
}

impl SvgSampleRender {
    fn new(sample: &SvgSample) -> Self {
        let reference = decode_png_rgba(sample.reference_png);
        let (width, height) = reference
            .as_ref()
            .map(|(_, w, h)| (*w, *h))
            .unwrap_or((1, 1));

        let rgba = render_svg_to_framebuffer_at_size(sample.svg, width, height)
            .map(|fb| {
                let mut pixels = fb.pixels_flipped();
                unpremultiply_rgba_inplace(&mut pixels);
                Arc::new(pixels)
            })
            .map_err(|e| e.to_string());

        let lcd = render_svg_to_lcd_buffer_at_size(sample.svg, width, height)
            .map(|buffer| SvgLcdPreview {
                color: Arc::new(buffer.color_plane_flipped()),
                alpha: Arc::new(buffer.alpha_plane_flipped()),
            })
            .map_err(|e| e.to_string());

        Self {
            name: sample.name,
            svg: sample.svg,
            width,
            height,
            reference: reference.map(|(pixels, _, _)| Arc::new(pixels)),
            rgba,
            lcd,
        }
    }
}

impl Widget for SvgZoomButton {
    fn type_name(&self) -> &'static str {
        "SvgZoomButton"
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

    fn layout(&mut self, _: Size) -> Size {
        Size::new(if self.label == "Custom" { 58.0 } else { 48.0 }, 22.0)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let active = self.active();
        let fill = if active {
            v.accent
        } else if self.pressed {
            v.accent_pressed
        } else if self.hovered {
            v.widget_bg_hovered
        } else {
            v.widget_bg
        };
        let text_color = if active { Color::white() } else { v.text_color };

        ctx.set_fill_color(fill);
        ctx.set_stroke_color(if active { v.accent } else { v.widget_stroke });
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.rounded_rect(
            0.5,
            0.5,
            self.bounds.width - 1.0,
            self.bounds.height - 1.0,
            6.0,
        );
        ctx.fill_and_stroke();

        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(10.5);
        ctx.set_fill_color(text_color);
        let metrics = ctx.measure_text(self.label);
        let text_w = metrics
            .as_ref()
            .map(|m| m.width)
            .unwrap_or(self.label.len() as f64 * 6.0);
        let baseline_y = metrics
            .as_ref()
            .map(|m| m.centered_baseline_y(self.bounds.height))
            .unwrap_or(7.0);
        ctx.fill_text(self.label, (self.bounds.width - text_w) * 0.5, baseline_y);
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseMove { pos } => {
                let hovered = self.contains(*pos);
                if self.hovered != hovered {
                    self.hovered = hovered;
                    agg_gui::animation::request_tick();
                }
                EventResult::Ignored
            }
            Event::MouseDown {
                pos,
                button: MouseButton::Left,
                ..
            } if self.contains(*pos) => {
                self.pressed = true;
                agg_gui::animation::request_tick();
                EventResult::Consumed
            }
            Event::MouseUp {
                pos,
                button: MouseButton::Left,
                ..
            } => {
                let was_pressed = self.pressed;
                self.pressed = false;
                if was_pressed && self.contains(*pos) {
                    if let Some(target_zoom) = self.target_zoom {
                        zoom_svg_around_viewport_center(
                            &self.samples,
                            &self.zoom,
                            &self.v_offset,
                            &self.v_max,
                            &self.h_offset,
                            &self.h_max,
                            target_zoom,
                        );
                    } else {
                        agg_gui::animation::request_tick();
                    }
                    EventResult::Consumed
                } else if was_pressed {
                    agg_gui::animation::request_tick();
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            _ => EventResult::Ignored,
        }
    }
}

impl Widget for SvgProgressHeader {
    fn type_name(&self) -> &'static str {
        "SvgProgressHeader"
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
        self.bounds = Rect::new(0.0, 0.0, available.width, SVG_HEADER_H);
        let mut x = SVG_PAD + 2.0;
        for child in &mut self.children {
            let size = child.layout(Size::new(78.0, 22.0));
            child.set_bounds(Rect::new(x, SVG_HEADER_H - 80.0, size.width, 22.0));
            x += size.width + 6.0;
        }
        Size::new(available.width, SVG_HEADER_H)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        ctx.set_font(Arc::clone(&self.font));
        let w = self.bounds.width;
        let h = self.bounds.height.max(self.min_content_height());
        let zoom = self.zoom.get();
        let col_w = column_width(&self.samples, w, zoom);
        let titles = [
            "reference.png / control",
            "agg-rgba-bitmap render",
            "agg-lcd-bitmap render",
            "hardware render",
        ];

        ctx.set_fill_color(v.widget_bg);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, w, h);
        ctx.fill();

        let title_y = h - 22.0;
        draw_small_text(
            ctx,
            "SVG renderer progress viewer",
            SVG_PAD + 2.0,
            title_y,
            13.0,
            v.text_color,
        );
        draw_small_text(
            ctx,
            "Headers are fixed; reference.png is from resvg-test-suite and every output is rendered/displayed at that native pixel size.",
            SVG_PAD + 2.0,
            title_y - 18.0,
            10.5,
            v.text_dim,
        );
        draw_small_text(
            ctx,
            &format!("Zoom: {:.0}%", zoom * 100.0),
            SVG_PAD + 2.0,
            h - 54.0,
            10.5,
            v.text_dim,
        );
        draw_small_text(
            ctx,
            "Ctrl+wheel zooms at cursor",
            SVG_PAD + 184.0,
            h - 74.0,
            10.5,
            v.text_dim,
        );

        let header_y = h - SVG_TITLE_H - 22.0;
        ctx.set_fill_color(v.window_title_fill);
        ctx.begin_path();
        ctx.rect(
            0.0,
            h - SVG_TITLE_H - SVG_COLUMN_HEADER_H,
            w,
            SVG_COLUMN_HEADER_H,
        );
        ctx.fill();
        for (i, title) in titles.iter().enumerate() {
            let x = SVG_PAD + i as f64 * (col_w + SVG_GAP);
            draw_small_text(ctx, title, x + 6.0, header_y, 10.5, v.text_color);
        }
    }

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

impl Widget for SvgProgressBody {
    fn type_name(&self) -> &'static str {
        "SvgProgressBody"
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

    fn layout(&mut self, _: Size) -> Size {
        let zoom = self.zoom.get();
        let size = Size::new(
            self.min_content_width_at(zoom),
            self.min_content_height_at(zoom),
        );
        self.bounds = Rect::new(0.0, 0.0, size.width, size.height);
        size
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let zoom = self.zoom.get();
        let w = self.bounds.width.max(self.min_content_width_at(zoom));
        let h = self.bounds.height.max(self.min_content_height_at(zoom));
        let row_h = self.row_height_at(zoom);
        let col_w = column_width(&self.samples, w, zoom);

        ctx.set_fill_color(v.widget_bg);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, w, h);
        ctx.fill();

        for (row, sample) in self.samples.iter().enumerate() {
            let row_top = h - SVG_PAD - row as f64 * row_h;
            let y = row_top - row_h + 6.0;
            draw_small_text(
                ctx,
                sample.name,
                SVG_PAD + 6.0,
                row_top - 17.0,
                10.0,
                v.text_dim,
            );

            for col in 0..4 {
                let x = SVG_PAD + col as f64 * (col_w + SVG_GAP);
                draw_panel(ctx, x, y, col_w, row_h - 26.0, &v);
                match col {
                    0 => draw_raster_column(
                        ctx,
                        &sample.reference,
                        sample.width,
                        sample.height,
                        zoom,
                        x,
                        y,
                        col_w,
                        row_h - 26.0,
                        &v,
                    ),
                    1 => draw_raster_column(
                        ctx,
                        &sample.rgba,
                        sample.width,
                        sample.height,
                        zoom,
                        x,
                        y,
                        col_w,
                        row_h - 26.0,
                        &v,
                    ),
                    2 => draw_lcd_column(
                        ctx,
                        &sample.lcd,
                        sample.width,
                        sample.height,
                        zoom,
                        x,
                        y,
                        col_w,
                        row_h - 26.0,
                        &v,
                    ),
                    3 => draw_hardware_column(ctx, sample, zoom, x, y, col_w, row_h - 26.0, &v),
                    _ => {}
                }
            }
        }
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseWheel {
                pos,
                delta_y,
                modifiers,
                ..
            } if modifiers.ctrl => {
                let old_zoom = self.zoom.get();
                let factor = (-delta_y * 0.1).exp();
                let new_zoom = (old_zoom * factor).clamp(SVG_MIN_ZOOM, SVG_MAX_ZOOM);
                zoom_svg_around_content_point(
                    &self.samples,
                    &self.zoom,
                    &self.v_offset,
                    &self.v_max,
                    &self.h_offset,
                    &self.h_max,
                    pos.x,
                    svg_content_height(&self.samples, old_zoom) - pos.y,
                    new_zoom,
                );
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }
}

impl SvgProgressHeader {
    fn min_content_height(&self) -> f64 {
        SVG_HEADER_H
    }
}

impl SvgProgressBody {
    fn row_height_at(&self, zoom: f64) -> f64 {
        self.samples
            .iter()
            .map(|sample| sample.height as f64 * zoom)
            .fold(90.0, f64::max)
            + 26.0
    }

    fn min_content_width_at(&self, zoom: f64) -> f64 {
        svg_content_width(&self.samples, zoom)
    }

    fn min_content_height_at(&self, zoom: f64) -> f64 {
        SVG_PAD * 2.0 + self.row_height_at(zoom) * self.samples.len().max(1) as f64
    }
}

fn svg_content_width(samples: &[SvgSampleRender], zoom: f64) -> f64 {
    let max_sample_w = samples
        .iter()
        .map(|sample| sample.width as f64 * zoom)
        .fold(120.0, f64::max);
    SVG_PAD * 2.0 + (max_sample_w + 16.0) * 4.0 + SVG_GAP * 3.0
}

fn svg_content_height(samples: &[SvgSampleRender], zoom: f64) -> f64 {
    SVG_PAD * 2.0 + svg_row_height(samples, zoom) * samples.len().max(1) as f64
}

fn svg_row_height(samples: &[SvgSampleRender], zoom: f64) -> f64 {
    samples
        .iter()
        .map(|sample| sample.height as f64 * zoom)
        .fold(90.0, f64::max)
        + 26.0
}

fn column_width(samples: &[SvgSampleRender], available_width: f64, zoom: f64) -> f64 {
    let max_sample_w = samples
        .iter()
        .map(|sample| sample.width as f64 * zoom)
        .fold(120.0, f64::max);
    ((available_width - SVG_PAD * 2.0 - SVG_GAP * 3.0) / 4.0).max(max_sample_w + 16.0)
}

fn is_zoom_level(actual: f64, expected: f64) -> bool {
    (actual - expected).abs() < 0.001
}

fn zoom_svg_around_viewport_center(
    samples: &[SvgSampleRender],
    zoom: &Rc<Cell<f64>>,
    v_offset: &Rc<Cell<f64>>,
    v_max: &Rc<Cell<f64>>,
    h_offset: &Rc<Cell<f64>>,
    h_max: &Rc<Cell<f64>>,
    new_zoom: f64,
) {
    let old_zoom = zoom.get();
    let old_w = svg_content_width(samples, old_zoom);
    let old_h = svg_content_height(samples, old_zoom);
    let viewport_w = (old_w - h_max.get()).max(1.0);
    let viewport_h = (old_h - v_max.get()).max(1.0);
    zoom_svg_around_content_point(
        samples,
        zoom,
        v_offset,
        v_max,
        h_offset,
        h_max,
        h_offset.get() + viewport_w * 0.5,
        v_offset.get() + viewport_h * 0.5,
        new_zoom,
    );
}

fn zoom_svg_around_content_point(
    samples: &[SvgSampleRender],
    zoom: &Rc<Cell<f64>>,
    v_offset: &Rc<Cell<f64>>,
    v_max: &Rc<Cell<f64>>,
    h_offset: &Rc<Cell<f64>>,
    h_max: &Rc<Cell<f64>>,
    anchor_x: f64,
    anchor_top_y: f64,
    new_zoom: f64,
) {
    let old_zoom = zoom.get();
    if (new_zoom - old_zoom).abs() < 0.001 {
        return;
    }

    let old_w = svg_content_width(samples, old_zoom);
    let old_h = svg_content_height(samples, old_zoom);
    let new_w = svg_content_width(samples, new_zoom);
    let new_h = svg_content_height(samples, new_zoom);
    let viewport_w = (old_w - h_max.get()).max(1.0);
    let viewport_h = (old_h - v_max.get()).max(1.0);
    let screen_x = anchor_x - h_offset.get();
    let screen_top_y = anchor_top_y - v_offset.get();
    let new_h_max = (new_w - viewport_w).max(0.0);
    let new_v_max = (new_h - viewport_h).max(0.0);
    let scaled_anchor_x = anchor_x * (new_w / old_w.max(1.0));
    let scaled_anchor_top_y = anchor_top_y * (new_h / old_h.max(1.0));

    zoom.set(new_zoom);
    h_max.set(new_h_max);
    v_max.set(new_v_max);
    h_offset.set((scaled_anchor_x - screen_x).clamp(0.0, new_h_max));
    v_offset.set((scaled_anchor_top_y - screen_top_y).clamp(0.0, new_v_max));
    agg_gui::animation::request_tick();
}

#[cfg(test)]
mod svg_tests {
    use std::sync::Arc;

    use agg_gui::{find_widget_by_type, Event, Font, Modifiers, MouseButton, Point, Size};

    #[test]
    fn svg_test_keeps_header_fixed_above_bidirectional_scroll_area() {
        const BYTES: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");
        let font = Arc::new(Font::from_slice(BYTES).expect("parse CascadiaCode.ttf"));
        let mut root = super::svg_test(font);

        root.layout(Size::new(520.0, 260.0));

        let children = root.children();
        assert_eq!(children[0].type_name(), "SvgProgressHeader");
        assert_eq!(children[1].type_name(), "ScrollView");
        assert_eq!(children[0].bounds().height, super::SVG_HEADER_H);
        assert_eq!(children[0].children().len(), 3);
        for button in children[0].children() {
            assert!(
                button.bounds().y > super::SVG_COLUMN_HEADER_H,
                "zoom buttons should sit above the fixed column header"
            );
        }

        let scroll =
            find_widget_by_type(root.as_ref(), "ScrollView").expect("SVG Test scroll view");
        let props = scroll.properties();
        assert_property(&props, "v_enabled", "true");
        assert_property(&props, "h_enabled", "true");
        assert_property(&props, "bar_visibility", "AlwaysVisible");
        assert_positive_property(&props, "max_scroll");
        assert_positive_property(&props, "h_max_scroll");
    }

    #[test]
    fn svg_test_defaults_to_half_zoom() {
        const BYTES: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");
        let font = Arc::new(Font::from_slice(BYTES).expect("parse CascadiaCode.ttf"));
        let mut root = super::svg_test(font);

        root.layout(Size::new(520.0, 260.0));

        let scroll =
            find_widget_by_type(root.as_ref(), "ScrollView").expect("SVG Test scroll view");
        let props = scroll.properties();
        let h_content = property_value(&props, "h_content").parse::<f64>().unwrap();
        assert!(
            h_content < 1400.0,
            "SVG Test should default to 50% zoom, got h_content={h_content}"
        );
    }

    #[test]
    fn svg_zoom_buttons_change_to_their_own_targets() {
        const BYTES: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");
        let font = Arc::new(Font::from_slice(BYTES).expect("parse CascadiaCode.ttf"));
        let mut root = super::svg_test(font);
        root.layout(Size::new(520.0, 260.0));

        let default_content_w = svg_scroll_property(&root, "h_content");
        click_header_button(&mut root, 1);
        root.layout(Size::new(520.0, 260.0));
        let zoom_100_content_w = svg_scroll_property(&root, "h_content");
        assert!(
            zoom_100_content_w > default_content_w,
            "100% button should increase content width"
        );

        click_header_button(&mut root, 0);
        root.layout(Size::new(520.0, 260.0));
        let zoom_50_content_w = svg_scroll_property(&root, "h_content");
        assert!(
            zoom_50_content_w < zoom_100_content_w,
            "50% button should restore the smaller half-zoom content width"
        );
    }

    fn assert_property(props: &[(&'static str, String)], name: &str, expected: &str) {
        let actual = property_value(props, name);
        assert_eq!(actual, expected);
    }

    fn assert_positive_property(props: &[(&'static str, String)], name: &str) {
        let actual = property_value(props, name);
        let value = actual
            .parse::<f64>()
            .unwrap_or_else(|_| panic!("{name} should be a number, got {actual:?}"));
        assert!(value > 0.0, "{name} should be positive, got {value}");
    }

    fn property_value<'a>(props: &'a [(&'static str, String)], name: &str) -> &'a str {
        props
            .iter()
            .find_map(|(key, value)| (*key == name).then_some(value.as_str()))
            .unwrap_or_else(|| panic!("missing property {name}"))
    }

    fn click_header_button(root: &mut Box<dyn agg_gui::Widget>, index: usize) {
        let button = &mut root.children_mut()[0].children_mut()[index];
        let center = Point::new(button.bounds().width * 0.5, button.bounds().height * 0.5);
        let mods = Modifiers::default();
        button.on_event(&Event::MouseDown {
            pos: center,
            button: MouseButton::Left,
            modifiers: mods,
        });
        button.on_event(&Event::MouseUp {
            pos: center,
            button: MouseButton::Left,
            modifiers: mods,
        });
    }

    fn svg_scroll_property(root: &Box<dyn agg_gui::Widget>, name: &str) -> f64 {
        let scroll =
            find_widget_by_type(root.as_ref(), "ScrollView").expect("SVG Test scroll view");
        property_value(&scroll.properties(), name)
            .parse::<f64>()
            .unwrap_or_else(|_| panic!("{name} should be a number"))
    }
}

struct SvgSample {
    name: &'static str,
    svg: &'static [u8],
    reference_png: &'static [u8],
}

const SVG_SAMPLES: &[SvgSample] = &[
    SvgSample {
        name: "shapes/rect/simple-case.svg",
        svg: include_bytes!("../../../tests/resvg-test-suite/tests/shapes/rect/simple-case.svg"),
        reference_png: include_bytes!(
            "../../../tests/resvg-test-suite/tests/shapes/rect/simple-case.png"
        ),
    },
    SvgSample {
        name: "shapes/path/M-L-L-Z.svg",
        svg: include_bytes!("../../../tests/resvg-test-suite/tests/shapes/path/M-L-L-Z.svg"),
        reference_png: include_bytes!(
            "../../../tests/resvg-test-suite/tests/shapes/path/M-L-L-Z.png"
        ),
    },
    SvgSample {
        name: "painting/stroke/line-as-curve-1.svg",
        svg: include_bytes!(
            "../../../tests/resvg-test-suite/tests/painting/stroke/line-as-curve-1.svg"
        ),
        reference_png: include_bytes!(
            "../../../tests/resvg-test-suite/tests/painting/stroke/line-as-curve-1.png"
        ),
    },
    SvgSample {
        name: "structure/image/embedded-png.svg",
        svg: include_bytes!(
            "../../../tests/resvg-test-suite/tests/structure/image/embedded-png.svg"
        ),
        reference_png: include_bytes!(
            "../../../tests/resvg-test-suite/tests/structure/image/embedded-png.png"
        ),
    },
];

fn draw_panel(ctx: &mut dyn DrawCtx, x: f64, y: f64, w: f64, h: f64, v: &Visuals) {
    ctx.set_fill_color(v.panel_fill);
    ctx.begin_path();
    ctx.rounded_rect(x, y, w, h, 5.0);
    ctx.fill();
    ctx.set_stroke_color(Color::rgba(
        v.text_color.r,
        v.text_color.g,
        v.text_color.b,
        0.18,
    ));
    ctx.set_line_width(1.0);
    ctx.begin_path();
    ctx.rounded_rect(x, y, w, h, 5.0);
    ctx.stroke();
}

fn draw_raster_column(
    ctx: &mut dyn DrawCtx,
    pixels: &Result<Arc<Vec<u8>>, String>,
    img_w: u32,
    img_h: u32,
    zoom: f64,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    v: &Visuals,
) {
    match pixels {
        Ok(pixels) => {
            let (dx, dy, dw, dh) =
                native_rect(img_w as f64 * zoom, img_h as f64 * zoom, x, y, w, h);
            ctx.draw_image_rgba_arc(pixels, img_w, img_h, dx, dy, dw, dh);
        }
        Err(err) => draw_small_text(ctx, err, x + 8.0, y + h * 0.5, 9.0, v.text_dim),
    }
}

fn draw_lcd_column(
    ctx: &mut dyn DrawCtx,
    pixels: &Result<SvgLcdPreview, String>,
    img_w: u32,
    img_h: u32,
    zoom: f64,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    v: &Visuals,
) {
    match pixels {
        Ok(pixels) => {
            let (dx, dy, dw, dh) =
                native_rect(img_w as f64 * zoom, img_h as f64 * zoom, x, y, w, h);
            ctx.draw_lcd_backbuffer_arc(&pixels.color, &pixels.alpha, img_w, img_h, dx, dy, dw, dh);
        }
        Err(err) => draw_small_text(ctx, err, x + 8.0, y + h * 0.5, 9.0, v.text_dim),
    }
}

fn draw_hardware_column(
    ctx: &mut dyn DrawCtx,
    sample: &SvgSampleRender,
    zoom: f64,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    v: &Visuals,
) {
    let (dx, dy, _, _) = native_rect(
        sample.width as f64 * zoom,
        sample.height as f64 * zoom,
        x,
        y,
        w,
        h,
    );
    ctx.save();
    ctx.translate(dx, dy);
    ctx.scale(zoom, zoom);
    if let Err(err) = render_svg_at_size(sample.svg, ctx, sample.width, sample.height) {
        ctx.restore();
        draw_small_text(ctx, &err.to_string(), x + 8.0, y + h * 0.5, 9.0, v.text_dim);
        return;
    }
    ctx.restore();
}

fn native_rect(src_w: f64, src_h: f64, x: f64, y: f64, w: f64, h: f64) -> (f64, f64, f64, f64) {
    (x + (w - src_w) * 0.5, y + (h - src_h) * 0.5, src_w, src_h)
}

fn draw_small_text(ctx: &mut dyn DrawCtx, text: &str, x: f64, y: f64, size: f64, color: Color) {
    ctx.set_font_size(size);
    ctx.set_fill_color(color);
    ctx.fill_text(text, x, y);
}

fn decode_png_rgba(data: &[u8]) -> Result<(Vec<u8>, u32, u32), String> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(data));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0_u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    let src = &buf[..info.buffer_size()];
    let rgba = match info.color_type {
        png::ColorType::Rgba => src.to_vec(),
        png::ColorType::Rgb => {
            let mut out = Vec::with_capacity(info.width as usize * info.height as usize * 4);
            for chunk in src.chunks_exact(3) {
                out.extend_from_slice(chunk);
                out.push(255);
            }
            out
        }
        png::ColorType::Grayscale => {
            let mut out = Vec::with_capacity(info.width as usize * info.height as usize * 4);
            for &v in src {
                out.extend_from_slice(&[v, v, v, 255]);
            }
            out
        }
        png::ColorType::GrayscaleAlpha => {
            let mut out = Vec::with_capacity(info.width as usize * info.height as usize * 4);
            for chunk in src.chunks_exact(2) {
                out.extend_from_slice(&[chunk[0], chunk[0], chunk[0], chunk[1]]);
            }
            out
        }
        other => return Err(format!("unsupported PNG color type: {other:?}")),
    };

    Ok((rgba, info.width, info.height))
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
    pub title: String,
    pub content: Box<dyn Widget>,
    pub initial_rect: Rect,
    /// Window fits tightly to its content; ignores `resizable_*`.
    pub auto_size: bool,
    /// Master user-resize toggle.  `false` → no handles active.
    pub resizable: bool,
    /// Axis-specific locks (only consulted when `resizable` is `true`).
    pub resizable_h: bool,
    pub resizable_v: bool,
    /// Wrap content in a built-in vertical `ScrollView` at window
    /// build time.  Matches egui's `Window::vscroll(true)`.
    pub vscroll: bool,
    /// Resize floor + ceiling follow content natural height.
    /// Matches egui's no-scroll-no-clip-no-whitespace contract for
    /// W4 (window snaps to content height in both directions).
    pub tight_fit: bool,
    /// Resize FLOOR only follows content height; user can pull the
    /// window taller (whitespace below).  Used for W5 where a
    /// flex-fill `TextArea` absorbs extra space.
    pub floor_fit: bool,
}

impl ResizeTestWindow {
    fn new(title: &str, content: Box<dyn Widget>, initial_rect: Rect) -> Self {
        Self {
            title: title.into(),
            content,
            initial_rect,
            auto_size: false,
            resizable: true,
            resizable_h: true,
            resizable_v: true,
            vscroll: false,
            tight_fit: false,
            floor_fit: false,
        }
    }
    fn auto_sized(mut self) -> Self {
        self.auto_size = true;
        self.resizable = false;
        self
    }
    fn with_vscroll(mut self) -> Self {
        self.vscroll = true;
        self
    }
    fn with_tight_fit(mut self) -> Self {
        self.tight_fit = true;
        self
    }
    fn with_floor_fit(mut self) -> Self {
        self.floor_fit = true;
        self
    }
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
            .on_click(|| crate::url::open_url(RESIZE_TEST_SOURCE_URL)),
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
        Rect::new(30.0, 100.0, 360.0, 240.0),  // 1. ↔ auto-sized
        Rect::new(410.0, 100.0, 300.0, 290.0), // 2. ↔ resizable + scroll
        Rect::new(730.0, 100.0, 300.0, 290.0), // 3. ↔ resizable + embedded scroll
        Rect::new(30.0, 410.0, 300.0, 290.0),  // 4. ↔ resizable without scroll
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
            .with_gap(6.0)
            .with_padding(10.0)
            .with_panel_bg()
            .with_fit_width(true);
        // Keep explanatory text from dictating the auto-sized
        // window's width: W1 should track the inner Resize width
        // plus padding, while fixed-width wrapped prose can grow
        // taller instead of leaving stale right-side whitespace.
        root.push(
            Box::new(
                SizedBox::new().with_width(320.0).with_child(Box::new(
                    Label::new(
                        "This window will auto-size based on its contents.",
                        Arc::clone(&font),
                    )
                    .with_font_size(12.0)
                    .with_wrap(true),
                )),
            ),
            0.0,
        );
        root.push(
            Box::new(Label::new("Resize this area:", Arc::clone(&font)).with_font_size(14.0)),
            0.0,
        );
        // The lorem ipsum INSIDE the Resize widget still wraps so it
        // reshapes as the user narrows / widens the Resize.  The
        // Resize widget enforces a content-natural minimum so the
        // wrapped text can never be clipped.  `top_anchor` keeps the
        // text at the top of the Resize frame when the user pulls it
        // taller — without this, FlexColumn's default natural-anchor
        // would leave the text pinned to the BOTTOM of the frame
        // with whitespace above (the bug visible in image #24).
        let mut inner = FlexColumn::new()
            .with_gap(4.0)
            .with_padding(8.0)
            .with_fit_width(true)
            .with_top_anchor(true);
        inner.push(
            Box::new(
                Label::new(LOREM_IPSUM, Arc::clone(&font))
                    .with_font_size(11.5)
                    .with_wrap(true),
            ),
            0.0,
        );
        // No explicit max_size_hint here — we want the user to be
        // able to drag the inner Resize all the way to the canvas
        // extent, letting the outer auto-sized Window grow with it.
        // The `Window::auto_size` clamp to `available.width` caps
        // final growth at the surrounding layout's inner width.
        root.push(
            Box::new(
                Resize::new(Box::new(inner))
                    .with_default_size(Size::new(320.0, 120.0))
                    .with_min_size_hint(Size::new(120.0, 60.0))
                    .with_max_size_hint(Size::new(4000.0, 3000.0)),
            ),
            0.0,
        );
        root.push(
            Box::new(Label::new("Resize the above area!", Arc::clone(&font)).with_font_size(14.0)),
            0.0,
        );
        root.push(source_link(Arc::clone(&font)), 0.0);
        out.push(ResizeTestWindow::new("↔ auto-sized", Box::new(root), rects[0]).auto_sized());
    }

    // ── 2. ↔ resizable + scroll ──────────────────────────────────────────────
    //
    // Window-level vscroll (egui's `.vscroll(true)`).  No manual
    // ScrollView in the content tree — the `Window::with_vscroll(true)`
    // call in `lib.rs` (Stage 2) wraps `root` itself in a vertical
    // ScrollView at builder time.  The inner content is a single
    // overflowing FlexColumn so the scroll bar has range.
    {
        let mut root = FlexColumn::new()
            .with_gap(8.0)
            .with_padding(10.0)
            .with_panel_bg();
        root.push(
            Box::new(
                Label::new(
                    "This window is resizable and has a scroll area. You can shrink it \
             to any size.",
                    Arc::clone(&font),
                )
                .with_font_size(12.0)
                .with_wrap(true),
            ),
            0.0,
        );
        root.push(Box::new(Separator::horizontal()), 0.0);
        root.push(
            Box::new(
                Label::new(LOREM_IPSUM_LONG, Arc::clone(&font))
                    .with_font_size(11.5)
                    .with_wrap(true),
            ),
            0.0,
        );
        root.push(source_link(Arc::clone(&font)), 0.0);
        out.push(
            ResizeTestWindow::new("↔ resizable + scroll", Box::new(root), rects[1]).with_vscroll(),
        );
    }

    // ── 3. ↔ resizable + embedded scroll ────────────────────────────────────
    {
        let mut root = FlexColumn::new()
            .with_gap(8.0)
            .with_padding(10.0)
            .with_panel_bg();
        root.push(
            Box::new(
                Label::new(
                    "This window is resizable but has no built-in scroll area.",
                    Arc::clone(&font),
                )
                .with_font_size(12.0)
                .with_wrap(true),
            ),
            0.0,
        );
        root.push(
            Box::new(
                Label::new(
                    "However, we have a sub-region with a scroll bar:",
                    Arc::clone(&font),
                )
                .with_font_size(12.0)
                .with_wrap(true),
            ),
            0.0,
        );
        root.push(Box::new(Separator::horizontal()), 0.0);
        let long2 = format!("{}\n\n{}", LOREM_IPSUM_LONG, LOREM_IPSUM_LONG);
        let mut inner = FlexColumn::new().with_gap(4.0).with_padding(4.0);
        inner.push(
            Box::new(
                Label::new(&long2, Arc::clone(&font))
                    .with_font_size(11.5)
                    .with_wrap(true),
            ),
            0.0,
        );
        root.push(Box::new(ScrollView::new(Box::new(inner))), 1.0);
        root.push(source_link(Arc::clone(&font)), 0.0);
        out.push(ResizeTestWindow::new(
            "↔ resizable + embedded scroll",
            Box::new(root),
            rects[2],
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
        let mut root = FlexColumn::new()
            .with_gap(8.0)
            .with_padding(10.0)
            .with_panel_bg();
        root.push(
            Box::new(
                Label::new(
                    "This window is resizable but has no scroll area. This means it \
             can only be resized to a size where all the contents is visible.",
                    Arc::clone(&font),
                )
                .with_font_size(12.0)
                .with_wrap(true),
            ),
            0.0,
        );
        root.push(
            Box::new(
                Label::new(
                    "agg-gui will not clip the contents of a window, nor add \
             whitespace to it.",
                    Arc::clone(&font),
                )
                .with_font_size(12.0)
                .with_wrap(true),
            ),
            0.0,
        );
        root.push(Box::new(Separator::horizontal()), 0.0);
        root.push(
            Box::new(
                Label::new(LOREM_IPSUM, Arc::clone(&font))
                    .with_font_size(11.5)
                    .with_wrap(true),
            ),
            0.0,
        );
        root.push(source_link(Arc::clone(&font)), 0.0);
        out.push(
            ResizeTestWindow::new("↔ resizable without scroll", Box::new(root), rects[3])
                .with_tight_fit(),
        );
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
        let mut root = FlexColumn::new()
            .with_gap(8.0)
            .with_padding(10.0)
            .with_panel_bg();
        root.push(
            Box::new(
                Label::new(
                    "Shows how you can fill an area with a widget.",
                    Arc::clone(&font),
                )
                .with_font_size(12.0)
                .with_wrap(true),
            ),
            0.0,
        );
        root.push(
            Box::new(
                TextArea::new(Arc::clone(&font))
                    .with_font_size(12.5)
                    .with_text(LOREM_IPSUM),
            ),
            1.0,
        );
        root.push(source_link(Arc::clone(&font)), 0.0);
        out.push(
            ResizeTestWindow::new("↔ resizable with TextEdit", Box::new(root), rects[4])
                .with_floor_fit(),
        );
    }

    // ── 6. ↔ freely resized ─────────────────────────────────────────────────
    {
        let mut root = FlexColumn::new()
            .with_gap(8.0)
            .with_padding(10.0)
            .with_panel_bg();
        root.push(
            Box::new(
                Label::new(
                    "This window has empty space that fills up the available space, \
             preventing auto-shrink.",
                    Arc::clone(&font),
                )
                .with_font_size(12.0)
                .with_wrap(true),
            ),
            0.0,
        );
        root.push(source_link(Arc::clone(&font)), 0.0);
        root.push(Box::new(SizedBox::new()), 1.0);
        out.push(ResizeTestWindow::new(
            "↔ freely resized",
            Box::new(root),
            rects[5],
        ));
    }

    out
}
