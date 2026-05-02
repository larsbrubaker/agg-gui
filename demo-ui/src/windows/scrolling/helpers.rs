//! Shared helper widgets for the Scrolling demo:
//!
//! - [`RowList`]    — virtual list that paints only rows inside the viewport.
//! - [`SegRow`]     — segmented-button row bound to an `Rc<Cell<T>>`.
//! - [`OffsetReadout`]        — "offset / max" text readout.
//! - [`MaxScrollWatcher`]     — fires a callback when `max_scroll` changes.
//! - [`CounterTicker`]        — zero-size widget that increments an Rc<Cell<usize>>.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{Color, DrawCtx, Event, EventResult, Font, Label, Rect, Size, Widget};

// ── Constants ───────────────────────────────────────────────────────────────

pub const LOREM_IPSUM_LONG: &str = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. \
     Curabitur et mauris auctor, cursus leo ut, viverra erat. \
     Nulla facilisi. Vivamus tempus ligula a lectus condimentum aliquam. \
     Sed sit amet magna et arcu efficitur porttitor. Suspendisse potenti. \
     Praesent consequat, lacus in sollicitudin tempor, ex purus commodo urna.";

// ── RowList: virtualised row list rendered through Label children ──────────

/// Demo virtualised list — rows that aren't on-screen don't allocate
/// Label widgets.  `layout()` resizes `children` to match the visible
/// range and pushes each row's formatted text into the Label child via
/// the `set_label_text` Widget-trait method, which Label uses to keep
/// its glyph cache valid across frames (no re-rasterisation when the
/// text didn't change).  The framework paints the Label children
/// itself; `paint()` only draws striping and the highlight overlay.
pub struct RowList {
    bounds: Rect,
    /// Pool of Label children, sized to the current visible range.
    /// Index `i` in this vec corresponds to row `first_visible + i`.
    children: Vec<Box<dyn Widget>>,
    font: Arc<Font>,
    font_size: f64,
    row_height: f64,
    padding_x: f64,
    count: Rc<Cell<usize>>,
    highlight: Rc<Cell<Option<usize>>>,
    formatter: Rc<dyn Fn(usize) -> String>,
    striped: bool,
    /// When bound, rows outside this content-space rect are skipped in paint.
    /// The rect uses top-down coordinates (y = 0 at top of content).
    viewport: Option<Rc<Cell<Rect>>>,
    /// First absolute row index currently mapped to `children[0]`;
    /// `last_visible_count` is `children.len()`.
    first_visible: usize,
}

impl RowList {
    pub fn new(
        font: Arc<Font>,
        count: Rc<Cell<usize>>,
        formatter: Rc<dyn Fn(usize) -> String>,
    ) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            font,
            font_size: 12.0,
            row_height: 18.0,
            padding_x: 8.0,
            count,
            highlight: Rc::new(Cell::new(None)),
            formatter,
            striped: true,
            viewport: None,
            first_visible: 0,
        }
    }

    pub fn with_row_height(mut self, h: f64) -> Self {
        self.row_height = h;
        self
    }
    pub fn with_highlight_cell(mut self, c: Rc<Cell<Option<usize>>>) -> Self {
        self.highlight = c;
        self
    }
    pub fn with_viewport_cell(mut self, c: Rc<Cell<Rect>>) -> Self {
        self.viewport = Some(c);
        self
    }
    #[allow(dead_code)]
    pub fn with_striped(mut self, s: bool) -> Self {
        self.striped = s;
        self
    }
}

impl Widget for RowList {
    fn type_name(&self) -> &'static str {
        "RowList"
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
        let n = self.count.get();
        let h = (n as f64) * self.row_height;
        self.bounds = Rect::new(0.0, 0.0, available.width, h);

        // Pick which row range is currently visible — the same range the
        // pre-refactor `paint()` used to pick which rows to draw.
        let (first, last) = match &self.viewport {
            Some(cell) => {
                let vp = cell.get();
                let f = (vp.y / self.row_height).floor().max(0.0) as usize;
                let l = ((vp.y + vp.height) / self.row_height).ceil() as usize + 1;
                (f.min(n), l.min(n))
            }
            None => (0, n),
        };

        // Resize the Label child pool to the visible count.  Reusing the
        // existing children preserves their glyph caches across scrolls;
        // only freshly-added rows allocate.
        let visible = last.saturating_sub(first);
        while self.children.len() < visible {
            self.children.push(Box::new(
                Label::new("", Arc::clone(&self.font)).with_font_size(self.font_size),
            ));
        }
        self.children.truncate(visible);
        self.first_visible = first;

        // Push the formatter's text + bounds into each visible row's
        // Label child.  In Y-up: row `first` is the topmost visible
        // row, with its bottom edge at `h - (first + 1) * row_h`.
        for (slot, row) in (first..last).enumerate() {
            let y_bottom = h - (row as f64 + 1.0) * self.row_height;
            let y_text = y_bottom + (self.row_height - self.font_size) * 0.5;
            let text = (self.formatter)(row);
            if let Some(child) = self.children.get_mut(slot) {
                child.set_label_text(&text);
                let s = child.layout(Size::new(
                    available.width - self.padding_x,
                    self.row_height,
                ));
                child.set_bounds(Rect::new(self.padding_x, y_text, s.width, s.height));
            }
        }

        Size::new(available.width, h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let n = self.count.get();
        if n == 0 {
            return;
        }
        let total_h = (n as f64) * self.row_height;
        let highlight = self.highlight.get();
        let visible_first = self.first_visible;
        let visible_last = visible_first + self.children.len();

        // Stripes + highlight under the row labels (children paint after).
        for i in visible_first..visible_last {
            let y_bottom = total_h - (i as f64 + 1.0) * self.row_height;
            if self.striped && i % 2 == 0 {
                ctx.set_fill_color(Color::rgba(
                    v.text_color.r,
                    v.text_color.g,
                    v.text_color.b,
                    0.05,
                ));
                ctx.begin_path();
                ctx.rect(0.0, y_bottom, self.bounds.width, self.row_height);
                ctx.fill();
            }
            if highlight == Some(i) {
                ctx.set_fill_color(Color::rgba(v.accent.r, v.accent.g, v.accent.b, 0.25));
                ctx.begin_path();
                ctx.rect(0.0, y_bottom, self.bounds.width, self.row_height);
                ctx.fill();
            }
        }

        // Recolour each visible row's Label child to match highlight state.
        for (slot, row) in (visible_first..visible_last).enumerate() {
            let color = if highlight == Some(row) {
                v.accent
            } else {
                v.text_color
            };
            if let Some(child) = self.children.get_mut(slot) {
                child.set_label_color(color);
            }
        }
        // Label children paint themselves via the framework's tree walk.
    }

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

// ── SegRow: segmented-button row ─────────────────────────────────────────────

/// Generic segmented selector for the scrolling demos.  Composed from
/// real `Button` children — each segment uses `with_subtle()` +
/// `with_active_fn()` so the inactive segments paint muted and the
/// selected one flips to the accent surface.  Same pattern as
/// `RunModeRow` / `MsaaRow` in the backend panel; lives here because
/// the scrolling demos pre-date the consolidation.
pub struct SegRow<T: Clone + Copy + PartialEq + 'static> {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    n: usize,
    state: Rc<Cell<T>>,
    /// Optional callback fired when the selection changes.
    on_change: Option<Rc<dyn Fn()>>,
    last: Cell<Option<T>>,
}

impl<T: Clone + Copy + PartialEq + 'static> SegRow<T> {
    pub fn new(font: Arc<Font>, options: Vec<(&'static str, T)>, state: Rc<Cell<T>>) -> Self {
        let n = options.len();
        let start = state.get();
        let children: Vec<Box<dyn Widget>> = options
            .into_iter()
            .map(|(text, val)| {
                let state_active = Rc::clone(&state);
                let state_click = Rc::clone(&state);
                let btn = agg_gui::Button::new(text, Arc::clone(&font))
                    .with_font_size(12.0)
                    .with_subtle()
                    .with_h_anchor(agg_gui::HAnchor::STRETCH)
                    .with_active_fn(move || state_active.get() == val)
                    .on_click(move || {
                        if state_click.get() != val {
                            state_click.set(val);
                            agg_gui::animation::request_draw();
                        }
                    });
                Box::new(btn) as Box<dyn Widget>
            })
            .collect();
        Self {
            bounds: Rect::default(),
            children,
            n,
            state,
            on_change: None,
            last: Cell::new(Some(start)),
        }
    }

    pub fn on_change(mut self, cb: impl Fn() + 'static) -> Self {
        self.on_change = Some(Rc::new(cb));
        self
    }

    const BTN_H: f64 = 24.0;
    const BTN_GAP: f64 = 1.0;
}

impl<T: Clone + Copy + PartialEq + 'static> Widget for SegRow<T> {
    fn type_name(&self) -> &'static str {
        "SegRow"
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
        let n = self.n.max(1);
        let row_h = Self::BTN_H + 6.0;
        self.bounds = Rect::new(0.0, 0.0, available.width, row_h);
        let cell_w = ((available.width - Self::BTN_GAP * (n - 1) as f64) / n as f64).max(20.0);
        let y = (row_h - Self::BTN_H) * 0.5;
        for (i, child) in self.children.iter_mut().enumerate() {
            child.layout(Size::new(cell_w, Self::BTN_H));
            child.set_bounds(Rect::new(
                i as f64 * (cell_w + Self::BTN_GAP),
                y,
                cell_w,
                Self::BTN_H,
            ));
        }

        // Fire on_change when state changed since last layout.
        let cur = self.state.get();
        if self.last.get() != Some(cur) {
            self.last.set(Some(cur));
            if let Some(cb) = &self.on_change {
                cb();
            }
        }
        Size::new(available.width, row_h)
    }

    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {
        // Buttons paint themselves through the framework's tree walk.
    }

    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

// ── OffsetReadout ────────────────────────────────────────────────────────────

pub struct OffsetReadout {
    pub bounds: Rect,
    /// Single Label child holding the live offset / max-scroll text.
    /// Composing through Label keeps the glyph cache warm — `paint()`
    /// only refreshes the text via `set_label_text`, which Label
    /// internally uses to invalidate its cache only when the value
    /// actually changes.
    pub children: Vec<Box<dyn Widget>>,
    pub offset: Rc<Cell<f64>>,
    pub max: Rc<Cell<f64>>,
}

impl OffsetReadout {
    pub fn new(font: Arc<Font>, offset: Rc<Cell<f64>>, max: Rc<Cell<f64>>) -> Self {
        Self {
            bounds: Rect::default(),
            children: vec![Box::new(
                Label::new("", font).with_font_size(11.0),
            )],
            offset,
            max,
        }
    }
}

impl Widget for OffsetReadout {
    fn type_name(&self) -> &'static str {
        "OffsetReadout"
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
    fn layout(&mut self, a: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, a.width, 16.0);
        if let Some(child) = self.children.first_mut() {
            let s = child.layout(Size::new(a.width, 16.0));
            child.set_bounds(Rect::new(2.0, 0.0, s.width, s.height));
        }
        Size::new(a.width, 16.0)
    }
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let text = format!(
            "Scroll offset: {:.0} / {:.0} px",
            self.offset.get(),
            self.max.get()
        );
        if let Some(child) = self.children.first_mut() {
            child.set_label_text(&text);
            child.set_label_color(v.text_dim);
        }
        // Label child paints itself via the framework's tree walk.
    }
    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

// ── MaxScrollWatcher ─────────────────────────────────────────────────────────

pub struct MaxScrollWatcher {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    max: Rc<Cell<f64>>,
    last: Cell<f64>,
    cb: Rc<dyn Fn()>,
}
impl MaxScrollWatcher {
    pub fn new(max: Rc<Cell<f64>>, cb: Rc<dyn Fn()>) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            max,
            last: Cell::new(f64::NAN),
            cb,
        }
    }
}
impl Widget for MaxScrollWatcher {
    fn type_name(&self) -> &'static str {
        "MaxScrollWatcher"
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
    fn show_in_inspector(&self) -> bool {
        false
    }
    fn layout(&mut self, _: Size) -> Size {
        let cur = self.max.get();
        let last = self.last.get();
        if last.is_nan() || (cur - last).abs() > 0.5 {
            self.last.set(cur);
            (self.cb)();
        }
        Size::ZERO
    }
    fn paint(&mut self, _: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

// ── CounterTicker ────────────────────────────────────────────────────────────

pub struct CounterTicker {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    counter: Rc<Cell<usize>>,
}
impl CounterTicker {
    pub fn new(c: Rc<Cell<usize>>) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            counter: c,
        }
    }
}
impl Widget for CounterTicker {
    fn type_name(&self) -> &'static str {
        "CounterTicker"
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
    fn show_in_inspector(&self) -> bool {
        false
    }
    fn layout(&mut self, _: Size) -> Size {
        self.counter.set(self.counter.get() + 1);
        Size::ZERO
    }
    fn paint(&mut self, _: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

// ── Small label builders ─────────────────────────────────────────────────────

pub fn label(font: Arc<Font>, text: impl Into<String>, size: f64) -> Box<dyn Widget> {
    Box::new(Label::new(text.into(), font).with_font_size(size))
}

pub fn wrapped_label(font: Arc<Font>, text: impl Into<String>, size: f64) -> Box<dyn Widget> {
    Box::new(
        Label::new(text.into(), font)
            .with_font_size(size)
            .with_wrap(true),
    )
}

// ── LiveLabel: a label whose text is produced by a closure each layout ──────

pub struct LiveLabel {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    font: Arc<Font>,
    font_size: f64,
    producer: Rc<dyn Fn() -> String>,
    color: Option<Color>,
}
impl LiveLabel {
    pub fn new(font: Arc<Font>, producer: Rc<dyn Fn() -> String>) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            font,
            font_size: 12.0,
            producer,
            color: None,
        }
    }
    pub fn with_font_size(mut self, s: f64) -> Self {
        self.font_size = s;
        self
    }
    #[allow(dead_code)]
    pub fn with_color(mut self, c: Color) -> Self {
        self.color = Some(c);
        self
    }
}
impl Widget for LiveLabel {
    fn type_name(&self) -> &'static str {
        "LiveLabel"
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
    fn layout(&mut self, a: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, a.width.min(80.0), self.font_size + 6.0);
        Size::new(self.bounds.width, self.bounds.height)
    }
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(self.font_size);
        ctx.set_fill_color(self.color.unwrap_or(v.text_color));
        let text = (self.producer)();
        let y_text = (self.bounds.height - self.font_size) * 0.5;
        ctx.fill_text(&text, 0.0, y_text);
    }
    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}
