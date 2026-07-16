//! Test window implementations for egui-inspired diagnostic windows.
//!
//! These are diagnostic/test widgets that verify framework behaviour.  The
//! Clipboard Test performs real copy operations through
//! [`agg_gui::clipboard`]; the remaining widgets exercise cursor, input and
//! event plumbing directly.

#![allow(unused_imports)]
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::framebuffer::unpremultiply_rgba_inplace;
use agg_gui::widget::paint_subtree;
use agg_gui::{
    clipboard, render_svg_at_size, render_svg_to_framebuffer_at_size,
    render_svg_to_lcd_buffer_at_size, set_cursor_icon, Button, Checkbox, Color, Container,
    CursorIcon, DrawCtx, Event, EventResult, FlexColumn, FlexRow, Font, Hyperlink, ImageView,
    Label, MouseButton, Point, Rect, Resize, ScrollBarVisibility, ScrollView, Separator, Size,
    SizedBox, TextArea, TextField, Visuals, Widget,
};

// ---------------------------------------------------------------------------
// Clipboard Test
// ---------------------------------------------------------------------------

/// Generate a small RGBA test image (a two-axis color gradient) used by the
/// image-copy row.  The same buffer is displayed via [`ImageView`] and handed
/// to [`clipboard::set_image_rgba`], so what you see is exactly what is copied.
fn generate_test_image(w: u32, h: u32) -> Vec<u8> {
    let mut data = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let r = (255 * x / w.max(1)) as u8;
            let g = (255 * y / h.max(1)) as u8;
            data.extend_from_slice(&[r, g, 160, 255]);
        }
    }
    data
}

/// Build the Clipboard Test — mirrors egui's `tests/clipboard_test.rs`: an
/// editable text field with a 📋 copy button that writes to the system
/// clipboard, plus an image-copy row.  The keyboard shortcut legend documents
/// the text field's real copy/cut/paste behaviour.
pub fn clipboard_test(font: Arc<Font>) -> Box<dyn Widget> {
    let mut col = FlexColumn::new()
        .with_gap(14.0)
        .with_padding(16.0)
        .with_panel_bg();

    col.push(
        Box::new(
            Label::new(
                "agg-gui integrates with the system clipboard.",
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

    // The text field mirrors its content into a shared cell so the 📋 button
    // can copy the live text (egui's `ui.copy_text(self.text.clone())`).
    let text_cell = Rc::new(RefCell::new(
        "Example text you can copy-and-paste".to_string(),
    ));
    let copy_cell = Rc::clone(&text_cell);
    let initial_text = text_cell.borrow().clone();
    let row = FlexRow::new()
        .with_gap(10.0)
        .add_flex(
            Box::new(
                SizedBox::new().with_height(32.0).with_child(Box::new(
                    TextField::new(Arc::clone(&font))
                        .with_font_size(13.0)
                        .with_text(initial_text)
                        .on_change(move |s| *text_cell.borrow_mut() = s.to_string()),
                )),
            ),
            1.0,
        )
        .add(Box::new(
            SizedBox::new().with_height(32.0).with_child(Box::new(
                Button::new("\u{F0C5}", Arc::clone(&font))
                    .with_font_size(13.0)
                    .on_click(move || clipboard::set_text(&copy_cell.borrow())),
            )),
        ));
    col.push(Box::new(row), 0.0);

    // ── Image copy ───────────────────────────────────────────────────────────
    // egui shows an image next to a 📋 button that copies it. We generate a
    // small RGBA gradient, display it, and copy the identical buffer.
    col.push(
        Box::new(
            Label::new("You can also copy images:", Arc::clone(&font)).with_font_size(12.0),
        ),
        0.0,
    );
    const IMG_W: u32 = 48;
    const IMG_H: u32 = 48;
    let image_data = Rc::new(generate_test_image(IMG_W, IMG_H));
    let image_source = Rc::new(RefCell::new(Some((
        (*image_data).clone(),
        IMG_W,
        IMG_H,
    ))));
    let copy_image = Rc::clone(&image_data);
    let image_row = FlexRow::new()
        .with_gap(10.0)
        .add(Box::new(
            SizedBox::new()
                .with_width(48.0)
                .with_height(48.0)
                .with_child(Box::new(
                    ImageView::new(Arc::clone(&font), image_source).with_outline(true),
                )),
        ))
        .add(Box::new(
            SizedBox::new().with_height(32.0).with_child(Box::new(
                Button::new("\u{F0C5}", Arc::clone(&font))
                    .with_font_size(13.0)
                    .on_click(move || {
                        clipboard::set_image_rgba(&copy_image, IMG_W, IMG_H);
                    }),
            )),
        ));
    col.push(Box::new(image_row), 0.0);

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
/// The row's hover background is drawn directly; the cursor name renders
/// through a real `Label` child so its glyph cache stays warm.
struct CursorRow {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    icon: CursorIcon,
    hovered: bool,
}

impl CursorRow {
    const H: f64 = 24.0;

    fn new(icon: CursorIcon, name: &'static str, font: Arc<Font>) -> Self {
        Self {
            bounds: Rect::default(),
            children: vec![Box::new(Label::new(name, font).with_font_size(12.0))],
            icon,
            hovered: false,
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
        if let Some(child) = self.children.first_mut() {
            let s = child.layout(Size::new(available.width, Self::H));
            // Centre the label within the row.
            let lx = (available.width - s.width) * 0.5;
            let ly = (Self::H - s.height) * 0.5;
            child.set_bounds(Rect::new(lx, ly, s.width, s.height));
        }
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
        if let Some(child) = self.children.first_mut() {
            child.set_label_color(v.text_color);
        }
        // Label child paints itself via the framework's tree walk.
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
// Input Event History
// ---------------------------------------------------------------------------

/// A deduplicated history row: consecutive identical `summary`s coalesce into
/// one entry with an incrementing `count` (egui's `DeduplicatedHistory`).
/// `full` keeps the detailed debug string (position, etc.) of the most recent
/// occurrence — retained for a future per-row tooltip (see the module notes).
struct HistoryEntry {
    summary: String,
    count: usize,
    full: String,
}

const EVENT_LINE_H: f64 = 18.0;
const EVENT_HISTORY_CAP: usize = 1000;

/// Records raw input events (deduplicated) and renders them as a tall content
/// widget meant to live inside a [`ScrollView`].  `include_movements` gates
/// `MouseMove` recording, matching egui's "Include pointer/mouse movements"
/// checkbox (off by default).
struct EventHistoryWidget {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    font: Arc<Font>,
    /// Newest at index 0.
    history: Vec<HistoryEntry>,
    include_movements: Rc<Cell<bool>>,
    /// Shared with the wrapping `ScrollView` so the content can size itself to
    /// at least fill the viewport (keeps the whole box interactive/recordable
    /// even when there are only a handful of events).
    viewport: Rc<Cell<Rect>>,
}

impl EventHistoryWidget {
    fn new(font: Arc<Font>, include_movements: Rc<Cell<bool>>, viewport: Rc<Cell<Rect>>) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            font,
            history: Vec::new(),
            include_movements,
            viewport,
        }
    }

    /// Add an event, coalescing with the newest entry if the summary matches.
    fn add(&mut self, summary: String, full: String) {
        if let Some(first) = self.history.first_mut() {
            if first.summary == summary {
                first.count += 1;
                first.full = full;
                return;
            }
        }
        self.history.insert(
            0,
            HistoryEntry {
                summary,
                count: 1,
                full,
            },
        );
        self.history.truncate(EVENT_HISTORY_CAP);
    }

    fn content_height(&self) -> f64 {
        let rows = self.history.len().max(1) as f64;
        let natural = rows * EVENT_LINE_H + 12.0;
        natural.max(self.viewport.get().height)
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
        // Inside a ScrollView we're asked for our natural height; report the
        // content height so the ScrollView can scroll it.
        let h = self.content_height();
        self.bounds = Rect::new(0.0, 0.0, available.width, h);
        Size::new(available.width, h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let w = self.bounds.width;
        let content_h = self.bounds.height;

        ctx.set_fill_color(v.widget_bg);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, w, content_h);
        ctx.fill();

        if self.history.is_empty() {
            ctx.set_font(Arc::clone(&self.font));
            ctx.set_font_size(11.0);
            ctx.set_fill_color(v.text_dim);
            ctx.fill_text(
                "Interact inside the box to record events…",
                8.0,
                content_h - 22.0,
            );
            return;
        }

        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(11.0);

        // Newest at the top (Y-up: highest y). Row i sits at content_h - (i+1)*line.
        for (i, entry) in self.history.iter().enumerate() {
            let y = content_h - (i as f64 + 1.0) * EVENT_LINE_H;
            if y + EVENT_LINE_H < 0.0 {
                break;
            }
            ctx.set_fill_color(v.text_color);
            ctx.fill_text(&entry.summary, 6.0, y + 4.0);
            if entry.count >= 2 {
                let sw = ctx
                    .measure_text(&entry.summary)
                    .map(|m| m.width)
                    .unwrap_or(0.0);
                ctx.set_fill_color(v.text_dim);
                ctx.fill_text(&format!(" \u{00d7}{}", entry.count), 6.0 + sw + 2.0, y + 4.0);
            }
        }
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        // Build a dedup summary (kind) and a full debug string (with detail).
        let (summary, full, is_move, consume) = match event {
            Event::MouseMove { pos } => (
                "MouseMove".to_string(),
                format!("MouseMove ({:.0}, {:.0})", pos.x, pos.y),
                true,
                false,
            ),
            Event::MouseDown { pos, button, .. } => (
                format!("MouseDown {button:?}"),
                format!("MouseDown {:?} ({:.0}, {:.0})", button, pos.x, pos.y),
                false,
                true,
            ),
            Event::MouseUp { button, .. } => {
                (format!("MouseUp {button:?}"), format!("MouseUp {button:?}"), false, true)
            }
            Event::KeyDown { key, .. } => {
                (format!("KeyDown {key:?}"), format!("KeyDown {key:?}"), false, true)
            }
            Event::KeyUp { key, .. } => {
                (format!("KeyUp {key:?}"), format!("KeyUp {key:?}"), false, true)
            }
            // Record wheel but let it bubble so the ScrollView can scroll.
            Event::MouseWheel { delta_y, .. } => (
                "MouseWheel".to_string(),
                format!("MouseWheel {delta_y:.1}"),
                false,
                false,
            ),
            _ => return EventResult::Ignored,
        };

        if is_move && !self.include_movements.get() {
            return EventResult::Ignored;
        }

        self.add(summary, full);
        agg_gui::animation::request_draw();
        if consume {
            EventResult::Consumed
        } else {
            EventResult::Ignored
        }
    }

    fn hit_test(&self, p: Point) -> bool {
        p.x >= 0.0 && p.x <= self.bounds.width && p.y >= 0.0 && p.y <= self.bounds.height
    }
}

/// Build the Input Event History — records raw input events (deduplicated,
/// with an ×N repeat counter) in a scrollable list, matching egui's
/// `input_event_history.rs`.
pub fn input_event_history(font: Arc<Font>) -> Box<dyn Widget> {
    let include_movements = Rc::new(Cell::new(false));
    let viewport = Rc::new(Cell::new(Rect::default()));

    let mut col = FlexColumn::new()
        .with_gap(8.0)
        .with_padding(10.0)
        .with_panel_bg();

    col.push(
        Box::new(
            Label::new(
                "Recent history of raw input events. Consecutive identical \
                 events coalesce with an ×N counter.",
                Arc::clone(&font),
            )
            .with_font_size(11.5)
            .with_wrap(true),
        ),
        0.0,
    );

    col.push(
        Box::new(
            Checkbox::new(
                "Include pointer/mouse movements",
                Arc::clone(&font),
                include_movements.get(),
            )
            .with_font_size(12.0)
            .with_state_cell(Rc::clone(&include_movements)),
        ),
        0.0,
    );

    let recorder = EventHistoryWidget::new(
        Arc::clone(&font),
        Rc::clone(&include_movements),
        Rc::clone(&viewport),
    );
    let scroll = ScrollView::new(Box::new(recorder))
        .vertical(true)
        .horizontal(false)
        .with_viewport_cell(Rc::clone(&viewport));
    col.push(Box::new(scroll), 1.0);
    Box::new(col)
}
