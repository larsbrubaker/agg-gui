//! Interaction demo windows: drag-and-drop, scrolling, panels layout, popups,
//! scene viewer, and screenshot info.
//!
//! These demos show stateful interaction patterns — shared state via
//! `Rc<Cell<…>>`, custom painting, and event handling — without animation.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{
    Button, Color, DrawCtx, Event, EventResult,
    FlexColumn, Font, Label,
    MouseButton, Point, Rect, Separator,
    Size, SizedBox, Widget,
};
use agg_gui::widget::paint_subtree;

// ---------------------------------------------------------------------------
// Drag and Drop demo
// ---------------------------------------------------------------------------

// Layout constants shared by DragAndDropWidget painting and hit-testing.
const DND_HEADER_H: f64 = 26.0;
const DND_ITEM_H:   f64 = 26.0;
const DND_ITEM_GAP: f64 = 3.0;
const DND_PAD:      f64 = 5.0;
const DND_COL_GAP:  f64 = 8.0;
/// Minimum cursor movement (px) before a click becomes a drag.
const DND_DRAG_THRESHOLD: f64 = 4.0;

/// Returns the Y-up bottom edge of item `i` within a column of height `h`.
fn dnd_item_y_bottom(h: f64, i: usize) -> f64 {
    h - DND_HEADER_H - (i as f64 + 1.0) * DND_ITEM_H - i as f64 * DND_ITEM_GAP
}

/// Returns the midpoint Y of item `i` within a column of height `h`.
fn dnd_item_y_mid(h: f64, i: usize) -> f64 {
    dnd_item_y_bottom(h, i) + DND_ITEM_H * 0.5
}

/// Given a cursor Y within a column, return the insertion index (0 = before all items).
fn dnd_find_insert_row(cursor_y: f64, col_h: f64, n: usize) -> usize {
    for i in 0..n {
        if cursor_y > dnd_item_y_mid(col_h, i) { return i; }
    }
    n
}

/// A single drag-and-drop demo that manages three columns internally.
///
/// Implements true mouse-drag mechanics: press and hold on an item, move the
/// mouse to a target column (an insertion preview line appears), then release
/// to drop.  Items can be reordered within and between columns.
struct DragAndDropWidget {
    bounds:   Rect,
    children: Vec<Box<dyn Widget>>,
    font:     Arc<Font>,

    /// Items for each of the 3 columns.
    columns: Vec<Vec<String>>,

    // --- drag state ---
    /// True once cursor has moved past DND_DRAG_THRESHOLD from press position.
    drag_active: bool,
    /// Column/row the drag started from (set on MouseDown, cleared on MouseUp).
    drag_source: Option<(usize, usize)>,
    /// Current cursor position in widget-local coordinates.
    cursor: Point,
    /// Where the mouse button was pressed (local coords).
    press_pos: Point,

    // --- drop target (recomputed each MouseMove) ---
    /// Target column and insertion row (0 = before all).
    drop_target: Option<(usize, usize)>,

    // --- hover (when not dragging) ---
    hovered: Option<(usize, usize)>, // (col, row)
}

impl DragAndDropWidget {
    fn new(font: Arc<Font>) -> Self {
        Self {
            bounds:   Rect::default(),
            children: Vec::new(),
            font,
            columns: vec![
                vec!["Item A".into(), "Item B".into(), "Item C".into(), "Item D".into()],
                vec!["Item E".into(), "Item F".into(), "Item G".into()],
                vec!["Item H".into(), "Item I".into(), "Item J".into(), "Item K".into()],
            ],
            drag_active:  false,
            drag_source:  None,
            cursor:       Point::ORIGIN,
            press_pos:    Point::ORIGIN,
            drop_target:  None,
            hovered:      None,
        }
    }

    /// Compute the X position and width of column `c` given total widget width `w`.
    fn col_rect(&self, c: usize, w: f64, h: f64) -> Rect {
        let n = self.columns.len() as f64;
        let col_w = (w - DND_COL_GAP * (n - 1.0)) / n;
        let x = c as f64 * (col_w + DND_COL_GAP);
        Rect::new(x, 0.0, col_w, h)
    }

    /// Find which (column, row) the cursor is over.  Returns `None` if not over
    /// any item.  Only used when NOT dragging (for hover highlight).
    fn col_row_at(&self, pos: Point) -> Option<(usize, usize)> {
        let w = self.bounds.width;
        let h = self.bounds.height;
        for (c, col) in self.columns.iter().enumerate() {
            let cr = self.col_rect(c, w, h);
            if pos.x < cr.x || pos.x > cr.x + cr.width { continue; }
            let local_x = pos.x - cr.x;
            let local_y = pos.y;
            if local_x < DND_PAD || local_x > cr.width - DND_PAD { continue; }
            for (i, _) in col.iter().enumerate() {
                let yb = dnd_item_y_bottom(h, i);
                let yt = yb + DND_ITEM_H;
                if local_y >= yb && local_y <= yt { return Some((c, i)); }
            }
        }
        None
    }

    /// Find drop target (column + insertion row) given cursor position.
    fn find_drop_target(&self, pos: Point) -> Option<(usize, usize)> {
        let w = self.bounds.width;
        let h = self.bounds.height;
        for (c, col) in self.columns.iter().enumerate() {
            let cr = self.col_rect(c, w, h);
            if pos.x >= cr.x && pos.x <= cr.x + cr.width {
                let local_y = pos.y;
                let row = dnd_find_insert_row(local_y, h, col.len());
                return Some((c, row));
            }
        }
        None
    }

    /// Execute the pending drop: move `drag_source` item to `drop_target`.
    fn commit_drop(&mut self) {
        let (sc, sr) = match self.drag_source.take() { Some(x) => x, None => return };
        let (tc, mut tr) = match self.drop_target.take() { Some(x) => x, None => return };
        if sc >= self.columns.len() || sr >= self.columns[sc].len() { return; }
        let item = self.columns[sc].remove(sr);
        // Adjust insertion index if moving within the same column and shifting left.
        if sc == tc && sr < tr { tr -= 1; }
        let col = &mut self.columns[tc];
        tr = tr.min(col.len());
        col.insert(tr, item);
    }

    /// Paint a single column.
    fn paint_column(&self, ctx: &mut dyn DrawCtx, c: usize, col_r: Rect) {
        let v = ctx.visuals();
        let w = col_r.width;
        let h = col_r.height;

        // Column background.
        ctx.set_fill_color(v.panel_fill);
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, w, h, 6.0);
        ctx.fill();

        // Highlight the column when it's the drop target.
        if self.drag_active {
            if let Some((tc, _)) = self.drop_target {
                if tc == c {
                    ctx.set_fill_color(Color::rgba(
                        v.accent.r, v.accent.g, v.accent.b, 0.08,
                    ));
                    ctx.begin_path();
                    ctx.rounded_rect(0.0, 0.0, w, h, 6.0);
                    ctx.fill();
                }
            }
        }

        ctx.set_stroke_color(v.widget_stroke);
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, w, h, 6.0);
        ctx.stroke();

        // Header label.
        let labels = ["Column A", "Column B", "Column C"];
        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(11.0);
        ctx.set_fill_color(v.text_dim);
        let header_y = h - DND_HEADER_H * 0.5 - 5.0;
        ctx.fill_text(labels[c], DND_PAD + 2.0, header_y);

        // Items.
        let (drag_src_col, drag_src_row) = self.drag_source
            .map(|(sc, sr)| (sc, Some(sr)))
            .unwrap_or((usize::MAX, None));

        ctx.set_font_size(12.5);
        for (i, item) in self.columns[c].iter().enumerate() {
            // Skip the item being dragged (show as ghost at cursor instead).
            if self.drag_active && c == drag_src_col && Some(i) == drag_src_row {
                continue;
            }
            let yb = dnd_item_y_bottom(h, i);
            if yb + DND_ITEM_H < 0.0 { continue; }

            let is_hov = !self.drag_active && self.hovered == Some((c, i));
            let bg = if is_hov { v.widget_bg_hovered } else { v.widget_bg };
            ctx.set_fill_color(bg);
            ctx.begin_path();
            ctx.rounded_rect(DND_PAD, yb, w - DND_PAD * 2.0, DND_ITEM_H, 4.0);
            ctx.fill();

            ctx.set_fill_color(v.text_color);
            let text = format!("\u{2261}  {}", item);
            ctx.fill_text(&text, DND_PAD + 8.0, yb + DND_ITEM_H * 0.35 + 4.0);
        }

        // Insertion line (when dragging and this column is the drop target).
        if self.drag_active {
            if let Some((tc, tr)) = self.drop_target {
                if tc == c {
                    let n = self.columns[c].len();
                    // Account for the missing dragged item in this column.
                    let effective_n = if c == drag_src_col && drag_src_row.is_some() {
                        n.saturating_sub(1)
                    } else { n };

                    let line_y = if tr == 0 {
                        // Above all items.
                        dnd_item_y_bottom(h, 0) + DND_ITEM_H
                    } else if tr >= effective_n {
                        // Below all items.
                        dnd_item_y_bottom(h, effective_n.saturating_sub(1))
                    } else {
                        // Between items.
                        dnd_item_y_bottom(h, tr) + DND_ITEM_H + DND_ITEM_GAP * 0.5
                    };

                    ctx.set_stroke_color(v.text_color);
                    ctx.set_line_width(2.0);
                    ctx.begin_path();
                    ctx.move_to(DND_PAD, line_y);
                    ctx.line_to(w - DND_PAD, line_y);
                    ctx.stroke();
                }
            }
        }
    }
}

impl Widget for DragAndDropWidget {
    fn type_name(&self) -> &'static str { "DragAndDropWidget" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
        available
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let w = self.bounds.width;
        let h = self.bounds.height;

        for c in 0..self.columns.len() {
            let cr = self.col_rect(c, w, h);
            ctx.save();
            ctx.translate(cr.x, cr.y);
            self.paint_column(ctx, c, cr);
            ctx.restore();
        }

        // Drag ghost: a semi-transparent copy of the dragged item following the cursor.
        if self.drag_active {
            if let Some((sc, sr)) = self.drag_source {
                if let Some(item) = self.columns[sc].get(sr) {
                    let v = ctx.visuals();
                    let ghost_w = 100.0_f64;
                    let ghost_h = DND_ITEM_H;
                    let gx = self.cursor.x - ghost_w * 0.5;
                    let gy = self.cursor.y - ghost_h * 0.5;

                    ctx.set_fill_color(Color::rgba(
                        v.widget_bg.r, v.widget_bg.g, v.widget_bg.b, 0.85,
                    ));
                    ctx.begin_path();
                    ctx.rounded_rect(gx, gy, ghost_w, ghost_h, 4.0);
                    ctx.fill();
                    ctx.set_stroke_color(v.accent);
                    ctx.set_line_width(1.5);
                    ctx.begin_path();
                    ctx.rounded_rect(gx, gy, ghost_w, ghost_h, 4.0);
                    ctx.stroke();

                    ctx.set_font(Arc::clone(&self.font));
                    ctx.set_font_size(12.0);
                    ctx.set_fill_color(v.text_color);
                    let text = format!("\u{2261}  {}", item);
                    ctx.fill_text(&text, gx + 8.0, gy + ghost_h * 0.35 + 4.0);
                }
            }
        }
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseMove { pos } => {
                self.cursor = *pos;

                if self.drag_source.is_some() {
                    let dx = pos.x - self.press_pos.x;
                    let dy = pos.y - self.press_pos.y;
                    if !self.drag_active && (dx * dx + dy * dy).sqrt() >= DND_DRAG_THRESHOLD {
                        self.drag_active = true;
                    }
                    if self.drag_active {
                        self.drop_target = self.find_drop_target(*pos);
                    }
                    return EventResult::Consumed;
                }

                // Not dragging — update hover.
                let prev = self.hovered;
                self.hovered = self.col_row_at(*pos);
                if self.hovered != prev { EventResult::Consumed } else { EventResult::Ignored }
            }

            Event::MouseDown { button: MouseButton::Left, pos, .. } => {
                self.press_pos = *pos;
                self.cursor    = *pos;
                self.drag_active = false;
                // Find which item was pressed.
                if let Some((c, r)) = self.col_row_at(*pos) {
                    self.drag_source = Some((c, r));
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }

            Event::MouseUp { button: MouseButton::Left, pos, .. } => {
                self.cursor = *pos;
                if self.drag_active {
                    self.drop_target = self.find_drop_target(*pos);
                    self.commit_drop();
                }
                self.drag_active = false;
                self.drag_source = None;
                self.drop_target = None;
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

/// Build the Drag and Drop demo — three columns (A, B, C) with draggable items.
/// Drag an item to a new position within or between columns.
pub fn drag_and_drop(font: Arc<Font>) -> Box<dyn Widget> {
    let mut outer = FlexColumn::new()
        .with_gap(10.0)
        .with_padding(12.0)
        .with_panel_bg();

    outer.push(Box::new(Label::new(
        "This is a simple example of drag-and-drop in agg-gui.",
        Arc::clone(&font),
    ).with_font_size(11.5)), 0.0);
    outer.push(Box::new(Label::new(
        "Drag items between columns.",
        Arc::clone(&font),
    ).with_font_size(11.5)), 0.0);

    outer.push(Box::new(DragAndDropWidget::new(Arc::clone(&font))), 1.0);
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
        let bot_bg    = v.track_bg;
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
    let popup_panel = InlinePopup::new(Arc::clone(&font), Rc::clone(&open));
    col.push(Box::new(popup_panel), 0.0);

    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);
    Box::new(col)
}

/// An inline panel that is only visible (and takes space) when `open` is true.
///
/// Text is rendered through backbuffered Label children so rasterization
/// is cached to a framebuffer and never repeated while the text is unchanged.
struct InlinePopup {
    bounds:   Rect,
    children: Vec<Box<dyn Widget>>,
    open:     Rc<Cell<bool>>,
    /// "Popup is open!" — body label.
    label_title: Label,
    /// "Click 'Close' to dismiss." — hint label.
    label_hint:  Label,
}

impl InlinePopup {
    fn new(font: Arc<Font>, open: Rc<Cell<bool>>) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            open,
            label_title: Label::new("Popup is open!", Arc::clone(&font)).with_font_size(13.0),
            label_hint:  Label::new("Click inside the popup to dismiss.", Arc::clone(&font)).with_font_size(11.0),
        }
    }
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

        // Layout labels — position them within the popup panel.
        let title_s = self.label_title.layout(Size::new(available.width - 20.0, 24.0));
        self.label_title.set_bounds(Rect::new(10.0, h - title_s.height - 14.0, title_s.width, title_s.height));

        let hint_s = self.label_hint.layout(Size::new(available.width - 20.0, 20.0));
        self.label_hint.set_bounds(Rect::new(10.0, h - title_s.height - hint_s.height - 24.0, hint_s.width, hint_s.height));

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

        // Paint labels via backbuffered Label children.
        self.label_title.set_color(v.text_color);
        let tb = self.label_title.bounds();
        ctx.save(); ctx.translate(tb.x, tb.y);
        paint_subtree(&mut self.label_title, ctx);
        ctx.restore();

        self.label_hint.set_color(v.text_dim);
        let hb = self.label_hint.bounds();
        ctx.save(); ctx.translate(hb.x, hb.y);
        paint_subtree(&mut self.label_hint, ctx);
        ctx.restore();
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
