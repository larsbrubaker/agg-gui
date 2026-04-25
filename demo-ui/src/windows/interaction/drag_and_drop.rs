use std::sync::Arc;

use agg_gui::{
    set_cursor_icon, Color, CursorIcon, DrawCtx, Event, EventResult, FlexColumn, Font, Label,
    MouseButton, Point, Rect, Size, Widget,
};

// ---------------------------------------------------------------------------
// Drag and Drop demo
// ---------------------------------------------------------------------------

// Layout constants shared by DragAndDropWidget painting and hit-testing.
const DND_HEADER_H: f64 = 26.0;
const DND_ITEM_H: f64 = 26.0;
const DND_ITEM_GAP: f64 = 3.0;
const DND_PAD: f64 = 5.0;
const DND_COL_GAP: f64 = 8.0;
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
        if cursor_y > dnd_item_y_mid(col_h, i) {
            return i;
        }
    }
    n
}

/// A single drag-and-drop demo that manages three columns internally.
///
/// Implements true mouse-drag mechanics: press and hold on an item, move the
/// mouse to a target column (an insertion preview line appears), then release
/// to drop.  Items can be reordered within and between columns.
struct DragAndDropWidget {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    font: Arc<Font>,

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
            bounds: Rect::default(),
            children: Vec::new(),
            font,
            columns: vec![
                vec![
                    "Item A".into(),
                    "Item B".into(),
                    "Item C".into(),
                    "Item D".into(),
                ],
                vec!["Item E".into(), "Item F".into(), "Item G".into()],
                vec![
                    "Item H".into(),
                    "Item I".into(),
                    "Item J".into(),
                    "Item K".into(),
                ],
            ],
            drag_active: false,
            drag_source: None,
            cursor: Point::ORIGIN,
            press_pos: Point::ORIGIN,
            drop_target: None,
            hovered: None,
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
            if pos.x < cr.x || pos.x > cr.x + cr.width {
                continue;
            }
            let local_x = pos.x - cr.x;
            let local_y = pos.y;
            if local_x < DND_PAD || local_x > cr.width - DND_PAD {
                continue;
            }
            for (i, _) in col.iter().enumerate() {
                let yb = dnd_item_y_bottom(h, i);
                let yt = yb + DND_ITEM_H;
                if local_y >= yb && local_y <= yt {
                    return Some((c, i));
                }
            }
        }
        None
    }

    /// Find drop target (column + insertion row) given cursor position.
    fn find_drop_target(&self, pos: Point) -> Option<(usize, usize)> {
        let w = self.bounds.width;
        let h = self.bounds.height;
        for c in 0..self.columns.len() {
            let cr = self.col_rect(c, w, h);
            if pos.x >= cr.x && pos.x <= cr.x + cr.width {
                let local_y = pos.y;
                let visible_len = self.visible_len(c);
                let row = dnd_find_insert_row(local_y, h, visible_len);
                return Some((c, row));
            }
        }
        None
    }

    fn visible_len(&self, col: usize) -> usize {
        let len = self.columns[col].len();
        if self.drag_active && self.drag_source.map(|(c, _)| c) == Some(col) {
            len.saturating_sub(1)
        } else {
            len
        }
    }

    /// Execute the pending drop: move `drag_source` item to `drop_target`.
    fn commit_drop(&mut self) {
        let (sc, sr) = match self.drag_source.take() {
            Some(x) => x,
            None => return,
        };
        let (tc, mut tr) = match self.drop_target.take() {
            Some(x) => x,
            None => return,
        };
        if sc >= self.columns.len() || sr >= self.columns[sc].len() {
            return;
        }
        if sc == tc && sr < tr {
            tr -= 1;
        }
        let item = self.columns[sc].remove(sr);
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
                    ctx.set_fill_color(Color::rgba(v.accent.r, v.accent.g, v.accent.b, 0.08));
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
        let (drag_src_col, drag_src_row) = self
            .drag_source
            .map(|(sc, sr)| (sc, Some(sr)))
            .unwrap_or((usize::MAX, None));

        ctx.set_font_size(12.5);
        let mut visual_i = 0_usize;
        for (i, item) in self.columns[c].iter().enumerate() {
            // Skip the item being dragged (show as ghost at cursor instead).
            if self.drag_active && c == drag_src_col && Some(i) == drag_src_row {
                continue;
            }
            let yb = dnd_item_y_bottom(h, visual_i);
            visual_i += 1;
            if yb + DND_ITEM_H < 0.0 {
                continue;
            }

            let is_hov = !self.drag_active && self.hovered == Some((c, i));
            let bg = if is_hov {
                v.widget_bg_hovered
            } else {
                v.widget_bg
            };
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
                    let effective_n = self.visible_len(c);

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
    fn type_name(&self) -> &'static str {
        "DragAndDropWidget"
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
                        v.widget_bg.r,
                        v.widget_bg.g,
                        v.widget_bg.b,
                        0.85,
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
                    set_cursor_icon(if self.drag_active {
                        CursorIcon::Grabbing
                    } else {
                        CursorIcon::Grab
                    });
                    let dx = pos.x - self.press_pos.x;
                    let dy = pos.y - self.press_pos.y;
                    if !self.drag_active && (dx * dx + dy * dy).sqrt() >= DND_DRAG_THRESHOLD {
                        self.drag_active = true;
                    }
                    if self.drag_active {
                        self.drop_target = self.find_drop_target(*pos);
                    }
                    agg_gui::animation::request_tick();
                    return EventResult::Consumed;
                }

                // Not dragging — update hover.
                let prev = self.hovered;
                self.hovered = self.col_row_at(*pos);
                if self.hovered.is_some() {
                    set_cursor_icon(CursorIcon::Grab);
                }
                if self.hovered != prev {
                    agg_gui::animation::request_tick();
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }

            Event::MouseDown {
                button: MouseButton::Left,
                pos,
                ..
            } => {
                self.press_pos = *pos;
                self.cursor = *pos;
                self.drag_active = false;
                // Find which item was pressed.
                if let Some((c, r)) = self.col_row_at(*pos) {
                    self.drag_source = Some((c, r));
                    self.drop_target = Some((c, r));
                    set_cursor_icon(CursorIcon::Grabbing);
                    agg_gui::animation::request_tick();
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }

            Event::MouseUp {
                button: MouseButton::Left,
                pos,
                ..
            } => {
                self.cursor = *pos;
                if self.drag_active {
                    self.drop_target = self.find_drop_target(*pos);
                    self.commit_drop();
                }
                let had_drag_state = self.drag_active || self.drag_source.is_some();
                self.drag_active = false;
                self.drag_source = None;
                self.drop_target = None;
                if had_drag_state {
                    agg_gui::animation::request_tick();
                }
                EventResult::Consumed
            }

            _ => EventResult::Ignored,
        }
    }

    fn hit_test(&self, p: Point) -> bool {
        p.x >= 0.0 && p.x <= self.bounds.width && p.y >= 0.0 && p.y <= self.bounds.height
    }
}

/// Build the Drag and Drop demo — three columns (A, B, C) with draggable items.
/// Drag an item to a new position within or between columns.
pub fn drag_and_drop(font: Arc<Font>) -> Box<dyn Widget> {
    let mut outer = FlexColumn::new()
        .with_gap(10.0)
        .with_padding(12.0)
        .with_panel_bg();

    outer.push(
        Box::new(
            Label::new(
                "This is a simple example of drag-and-drop in agg-gui.",
                Arc::clone(&font),
            )
            .with_font_size(11.5),
        ),
        0.0,
    );
    outer.push(
        Box::new(Label::new("Drag items between columns.", Arc::clone(&font)).with_font_size(11.5)),
        0.0,
    );

    outer.push(Box::new(DragAndDropWidget::new(Arc::clone(&font))), 1.0);
    Box::new(outer)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_font() -> Arc<Font> {
        const BYTES: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");
        Arc::new(Font::from_slice(BYTES).expect("parse CascadiaCode.ttf"))
    }

    #[test]
    fn same_column_drop_adjusts_removed_row() {
        let mut widget = DragAndDropWidget::new(test_font());
        widget.drag_active = true;
        widget.drag_source = Some((0, 1));
        widget.drop_target = Some((0, 3));

        widget.commit_drop();

        assert_eq!(widget.columns[0], ["Item A", "Item C", "Item B", "Item D"]);
    }

    #[test]
    fn cross_column_drop_keeps_destination_row() {
        let mut widget = DragAndDropWidget::new(test_font());
        widget.drag_active = true;
        widget.drag_source = Some((0, 1));
        widget.drop_target = Some((1, 1));

        widget.commit_drop();

        assert_eq!(widget.columns[0], ["Item A", "Item C", "Item D"]);
        assert_eq!(widget.columns[1], ["Item E", "Item B", "Item F", "Item G"]);
    }
}
