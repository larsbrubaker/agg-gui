//! Test window implementations for egui-inspired diagnostic windows.
//!
//! These are diagnostic/test widgets that verify framework behaviour.  Where
//! native capabilities (clipboard, OS cursors, SVG) are not yet wired up, a
//! clear informational placeholder is shown instead of broken code.

#![allow(unused_imports)]
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

/// Build the Clipboard Test — egui wording with agg-gui's editable TextField.
pub fn clipboard_test(font: Arc<Font>) -> Box<dyn Widget> {
    let mut col = FlexColumn::new()
        .with_gap(14.0)
        .with_padding(16.0)
        .with_panel_bg();

    col.push(
        Box::new(
            Label::new(
                "egui integrates with the system clipboard.",
                Arc::clone(&font),
            )
            .with_font_size(12.0),
        ),
        0.0,
    );
    col.push(
        Box::new(
            Label::new(
                "Try copy-cut-pasting text in the text edit below.",
                Arc::clone(&font),
            )
            .with_font_size(12.0)
            .with_wrap(true),
        ),
        0.0,
    );

    let row = FlexRow::new().with_gap(10.0).add_flex(
        Box::new(
            SizedBox::new().with_height(32.0).with_child(Box::new(
                TextField::new(Arc::clone(&font))
                    .with_font_size(13.0)
                    .with_text("Example text you can copy-and-paste"),
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
