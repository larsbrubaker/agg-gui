//! Interaction demo windows: drag-and-drop, scrolling, panels layout, popups,
//! scene viewer, and screenshot info.
//!
//! These demos show stateful interaction patterns — shared state via
//! `Rc<Cell<…>>`, custom painting, and event handling — without animation.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{
    Button, Color, Container, DrawCtx, Event, EventResult,
    FlexColumn, FlexRow, Font, Label,
    MouseButton, Point, Rect, ScrollView, Separator,
    Size, SizedBox, Widget,
};

// ---------------------------------------------------------------------------
// Drag and Drop demo
// ---------------------------------------------------------------------------

/// A single column of draggable items.  Clicking an item moves it to the next
/// column (A → B → C → A) via a shared `Rc<RefCell<Option<…>>>` transfer cell.
struct DndColumn {
    bounds:   Rect,
    children: Vec<Box<dyn Widget>>,
    font:     Arc<Font>,
    label:    &'static str,
    items:    Vec<String>,
    /// Shared transfer: when Some((target_col, item_text)) another column signals
    /// that an item should be received here.
    transfer: Rc<RefCell<Option<(usize, String)>>>,
    /// This column's index (0=A, 1=B, 2=C).
    col_idx:  usize,
    hovered:  Option<usize>,
}

impl Widget for DndColumn {
    fn type_name(&self) -> &'static str { "DndColumn" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size {
        // Accept any pending transfer targeted at this column.
        let pending = self.transfer.borrow().as_ref().and_then(|(col, item)| {
            if *col == self.col_idx { Some((*col, item.clone())) } else { None }
        });
        if let Some((_col, item)) = pending {
            self.items.push(item);
            *self.transfer.borrow_mut() = None;
        }
        self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
        available
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let w = self.bounds.width;
        let h = self.bounds.height;

        // Column background.
        ctx.set_fill_color(v.panel_fill);
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, w, h, 6.0);
        ctx.fill();
        ctx.set_stroke_color(v.widget_stroke);
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, w, h, 6.0);
        ctx.stroke();

        // Column header.
        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(12.0);
        ctx.set_fill_color(v.text_dim);
        ctx.fill_text(self.label, 8.0, h - 18.0);

        // Items (Y-up: draw from top, so highest y first).
        let item_h = 28.0_f64;
        let pad    = 6.0_f64;
        let start_y = h - 32.0; // below header area

        ctx.set_font_size(12.5);
        for (i, item) in self.items.iter().enumerate() {
            let y = start_y - (i as f64 + 1.0) * (item_h + 2.0);
            if y < 0.0 { break; }

            let is_hovered = self.hovered == Some(i);
            let bg = if is_hovered { v.widget_bg_hovered } else { v.widget_bg };
            ctx.set_fill_color(bg);
            ctx.begin_path();
            ctx.rounded_rect(pad, y, w - pad * 2.0, item_h, 4.0);
            ctx.fill();

            ctx.set_fill_color(v.text_color);
            let text = format!("\u{2261} {}", item); // ≡ Item N
            ctx.fill_text(&text, pad + 6.0, y + item_h * 0.35 + 4.0);
        }
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        let item_h  = 28.0_f64;
        let pad     = 6.0_f64;
        let start_y = self.bounds.height - 32.0;

        // Map a local Y coordinate to an item index (Y-up layout).
        let y_to_item = |y: f64, count: usize| -> Option<usize> {
            for i in 0..count {
                let iy = start_y - (i as f64 + 1.0) * (item_h + 2.0);
                if iy < 0.0 { break; }
                if y >= iy && y <= iy + item_h { return Some(i); }
            }
            None
        };

        match event {
            Event::MouseMove { pos } => {
                let prev = self.hovered;
                if pos.x >= pad && pos.x <= self.bounds.width - pad {
                    self.hovered = y_to_item(pos.y, self.items.len());
                } else {
                    self.hovered = None;
                }
                if self.hovered != prev { EventResult::Consumed } else { EventResult::Ignored }
            }
            Event::MouseDown { pos, button: MouseButton::Left, .. } => {
                if let Some(idx) = y_to_item(pos.y, self.items.len()) {
                    let item = self.items.remove(idx);
                    let next_col = (self.col_idx + 1) % 3;
                    *self.transfer.borrow_mut() = Some((next_col, item));
                    self.hovered = None;
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }
            _ => EventResult::Ignored,
        }
    }

    fn hit_test(&self, p: Point) -> bool {
        p.x >= 0.0 && p.x <= self.bounds.width
            && p.y >= 0.0 && p.y <= self.bounds.height
    }
}

/// Build the Drag and Drop demo — three columns (A, B, C) with items.
/// Clicking an item moves it to the next column.
pub fn drag_and_drop(font: Arc<Font>) -> Box<dyn Widget> {
    // Shared transfer cell: (target_column_index, item_text).
    let transfer: Rc<RefCell<Option<(usize, String)>>> = Rc::new(RefCell::new(None));

    let items_a = vec!["Item 1".into(), "Item 2".into(), "Item 3".into()];
    let items_b = vec!["Item 4".into(), "Item 5".into()];
    let items_c = vec!["Item 6".into()];

    let col_a = DndColumn {
        bounds: Rect::default(), children: Vec::new(),
        font: Arc::clone(&font), label: "Column A",
        items: items_a, transfer: Rc::clone(&transfer), col_idx: 0, hovered: None,
    };
    let col_b = DndColumn {
        bounds: Rect::default(), children: Vec::new(),
        font: Arc::clone(&font), label: "Column B",
        items: items_b, transfer: Rc::clone(&transfer), col_idx: 1, hovered: None,
    };
    let col_c = DndColumn {
        bounds: Rect::default(), children: Vec::new(),
        font: Arc::clone(&font), label: "Column C",
        items: items_c, transfer: Rc::clone(&transfer), col_idx: 2, hovered: None,
    };

    let mut outer = FlexColumn::new()
        .with_gap(10.0)
        .with_padding(12.0)
        .with_panel_bg();

    outer.push(Box::new(Label::new(
        "Click an item to move it to the next column",
        Arc::clone(&font),
    ).with_font_size(11.5)), 0.0);

    let row = FlexRow::new().with_gap(8.0)
        .add_flex(Box::new(col_a), 1.0)
        .add_flex(Box::new(col_b), 1.0)
        .add_flex(Box::new(col_c), 1.0);

    outer.push(Box::new(row), 1.0);
    Box::new(outer)
}

// ---------------------------------------------------------------------------
// Scrolling demo
// ---------------------------------------------------------------------------

/// Build the Scrolling demo — a tall list of labelled rows inside a ScrollView.
pub fn scrolling_demo(font: Arc<Font>) -> Box<dyn Widget> {
    let mut outer = FlexColumn::new()
        .with_gap(10.0)
        .with_padding(12.0)
        .with_panel_bg();

    outer.push(Box::new(Label::new(
        "50 rows inside a ScrollView",
        Arc::clone(&font),
    ).with_font_size(12.0)), 0.0);

    let mut inner = FlexColumn::new().with_gap(4.0).with_padding(6.0);
    for i in 0..50_usize {
        let bg = if i % 2 == 0 {
            Color::rgba(0.0, 0.0, 0.0, 0.04)
        } else {
            Color::rgba(0.0, 0.0, 0.0, 0.0)
        };
        let row = Container::new()
            .with_background(bg)
            .with_padding(4.0)
            .add(Box::new(Label::new(
                format!("Row {}", i + 1),
                Arc::clone(&font),
            ).with_font_size(12.5)));
        inner.push(Box::new(row), 0.0);
    }

    outer.push(Box::new(ScrollView::new(Box::new(inner))), 1.0);
    Box::new(outer)
}

// ---------------------------------------------------------------------------
// Panels demo
// ---------------------------------------------------------------------------

/// A simple panels layout: top, bottom, left, right, and center areas.
struct PanelsLayout {
    bounds:   Rect,
    children: Vec<Box<dyn Widget>>,
    font:     Arc<Font>,
}

impl Widget for PanelsLayout {
    fn type_name(&self) -> &'static str { "PanelsLayout" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
        available
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v  = ctx.visuals();
        let w  = self.bounds.width;
        let h  = self.bounds.height;
        let tp = 36.0_f64; // top panel height
        let bp = 36.0_f64; // bottom panel height
        let lp = 72.0_f64; // left panel width
        let rp = 72.0_f64; // right panel width

        // Colors.
        let top_bg    = Color::rgba(v.accent.r, v.accent.g, v.accent.b, 0.25);
        let bot_bg    = Color::rgba(0.0, 0.0, 0.0, 0.12);
        let side_bg   = v.panel_fill;
        let center_bg = v.bg_color;

        let draw_panel = |ctx: &mut dyn DrawCtx, x: f64, y: f64, pw: f64, ph: f64,
                          bg: Color, label: &str, font: &Arc<Font>| {
            ctx.set_fill_color(bg);
            ctx.begin_path();
            ctx.rect(x, y, pw, ph);
            ctx.fill();
            ctx.set_stroke_color(v.separator);
            ctx.set_line_width(1.0);
            ctx.begin_path();
            ctx.rect(x, y, pw, ph);
            ctx.stroke();
            ctx.set_font(Arc::clone(font));
            ctx.set_font_size(11.5);
            ctx.set_fill_color(v.text_dim);
            ctx.fill_text(label, x + 6.0, y + ph * 0.4 + 4.0);
        };

        // Top panel (Y-up: top = h - tp … h).
        draw_panel(ctx, 0.0, h - tp, w, tp, top_bg, "Top Panel", &self.font);
        // Bottom panel (Y-up: y=0 … bp).
        draw_panel(ctx, 0.0, 0.0, w, bp, bot_bg, "Bottom Panel", &self.font);
        // Left panel.
        draw_panel(ctx, 0.0, bp, lp, h - tp - bp, side_bg, "Left", &self.font);
        // Right panel.
        draw_panel(ctx, w - rp, bp, rp, h - tp - bp, side_bg, "Right", &self.font);
        // Center.
        let cx = lp;
        let cy = bp;
        let cw = w - lp - rp;
        let ch = h - tp - bp;
        draw_panel(ctx, cx, cy, cw, ch, center_bg, "Central panel", &self.font);
    }

    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}

/// Build the Panels demo — a static five-area panel layout.
pub fn panels_demo(font: Arc<Font>) -> Box<dyn Widget> {
    Box::new(PanelsLayout {
        bounds: Rect::default(), children: Vec::new(), font,
    })
}

// ---------------------------------------------------------------------------
// Popups demo
// ---------------------------------------------------------------------------

/// Build the Popups demo — a button that reveals an inline "popup" panel.
pub fn popups_demo(font: Arc<Font>) -> Box<dyn Widget> {
    let open = Rc::new(Cell::new(false));

    let mut col = FlexColumn::new()
        .with_gap(10.0)
        .with_padding(14.0)
        .with_panel_bg();

    col.push(Box::new(Label::new("Popups demo", Arc::clone(&font))
        .with_font_size(12.0)), 0.0);

    {
        let open_for_btn = Rc::clone(&open);
        col.push(Box::new(SizedBox::new().with_height(30.0).with_child(Box::new(
            Button::new("Open popup", Arc::clone(&font))
                .with_font_size(13.0)
                .on_click(move || { open_for_btn.set(true); })
        ))), 0.0);
    }

    // Inline popup panel (shown when open == true).
    let popup_panel = InlinePopup {
        bounds: Rect::default(), children: Vec::new(),
        font: Arc::clone(&font), open: Rc::clone(&open),
    };
    col.push(Box::new(popup_panel), 0.0);

    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);
    Box::new(col)
}

/// An inline panel that is only visible (and takes space) when `open` is true.
struct InlinePopup {
    bounds:   Rect,
    children: Vec<Box<dyn Widget>>,
    font:     Arc<Font>,
    open:     Rc<Cell<bool>>,
}

impl Widget for InlinePopup {
    fn type_name(&self) -> &'static str { "InlinePopup" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size {
        if !self.open.get() {
            self.bounds = Rect::new(0.0, 0.0, 0.0, 0.0);
            return Size::new(0.0, 0.0);
        }
        let h = 90.0_f64;
        self.bounds = Rect::new(0.0, 0.0, available.width, h);
        Size::new(available.width, h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        if !self.open.get() { return; }
        let v = ctx.visuals();
        let w = self.bounds.width;
        let h = self.bounds.height;

        ctx.set_fill_color(v.widget_bg);
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, w, h, 6.0);
        ctx.fill();
        ctx.set_stroke_color(v.widget_stroke);
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, w, h, 6.0);
        ctx.stroke();

        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(13.0);
        ctx.set_fill_color(v.text_color);
        ctx.fill_text("Popup is open!", 10.0, h - 22.0);
        ctx.set_font_size(11.0);
        ctx.set_fill_color(v.text_dim);
        ctx.fill_text("Click 'Close' to dismiss.", 10.0, h - 42.0);
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        if !self.open.get() { return EventResult::Ignored; }
        // A simple "close" area: clicking anywhere in the bottom half dismisses.
        if let Event::MouseDown { pos, button: MouseButton::Left, .. } = event {
            if pos.y <= self.bounds.height * 0.45 {
                self.open.set(false);
                return EventResult::Consumed;
            }
        }
        EventResult::Ignored
    }
}

// ---------------------------------------------------------------------------
// Scene demo
// ---------------------------------------------------------------------------

/// A custom scene viewer showing circles and rectangles with hover highlight.
struct SceneWidget {
    bounds:   Rect,
    children: Vec<Box<dyn Widget>>,
    cursor:   Option<Point>,
    /// (cx, cy, radius) for each circle.
    circles:  [(f64, f64, f64); 6],
    /// (x, y, w, h) for each rectangle.
    rects:    [(f64, f64, f64, f64); 4],
}

impl SceneWidget {
    fn new() -> Self {
        Self {
            bounds:   Rect::default(),
            children: Vec::new(),
            cursor:   None,
            circles: [
                (60.0,  80.0, 22.0),
                (130.0, 60.0, 16.0),
                (200.0, 90.0, 28.0),
                (80.0,  160.0, 18.0),
                (170.0, 170.0, 24.0),
                (250.0, 130.0, 14.0),
            ],
            rects: [
                (20.0,  20.0, 50.0, 30.0),
                (110.0, 30.0, 40.0, 22.0),
                (210.0, 50.0, 60.0, 28.0),
                (150.0, 200.0, 45.0, 20.0),
            ],
        }
    }

    fn nearest_circle(&self, p: Point) -> Option<usize> {
        self.circles.iter().enumerate().find(|(_, &(cx, cy, r))| {
            let dx = p.x - cx;
            let dy = p.y - cy;
            dx * dx + dy * dy <= r * r
        }).map(|(i, _)| i)
    }

    fn nearest_rect(&self, p: Point) -> Option<usize> {
        self.rects.iter().enumerate().find(|(_, &(x, y, w, h))| {
            p.x >= x && p.x <= x + w && p.y >= y && p.y <= y + h
        }).map(|(i, _)| i)
    }
}

impl Widget for SceneWidget {
    fn type_name(&self) -> &'static str { "SceneWidget" }
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

        ctx.set_fill_color(v.bg_color);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, w, h);
        ctx.fill();

        let cursor = self.cursor;

        // Draw rectangles.
        for (i, &(rx, ry, rw, rh)) in self.rects.iter().enumerate() {
            let hov = cursor.map_or(false, |p| self.nearest_rect(p) == Some(i));
            let fill = if hov { v.accent_hovered } else { v.widget_bg };
            ctx.set_fill_color(fill);
            ctx.begin_path();
            ctx.rounded_rect(rx, ry, rw, rh, 3.0);
            ctx.fill();
            ctx.set_stroke_color(if hov { v.accent } else { v.widget_stroke });
            ctx.set_line_width(1.0);
            ctx.begin_path();
            ctx.rounded_rect(rx, ry, rw, rh, 3.0);
            ctx.stroke();
        }

        // Draw circles.
        for (i, &(cx, cy, r)) in self.circles.iter().enumerate() {
            let hov = cursor.map_or(false, |p| self.nearest_circle(p) == Some(i));
            let fill = if hov { v.accent } else { Color::rgba(v.accent.r, v.accent.g, v.accent.b, 0.55) };
            ctx.set_fill_color(fill);
            ctx.begin_path();
            ctx.circle(cx, cy, r);
            ctx.fill();
        }
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseMove { pos } => {
                self.cursor = Some(*pos);
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

/// Build the Scene demo — a custom-painted canvas with hover highlighting.
pub fn scene_demo(font: Arc<Font>) -> Box<dyn Widget> {
    let mut col = FlexColumn::new()
        .with_gap(8.0)
        .with_padding(8.0)
        .with_panel_bg();

    col.push(Box::new(Label::new(
        "Hover shapes to highlight them",
        Arc::clone(&font),
    ).with_font_size(11.5)), 0.0);
    col.push(Box::new(Label::new(
        "Pan: middle-drag, Zoom: scroll (not yet implemented)",
        Arc::clone(&font),
    ).with_font_size(11.0)), 0.0);
    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(Box::new(SceneWidget::new()), 1.0);
    Box::new(col)
}

// ---------------------------------------------------------------------------
// Screenshot demo
// ---------------------------------------------------------------------------

/// Build the Screenshot demo — informational placeholder.
pub fn screenshot_demo(font: Arc<Font>) -> Box<dyn Widget> {
    let mut col = FlexColumn::new()
        .with_gap(12.0)
        .with_padding(16.0)
        .with_panel_bg();

    col.push(Box::new(Label::new(
        "Screenshot functionality is platform-dependent.",
        Arc::clone(&font),
    ).with_font_size(13.0)), 0.0);

    col.push(Box::new(Label::new(
        "On desktop targets, screenshots can be captured via the OS screenshot\n\
         tool or a dedicated crate (e.g. `screenshots`).  WASM targets can use\n\
         the HTML Canvas `toDataURL()` API.\n\n\
         In-framework screenshot capture is not yet implemented.",
        Arc::clone(&font),
    ).with_font_size(12.0)), 0.0);

    Box::new(col)
}
