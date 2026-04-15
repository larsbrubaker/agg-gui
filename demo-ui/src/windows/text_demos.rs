//! Text-related and layout demo windows: scrolling rows, strip layout, table,
//! text layout showcase, undo/redo, window options, modals, and multi-touch info.
//!
//! Most demos here are purely compositional — they build a widget tree from
//! `FlexColumn`, `FlexRow`, `Container`, `Label`, etc. without custom painting.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{
    Button, Checkbox, Color, Container, DrawCtx, Event, EventResult,
    FlexColumn, FlexRow, Font, Label,
    MouseButton, Point, Rect, ScrollView, Separator,
    Size, SizedBox, TextField, Widget,
};
use agg_gui::widget::paint_subtree;

// ---------------------------------------------------------------------------
// Strip demo
// ---------------------------------------------------------------------------

/// A fixed-width labeled box used to visualise "strip" regions.
///
/// Text is rendered through a backbuffered Label child so the glyph rasterization
/// is cached to a framebuffer rather than repeated each frame.
struct StripCell {
    bounds:       Rect,
    children:     Vec<Box<dyn Widget>>,
    label_widget: Label,
    bg:           Color,
    w:            f64,
    h:            f64,
}

impl StripCell {
    fn new(label: impl Into<String>, font: Arc<Font>, bg: Color, w: f64, h: f64) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            label_widget: Label::new(label, font).with_font_size(11.0),
            bg,
            w,
            h,
        }
    }
}

impl Widget for StripCell {
    fn type_name(&self) -> &'static str { "StripCell" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, _available: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, self.w, self.h);
        // Position the label at 4px from the left, vertically centered.
        let ls = self.label_widget.layout(Size::new(self.w - 8.0, self.h));
        let ly = (self.h - ls.height) * 0.5;
        self.label_widget.set_bounds(Rect::new(4.0, ly, ls.width, ls.height));
        Size::new(self.w, self.h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        ctx.set_fill_color(self.bg);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, self.w, self.h);
        ctx.fill();
        ctx.set_stroke_color(v.widget_stroke);
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, self.w, self.h);
        ctx.stroke();

        // Paint label via backbuffered child.
        self.label_widget.set_color(v.text_color);
        let lb = self.label_widget.bounds();
        ctx.save(); ctx.translate(lb.x, lb.y);
        paint_subtree(&mut self.label_widget, ctx);
        ctx.restore();
    }

    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}

/// Build the Strip demo — a horizontal row of fixed-width strips, then a
/// vertical column of fixed-height strips.
pub fn strip_demo(font: Arc<Font>) -> Box<dyn Widget> {
    let mut outer = FlexColumn::new()
        .with_gap(16.0)
        .with_padding(14.0)
        .with_panel_bg();

    outer.push(Box::new(Label::new("Horizontal strips", Arc::clone(&font))
        .with_font_size(12.0)), 0.0);

    let colors_h = [
        Color::rgba(0.22, 0.45, 0.88, 0.18),
        Color::rgba(0.18, 0.72, 0.42, 0.18),
        Color::rgba(0.88, 0.25, 0.18, 0.18),
        Color::rgba(0.86, 0.78, 0.40, 0.18),
        Color::rgba(0.60, 0.25, 0.88, 0.18),
    ];
    let mut h_row = FlexRow::new().with_gap(4.0);
    for (i, &bg) in colors_h.iter().enumerate() {
        h_row.push(Box::new(StripCell::new(
            format!("S{}", i + 1), Arc::clone(&font), bg, 55.0, 40.0,
        )), 0.0);
    }
    outer.push(Box::new(h_row), 0.0);

    outer.push(Box::new(Separator::horizontal()), 0.0);
    outer.push(Box::new(Label::new("Vertical strips", Arc::clone(&font))
        .with_font_size(12.0)), 0.0);

    let colors_v = [
        Color::rgba(0.22, 0.65, 0.88, 0.18),
        Color::rgba(0.88, 0.55, 0.15, 0.18),
        Color::rgba(0.88, 0.25, 0.65, 0.18),
        Color::rgba(0.50, 0.50, 0.50, 0.18),
    ];
    let mut v_col = FlexColumn::new().with_gap(4.0);
    for (i, &bg) in colors_v.iter().enumerate() {
        v_col.push(Box::new(StripCell::new(
            format!("Strip {}", i + 1), Arc::clone(&font), bg, 200.0, 32.0,
        )), 0.0);
    }
    outer.push(Box::new(v_col), 0.0);

    outer.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);
    Box::new(outer)
}

// ---------------------------------------------------------------------------
// Table demo
// ---------------------------------------------------------------------------

/// Build the Table demo — a header row and 8 data rows with alternating colors.
pub fn table_demo(font: Arc<Font>) -> Box<dyn Widget> {
    let mut outer = FlexColumn::new()
        .with_gap(10.0)
        .with_padding(12.0)
        .with_panel_bg();

    outer.push(Box::new(Label::new("Simple data table", Arc::clone(&font))
        .with_font_size(12.0)), 0.0);

    // Column widths.
    let col_w = [55.0_f64, 90.0, 70.0, 55.0];
    let headers = ["#", "Name", "Value", "Status"];

    // Header row.
    let mut header_row = FlexRow::new().with_gap(0.0);
    for (i, &hdr) in headers.iter().enumerate() {
        let cell = Container::new()
            .with_background(Color::rgba(0.0, 0.0, 0.0, 0.10))
            .with_border(Color::rgba(0.0, 0.0, 0.0, 0.15), 1.0)
            .with_padding(5.0)
            .add(Box::new(SizedBox::new().with_width(col_w[i]).with_child(
                Box::new(Label::new(hdr, Arc::clone(&font)).with_font_size(11.5))
            )));
        header_row.push(Box::new(cell), 0.0);
    }
    outer.push(Box::new(header_row), 0.0);

    // Data rows.
    let data = [
        ("1", "Alpha",   "0.92",  "OK"),
        ("2", "Beta",    "1.44",  "OK"),
        ("3", "Gamma",   "0.07",  "Warn"),
        ("4", "Delta",   "3.14",  "OK"),
        ("5", "Epsilon", "2.72",  "OK"),
        ("6", "Zeta",    "0.00",  "Error"),
        ("7", "Eta",     "9.81",  "OK"),
        ("8", "Theta",   "1.618", "OK"),
    ];
    for (row_i, &(n, name, val, status)) in data.iter().enumerate() {
        let bg = if row_i % 2 == 0 {
            Color::rgba(0.0, 0.0, 0.0, 0.03)
        } else {
            Color::rgba(0.0, 0.0, 0.0, 0.0)
        };
        let cells_text = [n, name, val, status];
        let mut data_row = FlexRow::new().with_gap(0.0);
        for (ci, &text) in cells_text.iter().enumerate() {
            let cell = Container::new()
                .with_background(bg)
                .with_border(Color::rgba(0.0, 0.0, 0.0, 0.08), 1.0)
                .with_padding(5.0)
                .add(Box::new(SizedBox::new().with_width(col_w[ci]).with_child(
                    Box::new(Label::new(text, Arc::clone(&font)).with_font_size(12.0))
                )));
            data_row.push(Box::new(cell), 0.0);
        }
        outer.push(Box::new(data_row), 0.0);
    }

    outer.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);
    Box::new(ScrollView::new(Box::new(outer)))
}

// ---------------------------------------------------------------------------
// Text Layout demo
// ---------------------------------------------------------------------------

/// Build the Text Layout demo — a variety of font sizes, colors, and a
/// paragraph that illustrates line wrapping.
pub fn text_layout(font: Arc<Font>) -> Box<dyn Widget> {
    let mut col = FlexColumn::new()
        .with_gap(10.0)
        .with_padding(14.0)
        .with_panel_bg();

    col.push(Box::new(Label::new("Heading 1 — size 24", Arc::clone(&font))
        .with_font_size(24.0)), 0.0);
    col.push(Box::new(Label::new("Heading 2 — size 18", Arc::clone(&font))
        .with_font_size(18.0)), 0.0);
    col.push(Box::new(Label::new("Heading 3 — size 14", Arc::clone(&font))
        .with_font_size(14.0)), 0.0);
    col.push(Box::new(Label::new("Body text — size 12", Arc::clone(&font))
        .with_font_size(12.0)), 0.0);
    col.push(Box::new(Label::new("Caption — size 11", Arc::clone(&font))
        .with_font_size(11.0)), 0.0);

    col.push(Box::new(Separator::horizontal()), 0.0);

    col.push(Box::new(Label::new("Accent colored text", Arc::clone(&font))
        .with_font_size(13.0).with_color(Color::rgb(0.22, 0.45, 0.88))), 0.0);
    col.push(Box::new(Label::new("Danger / warning text", Arc::clone(&font))
        .with_font_size(13.0).with_color(Color::rgb(0.88, 0.25, 0.18))), 0.0);
    col.push(Box::new(Label::new("Dimmed secondary text", Arc::clone(&font))
        .with_font_size(13.0)), 0.0);

    col.push(Box::new(Separator::horizontal()), 0.0);

    col.push(Box::new(Label::new("Paragraph with line wrapping:", Arc::clone(&font))
        .with_font_size(11.5)), 0.0);
    col.push(Box::new(Label::new(
        "The quick brown fox jumps over the lazy dog. Pack my box with five dozen \
         liquor jugs. How vain it is to sit down to write when you have not stood up \
         to live. The art of writing is the art of discovering what you believe.",
        Arc::clone(&font),
    ).with_font_size(12.5)), 0.0);

    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);
    Box::new(ScrollView::new(Box::new(col)))
}

// ---------------------------------------------------------------------------
// Undo Redo demo
// ---------------------------------------------------------------------------

/// Build the Undo Redo demo — a TextField plus usage instructions.
/// (TextField manages its own internal undo history via Ctrl+Z / Ctrl+Y.)
pub fn undo_redo(font: Arc<Font>) -> Box<dyn Widget> {
    let mut col = FlexColumn::new()
        .with_gap(14.0)
        .with_padding(16.0)
        .with_panel_bg();

    col.push(Box::new(Label::new("Text field with undo/redo", Arc::clone(&font))
        .with_font_size(12.0)), 0.0);

    col.push(Box::new(SizedBox::new().with_height(34.0).with_child(Box::new(
        TextField::new(Arc::clone(&font))
            .with_font_size(13.0)
            .with_text("Edit me — then Ctrl+Z to undo")
    ))), 0.0);

    col.push(Box::new(Separator::horizontal()), 0.0);

    col.push(Box::new(Label::new("Keyboard shortcuts:", Arc::clone(&font))
        .with_font_size(12.0)), 0.0);

    for line in [
        "Ctrl+Z         — undo last edit",
        "Ctrl+Y         — redo",
        "Ctrl+Shift+Z   — redo (alternate)",
        "Ctrl+A         — select all",
        "Ctrl+C / X / V — clipboard",
    ] {
        col.push(Box::new(Label::new(line, Arc::clone(&font))
            .with_font_size(12.0)), 0.0);
    }

    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(Box::new(Label::new(
        "Each character insertion/deletion is recorded in the TextField's internal \
         UndoBuffer. Undo collapses runs of single-character edits into a single step.",
        Arc::clone(&font),
    ).with_font_size(11.0)), 0.0);

    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);
    Box::new(col)
}

// ---------------------------------------------------------------------------
// Window Options demo
// ---------------------------------------------------------------------------

/// Build the Window Options demo — checkboxes reflecting window capabilities.
pub fn window_options(font: Arc<Font>) -> Box<dyn Widget> {
    let resizable   = Rc::new(Cell::new(true));
    let collapsible = Rc::new(Cell::new(true));
    let auto_sized  = Rc::new(Cell::new(false));
    let anchored    = Rc::new(Cell::new(false));

    let mut col = FlexColumn::new()
        .with_gap(14.0)
        .with_padding(16.0)
        .with_panel_bg();

    col.push(Box::new(Label::new("Window options", Arc::clone(&font))
        .with_font_size(12.0)), 0.0);

    {
        let v = Rc::clone(&resizable);
        col.push(Box::new(Checkbox::new("Resizable", Arc::clone(&font), resizable.get())
            .with_font_size(13.0).on_change(move |b| v.set(b))), 0.0);
    }
    {
        let v = Rc::clone(&collapsible);
        col.push(Box::new(Checkbox::new("Collapsible", Arc::clone(&font), collapsible.get())
            .with_font_size(13.0).on_change(move |b| v.set(b))), 0.0);
    }
    {
        let v = Rc::clone(&auto_sized);
        col.push(Box::new(Checkbox::new("Auto-sized", Arc::clone(&font), auto_sized.get())
            .with_font_size(13.0).on_change(move |b| v.set(b))), 0.0);
    }
    {
        let v = Rc::clone(&anchored);
        col.push(Box::new(Checkbox::new("Anchored", Arc::clone(&font), anchored.get())
            .with_font_size(13.0).on_change(move |b| v.set(b))), 0.0);
    }

    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(Box::new(Label::new(
        "Current window size: 360 \u{00d7} 290",
        Arc::clone(&font),
    ).with_font_size(12.0)), 0.0);

    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);
    Box::new(col)
}

// ---------------------------------------------------------------------------
// Modals demo
// ---------------------------------------------------------------------------

/// Inline modal overlay: shown/hidden by the `open` cell.
///
/// Text is rendered through backbuffered Label children so glyph rasterization
/// is cached rather than repeated each frame.
struct ModalOverlay {
    bounds:      Rect,
    children:    Vec<Box<dyn Widget>>,
    open:        Rc<Cell<bool>>,
    lbl_title:   Label,
    lbl_body:    Label,
    lbl_dismiss: Label,
}

impl ModalOverlay {
    fn new(font: Arc<Font>, open: Rc<Cell<bool>>) -> Self {
        Self {
            bounds:      Rect::default(),
            children:    Vec::new(),
            open,
            lbl_title:   Label::new("Modal dialog", Arc::clone(&font)).with_font_size(13.0),
            lbl_body:    Label::new("This is a modal. Click anywhere to dismiss.", Arc::clone(&font)).with_font_size(11.5),
            lbl_dismiss: Label::new("[ Dismiss ]", Arc::clone(&font)).with_font_size(11.0),
        }
    }
}

impl Widget for ModalOverlay {
    fn type_name(&self) -> &'static str { "ModalOverlay" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size {
        if !self.open.get() {
            self.bounds = Rect::new(0.0, 0.0, 0.0, 0.0);
            return Size::new(0.0, 0.0);
        }
        let h = 120.0_f64;
        let w = available.width;
        self.bounds = Rect::new(0.0, 0.0, w, h);

        // Dialog dimensions (computed same as paint).
        let dw = w.min(280.0);
        let dh = 90.0_f64;
        let dx = (w - dw) * 0.5;
        let dy = (h - dh) * 0.5;
        let inner_w = dw - 20.0;

        let ts = self.lbl_title.layout(Size::new(inner_w, 20.0));
        self.lbl_title.set_bounds(Rect::new(dx + 10.0, dy + dh - ts.height - 10.0, ts.width, ts.height));

        let bs = self.lbl_body.layout(Size::new(inner_w, 18.0));
        self.lbl_body.set_bounds(Rect::new(dx + 10.0, dy + dh - ts.height - bs.height - 18.0, bs.width, bs.height));

        let ds = self.lbl_dismiss.layout(Size::new(inner_w, 18.0));
        self.lbl_dismiss.set_bounds(Rect::new(dx + 10.0, dy + dh - ts.height - bs.height - ds.height - 26.0, ds.width, ds.height));

        Size::new(w, h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        if !self.open.get() { return; }
        let v = ctx.visuals();
        let w = self.bounds.width;
        let h = self.bounds.height;

        // Semi-transparent overlay.
        ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, 0.35));
        ctx.begin_path();
        ctx.rect(0.0, 0.0, w, h);
        ctx.fill();

        // Dialog box.
        let dw = w.min(280.0);
        let dh = 90.0_f64;
        let dx = (w - dw) * 0.5;
        let dy = (h - dh) * 0.5;
        ctx.set_fill_color(v.window_fill);
        ctx.begin_path();
        ctx.rounded_rect(dx, dy, dw, dh, 8.0);
        ctx.fill();
        ctx.set_stroke_color(v.widget_stroke);
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.rounded_rect(dx, dy, dw, dh, 8.0);
        ctx.stroke();

        // Paint labels via backbuffered children.
        self.lbl_title.set_color(v.text_color);
        let tb = self.lbl_title.bounds();
        ctx.save(); ctx.translate(tb.x, tb.y);
        paint_subtree(&mut self.lbl_title, ctx);
        ctx.restore();

        self.lbl_body.set_color(v.text_dim);
        let bb = self.lbl_body.bounds();
        ctx.save(); ctx.translate(bb.x, bb.y);
        paint_subtree(&mut self.lbl_body, ctx);
        ctx.restore();

        self.lbl_dismiss.set_color(v.accent);
        let db = self.lbl_dismiss.bounds();
        ctx.save(); ctx.translate(db.x, db.y);
        paint_subtree(&mut self.lbl_dismiss, ctx);
        ctx.restore();
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        if !self.open.get() { return EventResult::Ignored; }
        // Click anywhere dismisses.
        if let Event::MouseDown { button: MouseButton::Left, .. } = event {
            self.open.set(false);
            return EventResult::Consumed;
        }
        EventResult::Ignored
    }

    fn hit_test(&self, p: Point) -> bool {
        self.open.get()
            && p.x >= 0.0 && p.x <= self.bounds.width
            && p.y >= 0.0 && p.y <= self.bounds.height
    }
}

/// Build the Modals demo — a button that shows an inline modal overlay.
pub fn modals_demo(font: Arc<Font>) -> Box<dyn Widget> {
    let open = Rc::new(Cell::new(false));

    let mut col = FlexColumn::new()
        .with_gap(12.0)
        .with_padding(14.0)
        .with_panel_bg();

    col.push(Box::new(Label::new("Modals demo", Arc::clone(&font))
        .with_font_size(12.0)), 0.0);

    {
        let open_for_btn = Rc::clone(&open);
        col.push(Box::new(SizedBox::new().with_height(30.0).with_child(Box::new(
            Button::new("Open modal", Arc::clone(&font))
                .with_font_size(13.0)
                .on_click(move || { open_for_btn.set(true); })
        ))), 0.0);
    }

    col.push(Box::new(ModalOverlay::new(Arc::clone(&font), Rc::clone(&open))), 0.0);

    col.push(Box::new(Label::new(
        "Click 'Open modal' to show the dialog. Click anywhere in it to dismiss.",
        Arc::clone(&font),
    ).with_font_size(11.0)), 0.0);

    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);
    Box::new(col)
}

// ---------------------------------------------------------------------------
// Multi Touch demo
// ---------------------------------------------------------------------------

/// Build the Multi Touch demo — an informational placeholder.
pub fn multi_touch(font: Arc<Font>) -> Box<dyn Widget> {
    let mut col = FlexColumn::new()
        .with_gap(12.0)
        .with_padding(16.0)
        .with_panel_bg();

    col.push(Box::new(Label::new(
        "Multi-touch events are not available on desktop platforms.",
        Arc::clone(&font),
    ).with_font_size(13.0)), 0.0);

    col.push(Box::new(Label::new(
        "On WASM targets, browser touch events are mapped to agg-gui pointer\n\
         events.  A future update will expose multi-touch gesture data (pinch,\n\
         rotation) via the `Event::Touch` variant.\n\n\
         To test on WASM: open the demo in a browser on a touch-capable device,\n\
         or use browser DevTools touch emulation.",
        Arc::clone(&font),
    ).with_font_size(12.0)), 0.0);

    Box::new(col)
}
