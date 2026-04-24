//! Text-related and layout demo windows: scrolling rows, strip layout, table,
//! text layout showcase, undo/redo, window options, modals, and multi-touch info.
//!
//! Most demos here are purely compositional — they build a widget tree from
//! `FlexColumn`, `FlexRow`, `Container`, `Label`, etc. without custom painting.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::widget::paint_subtree;
use agg_gui::{
    Button, Checkbox, Color, Container, DrawCtx, Event, EventResult, FlexColumn, FlexRow, Font,
    Label, MouseButton, Point, Rect, ScrollView, Separator, Size, SizedBox, TextField, Widget,
};

// ---------------------------------------------------------------------------
// Strip demo
// ---------------------------------------------------------------------------

/// A fixed-width labeled box used to visualise "strip" regions.
///
/// Text is rendered through a backbuffered Label child so the glyph rasterization
/// is cached to a framebuffer rather than repeated each frame.
struct StripCell {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    label_widget: Label,
    bg: Color,
    w: f64,
    h: f64,
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
    fn type_name(&self) -> &'static str {
        "StripCell"
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
        self.bounds = Rect::new(0.0, 0.0, self.w, self.h);
        // Position the label at 4px from the left, vertically centered.
        let ls = self.label_widget.layout(Size::new(self.w - 8.0, self.h));
        let ly = (self.h - ls.height) * 0.5;
        self.label_widget
            .set_bounds(Rect::new(4.0, ly, ls.width, ls.height));
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
        ctx.save();
        ctx.translate(lb.x, lb.y);
        paint_subtree(&mut self.label_widget, ctx);
        ctx.restore();
    }

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// Build the Strip demo — a horizontal row of fixed-width strips, then a
/// vertical column of fixed-height strips.
pub fn strip_demo(font: Arc<Font>) -> Box<dyn Widget> {
    let mut outer = FlexColumn::new()
        .with_gap(16.0)
        .with_padding(14.0)
        .with_panel_bg();

    outer.push(
        Box::new(Label::new("Horizontal strips", Arc::clone(&font)).with_font_size(12.0)),
        0.0,
    );

    let colors_h = [
        Color::rgba(0.22, 0.45, 0.88, 0.18),
        Color::rgba(0.18, 0.72, 0.42, 0.18),
        Color::rgba(0.88, 0.25, 0.18, 0.18),
        Color::rgba(0.86, 0.78, 0.40, 0.18),
        Color::rgba(0.60, 0.25, 0.88, 0.18),
    ];
    let mut h_row = FlexRow::new().with_gap(4.0);
    for (i, &bg) in colors_h.iter().enumerate() {
        h_row.push(
            Box::new(StripCell::new(
                format!("S{}", i + 1),
                Arc::clone(&font),
                bg,
                55.0,
                40.0,
            )),
            0.0,
        );
    }
    outer.push(Box::new(h_row), 0.0);

    outer.push(Box::new(Separator::horizontal()), 0.0);
    outer.push(
        Box::new(Label::new("Vertical strips", Arc::clone(&font)).with_font_size(12.0)),
        0.0,
    );

    let colors_v = [
        Color::rgba(0.22, 0.65, 0.88, 0.18),
        Color::rgba(0.88, 0.55, 0.15, 0.18),
        Color::rgba(0.88, 0.25, 0.65, 0.18),
        Color::rgba(0.50, 0.50, 0.50, 0.18),
    ];
    let mut v_col = FlexColumn::new().with_gap(4.0);
    for (i, &bg) in colors_v.iter().enumerate() {
        v_col.push(
            Box::new(StripCell::new(
                format!("Strip {}", i + 1),
                Arc::clone(&font),
                bg,
                200.0,
                32.0,
            )),
            0.0,
        );
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

    outer.push(
        Box::new(Label::new("Simple data table", Arc::clone(&font)).with_font_size(12.0)),
        0.0,
    );

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
                Box::new(Label::new(hdr, Arc::clone(&font)).with_font_size(11.5)),
            )));
        header_row.push(Box::new(cell), 0.0);
    }
    outer.push(Box::new(header_row), 0.0);

    // Data rows.
    let data = [
        ("1", "Alpha", "0.92", "OK"),
        ("2", "Beta", "1.44", "OK"),
        ("3", "Gamma", "0.07", "Warn"),
        ("4", "Delta", "3.14", "OK"),
        ("5", "Epsilon", "2.72", "OK"),
        ("6", "Zeta", "0.00", "Error"),
        ("7", "Eta", "9.81", "OK"),
        ("8", "Theta", "1.618", "OK"),
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
                    Box::new(Label::new(text, Arc::clone(&font)).with_font_size(12.0)),
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

    col.push(
        Box::new(Label::new("Heading 1 — size 24", Arc::clone(&font)).with_font_size(24.0)),
        0.0,
    );
    col.push(
        Box::new(Label::new("Heading 2 — size 18", Arc::clone(&font)).with_font_size(18.0)),
        0.0,
    );
    col.push(
        Box::new(Label::new("Heading 3 — size 14", Arc::clone(&font)).with_font_size(14.0)),
        0.0,
    );
    col.push(
        Box::new(Label::new("Body text — size 12", Arc::clone(&font)).with_font_size(12.0)),
        0.0,
    );
    col.push(
        Box::new(Label::new("Caption — size 11", Arc::clone(&font)).with_font_size(11.0)),
        0.0,
    );

    col.push(Box::new(Separator::horizontal()), 0.0);

    col.push(
        Box::new(
            Label::new("Accent colored text", Arc::clone(&font))
                .with_font_size(13.0)
                .with_color(Color::rgb(0.22, 0.45, 0.88)),
        ),
        0.0,
    );
    col.push(
        Box::new(
            Label::new("Danger / warning text", Arc::clone(&font))
                .with_font_size(13.0)
                .with_color(Color::rgb(0.88, 0.25, 0.18)),
        ),
        0.0,
    );
    col.push(
        Box::new(Label::new("Dimmed secondary text", Arc::clone(&font)).with_font_size(13.0)),
        0.0,
    );

    col.push(Box::new(Separator::horizontal()), 0.0);

    col.push(
        Box::new(
            Label::new("Paragraph with line wrapping:", Arc::clone(&font)).with_font_size(11.5),
        ),
        0.0,
    );
    col.push(
        Box::new(
            Label::new(
                "The quick brown fox jumps over the lazy dog. Pack my box with five dozen \
         liquor jugs. How vain it is to sit down to write when you have not stood up \
         to live. The art of writing is the art of discovering what you believe.",
                Arc::clone(&font),
            )
            .with_font_size(12.5),
        ),
        0.0,
    );

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

    col.push(
        Box::new(Label::new("Text field with undo/redo", Arc::clone(&font)).with_font_size(12.0)),
        0.0,
    );

    col.push(
        Box::new(
            SizedBox::new().with_height(34.0).with_child(Box::new(
                TextField::new(Arc::clone(&font))
                    .with_font_size(13.0)
                    .with_text("Edit me — then Ctrl+Z to undo"),
            )),
        ),
        0.0,
    );

    col.push(Box::new(Separator::horizontal()), 0.0);

    col.push(
        Box::new(Label::new("Keyboard shortcuts:", Arc::clone(&font)).with_font_size(12.0)),
        0.0,
    );

    for line in [
        "Ctrl+Z         — undo last edit",
        "Ctrl+Y         — redo",
        "Ctrl+Shift+Z   — redo (alternate)",
        "Ctrl+A         — select all",
        "Ctrl+C / X / V — clipboard",
    ] {
        col.push(
            Box::new(Label::new(line, Arc::clone(&font)).with_font_size(12.0)),
            0.0,
        );
    }

    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(
        Box::new(
            Label::new(
                "Each character insertion/deletion is recorded in the TextField's internal \
         UndoBuffer. Undo collapses runs of single-character edits into a single step.",
                Arc::clone(&font),
            )
            .with_font_size(11.0),
        ),
        0.0,
    );

    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);
    Box::new(col)
}

// ---------------------------------------------------------------------------
// Window Options demo
// ---------------------------------------------------------------------------

/// Build the Window Options demo — checkboxes reflecting window capabilities.
pub fn window_options(font: Arc<Font>) -> Box<dyn Widget> {
    let resizable = Rc::new(Cell::new(true));
    let collapsible = Rc::new(Cell::new(true));
    let auto_sized = Rc::new(Cell::new(false));
    let anchored = Rc::new(Cell::new(false));

    let mut col = FlexColumn::new()
        .with_gap(14.0)
        .with_padding(16.0)
        .with_panel_bg();

    col.push(
        Box::new(Label::new("Window options", Arc::clone(&font)).with_font_size(12.0)),
        0.0,
    );

    {
        let v = Rc::clone(&resizable);
        col.push(
            Box::new(
                Checkbox::new("Resizable", Arc::clone(&font), resizable.get())
                    .with_font_size(13.0)
                    .on_change(move |b| v.set(b)),
            ),
            0.0,
        );
    }
    {
        let v = Rc::clone(&collapsible);
        col.push(
            Box::new(
                Checkbox::new("Collapsible", Arc::clone(&font), collapsible.get())
                    .with_font_size(13.0)
                    .on_change(move |b| v.set(b)),
            ),
            0.0,
        );
    }
    {
        let v = Rc::clone(&auto_sized);
        col.push(
            Box::new(
                Checkbox::new("Auto-sized", Arc::clone(&font), auto_sized.get())
                    .with_font_size(13.0)
                    .on_change(move |b| v.set(b)),
            ),
            0.0,
        );
    }
    {
        let v = Rc::clone(&anchored);
        col.push(
            Box::new(
                Checkbox::new("Anchored", Arc::clone(&font), anchored.get())
                    .with_font_size(13.0)
                    .on_change(move |b| v.set(b)),
            ),
            0.0,
        );
    }

    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(
        Box::new(
            Label::new("Current window size: 360 \u{00d7} 290", Arc::clone(&font))
                .with_font_size(12.0),
        ),
        0.0,
    );

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
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    open: Rc<Cell<bool>>,
    lbl_title: Label,
    lbl_body: Label,
    lbl_dismiss: Label,
}

impl ModalOverlay {
    fn new(font: Arc<Font>, open: Rc<Cell<bool>>) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            open,
            lbl_title: Label::new("Modal dialog", Arc::clone(&font)).with_font_size(13.0),
            lbl_body: Label::new(
                "This is a modal. Click anywhere to dismiss.",
                Arc::clone(&font),
            )
            .with_font_size(11.5),
            lbl_dismiss: Label::new("[ Dismiss ]", Arc::clone(&font)).with_font_size(11.0),
        }
    }
}

impl Widget for ModalOverlay {
    fn type_name(&self) -> &'static str {
        "ModalOverlay"
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
        self.lbl_title.set_bounds(Rect::new(
            dx + 10.0,
            dy + dh - ts.height - 10.0,
            ts.width,
            ts.height,
        ));

        let bs = self.lbl_body.layout(Size::new(inner_w, 18.0));
        self.lbl_body.set_bounds(Rect::new(
            dx + 10.0,
            dy + dh - ts.height - bs.height - 18.0,
            bs.width,
            bs.height,
        ));

        let ds = self.lbl_dismiss.layout(Size::new(inner_w, 18.0));
        self.lbl_dismiss.set_bounds(Rect::new(
            dx + 10.0,
            dy + dh - ts.height - bs.height - ds.height - 26.0,
            ds.width,
            ds.height,
        ));

        Size::new(w, h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        if !self.open.get() {
            return;
        }
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
        ctx.save();
        ctx.translate(tb.x, tb.y);
        paint_subtree(&mut self.lbl_title, ctx);
        ctx.restore();

        self.lbl_body.set_color(v.text_dim);
        let bb = self.lbl_body.bounds();
        ctx.save();
        ctx.translate(bb.x, bb.y);
        paint_subtree(&mut self.lbl_body, ctx);
        ctx.restore();

        self.lbl_dismiss.set_color(v.accent);
        let db = self.lbl_dismiss.bounds();
        ctx.save();
        ctx.translate(db.x, db.y);
        paint_subtree(&mut self.lbl_dismiss, ctx);
        ctx.restore();
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        if !self.open.get() {
            return EventResult::Ignored;
        }
        // Click anywhere dismisses.
        if let Event::MouseDown {
            button: MouseButton::Left,
            ..
        } = event
        {
            self.open.set(false);
            return EventResult::Consumed;
        }
        EventResult::Ignored
    }

    fn hit_test(&self, p: Point) -> bool {
        self.open.get()
            && p.x >= 0.0
            && p.x <= self.bounds.width
            && p.y >= 0.0
            && p.y <= self.bounds.height
    }
}

/// Build the Modals demo — a button that shows an inline modal overlay.
pub fn modals_demo(font: Arc<Font>) -> Box<dyn Widget> {
    let open = Rc::new(Cell::new(false));

    let mut col = FlexColumn::new()
        .with_gap(12.0)
        .with_padding(14.0)
        .with_panel_bg();

    col.push(
        Box::new(Label::new("Modals demo", Arc::clone(&font)).with_font_size(12.0)),
        0.0,
    );

    {
        let open_for_btn = Rc::clone(&open);
        col.push(
            Box::new(
                SizedBox::new().with_height(30.0).with_child(Box::new(
                    Button::new("Open modal", Arc::clone(&font))
                        .with_font_size(13.0)
                        .on_click(move || {
                            open_for_btn.set(true);
                        }),
                )),
            ),
            0.0,
        );
    }

    col.push(
        Box::new(ModalOverlay::new(Arc::clone(&font), Rc::clone(&open))),
        0.0,
    );

    col.push(
        Box::new(
            Label::new(
                "Click 'Open modal' to show the dialog. Click anywhere in it to dismiss.",
                Arc::clone(&font),
            )
            .with_font_size(11.0),
        ),
        0.0,
    );

    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);
    Box::new(col)
}

// ---------------------------------------------------------------------------
// Multi Touch demo
// ---------------------------------------------------------------------------
//
// Port of egui's `multi_touch.rs` demo.  Layout + interaction + the
// decaying-arrow trick all match the original as closely as the
// coordinate-system flip allows.  The big visible difference vs. egui
// is Y-up: egui draws the arrow from (-0.5, 0.5) to (0.5, -0.5) in its
// Y-down normalised space, which reads visually as bottom-left →
// top-right; in our Y-up space that same visual is (-0.5, -0.5) to
// (0.5, 0.5).  Everything else — normalised ±1 canvas with square
// proportions, zoom/rotate/translate accumulators, pressure-driven
// stroke width, and the half-life reset animation — is the same.

/// Accumulated zoom / rotation / translation state for the arrow.
/// Mirrors the fields on egui's `MultiTouch` struct.
struct MultiTouchView {
    bounds: agg_gui::Rect,
    children: Vec<Box<dyn Widget>>,
    /// Multiplicative zoom; starts at 1.0 and pinch deltas multiply in.
    zoom: f64,
    /// Rotation in radians (Y-up CCW).
    rotation: f64,
    /// Translation in NORMALISED units (i.e. `pixels / scale`), so the
    /// arrow tracks the pinch midpoint regardless of widget size — this
    /// is what egui does via `to_screen.inverse().scale() * delta`.
    translation_x: f64,
    translation_y: f64,
    /// Timestamp of the most recent frame that saw a touch gesture.
    /// The reset animation keys off `(now - last_touch_time)`.
    last_touch_time: Option<web_time::Instant>,
    /// Previous frame's instant — used to derive `dt` for the half-life
    /// decay.  `None` until after the first paint.
    prev_frame_time: Option<web_time::Instant>,
    /// Latest frame's force reading (0.0 when unsupported), used to
    /// thicken the stroke.
    force: f32,
    /// Latest frame's finger count.  Surfaced through the status label.
    num_touches: usize,
}

impl MultiTouchView {
    fn new() -> Self {
        Self {
            bounds: agg_gui::Rect::default(),
            children: Vec::new(),
            zoom: 1.0,
            rotation: 0.0,
            translation_x: 0.0,
            translation_y: 0.0,
            last_touch_time: None,
            prev_frame_time: None,
            force: 0.0,
            num_touches: 0,
        }
    }

    /// Uniform pixels-per-normalised-unit scale, matching egui's
    /// `to_screen.scale()`.  The shorter widget axis maps to ±1.
    fn unit_scale(&self) -> f64 {
        self.bounds.width.min(self.bounds.height) * 0.5
    }

    /// Smoothly drift zoom / rotation / translation back toward identity
    /// once the user lifts their fingers.  Same curve as egui: hold for
    /// 0.5 s, then an exponential half-life decay whose time-constant
    /// itself ramps down over the next 0.5 s.
    fn slowly_reset(&mut self, now: web_time::Instant, dt: f64) -> bool {
        let last = match self.last_touch_time {
            Some(t) => t,
            None => return false,
        };
        let time_since_last = now.duration_since(last).as_secs_f64();
        let delay = 0.5_f64;
        if time_since_last < delay {
            return true; // keep ticking, don't change values yet
        }
        // `remap_clamp(time_since_last, 0.5..=1.0, 1.0..=0.0)` from egui.
        let t = ((time_since_last - delay) / (1.0 - delay)).clamp(0.0, 1.0);
        let half_life = (1.0 - t).powi(4);
        if half_life <= 1e-3 {
            self.zoom = 1.0;
            self.rotation = 0.0;
            self.translation_x = 0.0;
            self.translation_y = 0.0;
            return false;
        }
        // dt is the wall-clock delta between frames.
        let factor = (-(2_f64.ln()) / half_life * dt).exp();
        self.zoom = 1.0 + (self.zoom - 1.0) * factor;
        self.rotation *= factor;
        self.translation_x *= factor;
        self.translation_y *= factor;
        true
    }
}

impl Widget for MultiTouchView {
    fn type_name(&self) -> &'static str {
        "MultiTouchView"
    }
    fn bounds(&self) -> agg_gui::Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: agg_gui::Rect) {
        self.bounds = b;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }

    fn layout(&mut self, available: agg_gui::Size) -> agg_gui::Size {
        self.bounds = agg_gui::Rect::new(0.0, 0.0, available.width, available.height);
        available
    }

    fn paint(&mut self, ctx: &mut dyn agg_gui::DrawCtx) {
        let now = web_time::Instant::now();
        let dt = match self.prev_frame_time {
            Some(t) => now.duration_since(t).as_secs_f64().clamp(0.0, 0.25),
            None => 1.0 / 60.0,
        };
        self.prev_frame_time = Some(now);

        // ── Integrate this frame's gesture deltas ────────────────────────
        let scale = self.unit_scale();
        let mut stroke_width = 1.0_f32;
        let had_gesture = if let Some(mt) = agg_gui::current_multi_touch() {
            self.zoom *= mt.zoom_delta as f64;
            self.rotation += mt.rotation_delta as f64;
            // Pan delta comes in widget pixels; store in normalised units
            // so the accumulator is resolution-independent.
            if scale > 0.0 {
                self.translation_x += mt.translation_delta.x / scale;
                self.translation_y += mt.translation_delta.y / scale;
            }
            self.force = mt.force;
            self.num_touches = mt.num_touches;
            self.last_touch_time = Some(now);
            stroke_width += 10.0 * mt.force;
            true
        } else {
            self.num_touches = 0;
            self.force = 0.0;
            self.slowly_reset(now, dt)
        };
        if had_gesture {
            agg_gui::animation::request_tick();
        }

        // ── Canvas background ────────────────────────────────────────────
        let v = ctx.visuals();
        ctx.set_fill_color(v.panel_fill);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, self.bounds.width, self.bounds.height);
        ctx.fill();

        // ── Arrow geometry ───────────────────────────────────────────────
        //
        // egui draws from (-0.5, 0.5) to (0.5, -0.5) in Y-down, meaning
        // bottom-left → top-right visually.  In Y-up that's
        // (-0.5, -0.5) → (0.5, 0.5).
        let cx = self.bounds.width * 0.5;
        let cy = self.bounds.height * 0.5;
        let zoom = self.zoom;
        let (sin_r, cos_r) = self.rotation.sin_cos();
        let rot_scale = |vx: f64, vy: f64| -> (f64, f64) {
            (
                zoom * (vx * cos_r - vy * sin_r),
                zoom * (vx * sin_r + vy * cos_r),
            )
        };
        let (tail_ox, tail_oy) = rot_scale(-0.5, -0.5);
        let (dir_x, dir_y) = rot_scale(1.0, 1.0);
        let tail_nx = self.translation_x + tail_ox;
        let tail_ny = self.translation_y + tail_oy;
        let tail_px = cx + tail_nx * scale;
        let tail_py = cy + tail_ny * scale;
        let tip_px = tail_px + dir_x * scale;
        let tip_py = tail_py + dir_y * scale;

        // ── Arrow stroke ─────────────────────────────────────────────────
        let color = v.text_color;
        ctx.set_stroke_color(color);
        ctx.set_line_width(stroke_width as f64);
        ctx.begin_path();
        ctx.move_to(tail_px, tail_py);
        ctx.line_to(tip_px, tip_py);
        ctx.stroke();

        // ── Arrow head (filled triangle at the tip) ──────────────────────
        let head_len = (dir_x * scale).hypot(dir_y * scale) * 0.12;
        let tip_len = (tip_px - tail_px).hypot(tip_py - tail_py);
        if tip_len > 1.0 && head_len > 0.5 {
            let ux = (tip_px - tail_px) / tip_len;
            let uy = (tip_py - tail_py) / tip_len;
            let head_half_angle = 0.45_f64;
            let (sa, ca) = head_half_angle.sin_cos();
            let lx = tip_px - head_len * (ux * ca - uy * sa);
            let ly = tip_py - head_len * (uy * ca + ux * sa);
            let rx = tip_px - head_len * (ux * ca + uy * sa);
            let ry = tip_py - head_len * (uy * ca - ux * sa);
            ctx.set_fill_color(color);
            ctx.begin_path();
            ctx.move_to(tip_px, tip_py);
            ctx.line_to(lx, ly);
            ctx.line_to(rx, ry);
            ctx.close_path();
            ctx.fill();
        }
    }

    fn on_event(&mut self, _event: &agg_gui::Event) -> agg_gui::EventResult {
        // Consume drag events so the host window doesn't move when the
        // user single-finger-drags over the canvas.  Matches the
        // `Sense::drag()` workaround egui uses for the same reason.
        match _event {
            agg_gui::Event::MouseDown { .. }
            | agg_gui::Event::MouseMove { .. }
            | agg_gui::Event::MouseUp { .. } => agg_gui::EventResult::Consumed,
            _ => agg_gui::EventResult::Ignored,
        }
    }

    fn needs_paint(&self) -> bool {
        true
    }
}

/// Build the Multi Touch demo window content.  Single-finger acts like
/// a mouse; two or more fingers produce pinch / rotate / pan gestures
/// that drive the rendered arrow.  Pressure (when the platform reports
/// it) thickens the stroke.
pub fn multi_touch(font: Arc<Font>) -> Box<dyn Widget> {
    let status_font = Arc::clone(&font);

    /// Live status label that re-reads `current_multi_touch` every
    /// layout and formats its text.  Matches egui's "Input source" line.
    struct StatusLabel {
        bounds: agg_gui::Rect,
        children: Vec<Box<dyn Widget>>,
        inner: Label,
    }
    impl Widget for StatusLabel {
        fn type_name(&self) -> &'static str {
            "MultiTouchStatus"
        }
        fn bounds(&self) -> agg_gui::Rect {
            self.bounds
        }
        fn set_bounds(&mut self, b: agg_gui::Rect) {
            self.bounds = b;
            self.inner.set_bounds(b);
        }
        fn children(&self) -> &[Box<dyn Widget>] {
            &self.children
        }
        fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
            &mut self.children
        }
        fn layout(&mut self, available: agg_gui::Size) -> agg_gui::Size {
            let txt = match agg_gui::current_multi_touch() {
                Some(mt) => format!(
                    "Input source: {}-finger touch   force: {:.2}",
                    mt.num_touches, mt.force,
                ),
                None => "Input source: none".to_string(),
            };
            self.inner.set_text(&txt);
            self.inner.layout(available)
        }
        fn paint(&mut self, ctx: &mut dyn agg_gui::DrawCtx) {
            self.inner.paint(ctx);
        }
        fn on_event(&mut self, _e: &agg_gui::Event) -> agg_gui::EventResult {
            agg_gui::EventResult::Ignored
        }
        fn needs_paint(&self) -> bool {
            true
        }
    }

    let status_label: Box<dyn Widget> = Box::new(StatusLabel {
        bounds: agg_gui::Rect::default(),
        children: Vec::new(),
        inner: Label::new(" ", Arc::clone(&status_font))
            .with_font_size(12.0)
            .with_wrap(true),
    });

    let heading = Label::new(
        "This demo only works on devices with multitouch support \
         (e.g. mobiles, tablets, and trackpads).",
        Arc::clone(&font),
    )
    .with_font_size(13.0)
    .with_wrap(true);

    let hint = Label::new(
        "Try touch gestures Pinch/Stretch, Rotation, and Pressure with 2+ fingers.",
        Arc::clone(&font),
    )
    .with_font_size(11.0)
    .with_wrap(true);

    let view: Box<dyn Widget> = Box::new(MultiTouchView::new());

    let col = FlexColumn::new()
        .with_gap(8.0)
        .with_padding(10.0)
        .with_panel_bg()
        .add(Box::new(heading))
        .add(Box::new(Separator::horizontal()))
        .add(Box::new(hint))
        .add(status_label)
        .add_flex(view, 1.0);

    Box::new(col)
}
