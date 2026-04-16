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

use agg_gui::{
    Color, DrawCtx, Event, EventResult, Font, Label, MouseButton, Point, Rect,
    Size, Widget,
};
use agg_gui::widget::paint_subtree;

// ── Constants ───────────────────────────────────────────────────────────────

pub const LOREM_IPSUM_LONG: &str =
    "Lorem ipsum dolor sit amet, consectetur adipiscing elit. \
     Curabitur et mauris auctor, cursus leo ut, viverra erat. \
     Nulla facilisi. Vivamus tempus ligula a lectus condimentum aliquam. \
     Sed sit amet magna et arcu efficitur porttitor. Suspendisse potenti. \
     Praesent consequat, lacus in sollicitudin tempor, ex purus commodo urna.";

// ── RowList: virtual row list with direct text painting ────────────────────

pub struct RowList {
    bounds:     Rect,
    children:   Vec<Box<dyn Widget>>, // always empty
    font:       Arc<Font>,
    font_size:  f64,
    row_height: f64,
    padding_x:  f64,
    count:      Rc<Cell<usize>>,
    highlight:  Rc<Cell<Option<usize>>>,
    formatter:  Rc<dyn Fn(usize) -> String>,
    striped:    bool,
    /// When bound, rows outside this content-space rect are skipped in paint.
    /// The rect uses top-down coordinates (y = 0 at top of content).
    viewport:   Option<Rc<Cell<Rect>>>,
}

impl RowList {
    pub fn new(
        font:      Arc<Font>,
        count:     Rc<Cell<usize>>,
        formatter: Rc<dyn Fn(usize) -> String>,
    ) -> Self {
        Self {
            bounds:     Rect::default(),
            children:   Vec::new(),
            font,
            font_size:  12.0,
            row_height: 18.0,
            padding_x:  8.0,
            count,
            highlight:  Rc::new(Cell::new(None)),
            formatter,
            striped:    true,
            viewport:   None,
        }
    }

    pub fn with_row_height(mut self, h: f64) -> Self { self.row_height = h; self }
    pub fn with_highlight_cell(mut self, c: Rc<Cell<Option<usize>>>) -> Self {
        self.highlight = c; self
    }
    pub fn with_viewport_cell(mut self, c: Rc<Cell<Rect>>) -> Self {
        self.viewport = Some(c); self
    }
    #[allow(dead_code)]
    pub fn with_striped(mut self, s: bool) -> Self { self.striped = s; self }
}

impl Widget for RowList {
    fn type_name(&self) -> &'static str { "RowList" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size {
        let n = self.count.get();
        let h = (n as f64) * self.row_height;
        self.bounds = Rect::new(0.0, 0.0, available.width, h);
        Size::new(available.width, h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let n = self.count.get();
        if n == 0 { return; }
        let total_h = (n as f64) * self.row_height;

        // Pick which row range to paint.  Without a viewport cell we paint
        // everything (and rely on upstream clip to cull).
        let (first, last) = match &self.viewport {
            Some(cell) => {
                let vp = cell.get();
                let f = (vp.y / self.row_height).floor().max(0.0) as usize;
                let l = ((vp.y + vp.height) / self.row_height).ceil() as usize + 1;
                (f.min(n), l.min(n))
            }
            None => (0, n),
        };

        let highlight = self.highlight.get();
        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(self.font_size);

        for i in first..last {
            // Row 0 at top of content → in Y-up local space:
            //   y_bottom_of_row = total_h - (i + 1) * row_height
            let y_bottom = total_h - (i as f64 + 1.0) * self.row_height;
            let y_text   = y_bottom + (self.row_height - self.font_size) * 0.5;

            if self.striped && i % 2 == 0 {
                ctx.set_fill_color(Color::rgba(
                    v.text_color.r, v.text_color.g, v.text_color.b, 0.05));
                ctx.begin_path();
                ctx.rect(0.0, y_bottom, self.bounds.width, self.row_height);
                ctx.fill();
            }
            if highlight == Some(i) {
                ctx.set_fill_color(Color::rgba(
                    v.accent.r, v.accent.g, v.accent.b, 0.25));
                ctx.begin_path();
                ctx.rect(0.0, y_bottom, self.bounds.width, self.row_height);
                ctx.fill();
            }

            let text = (self.formatter)(i);
            let c = if highlight == Some(i) { v.accent } else { v.text_color };
            ctx.set_fill_color(c);
            ctx.fill_text(&text, self.padding_x, y_text);
        }
    }

    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}

// ── SegRow: segmented-button row ─────────────────────────────────────────────

pub struct SegRow<T: Clone + Copy + PartialEq + 'static> {
    bounds:   Rect,
    children: Vec<Box<dyn Widget>>,
    options:  Vec<(&'static str, T)>,
    state:    Rc<Cell<T>>,
    hovered:  Option<usize>,
    labels:   Vec<Label>,
    /// Optional callback fired when the selection changes (layout-driven).
    on_change: Option<Rc<dyn Fn()>>,
    last:     Cell<Option<T>>,
}

impl<T: Clone + Copy + PartialEq + 'static> SegRow<T> {
    pub fn new(
        font: Arc<Font>,
        options: Vec<(&'static str, T)>,
        state: Rc<Cell<T>>,
    ) -> Self {
        let labels = options.iter().map(|(text, _)| {
            Label::new(*text, Arc::clone(&font)).with_font_size(12.0)
        }).collect();
        let start = state.get();
        Self {
            bounds: Rect::default(), children: Vec::new(),
            options, state, hovered: None, labels,
            on_change: None, last: Cell::new(Some(start)),
        }
    }

    pub fn on_change(mut self, cb: impl Fn() + 'static) -> Self {
        self.on_change = Some(Rc::new(cb));
        self
    }

    const BTN_H:       f64 = 24.0;
    const BTN_PAD_X:   f64 = 14.0;   // horizontal padding around label
    const BTN_MIN_W:   f64 = 56.0;   // never smaller than this per segment
    const BTN_GAP:     f64 = 1.0;

    /// Natural width needed to fit every option's label + padding + gaps.
    /// Computed from the label widths stored in `self.labels` AFTER they are
    /// laid out — so `layout` calls this only after measuring labels.
    fn natural_width(&self) -> f64 {
        let n = self.labels.len().max(1);
        let per: f64 = self.labels.iter()
            .map(|l| l.bounds().width + Self::BTN_PAD_X * 2.0)
            .fold(Self::BTN_MIN_W, f64::max);
        per * n as f64 + Self::BTN_GAP * (n - 1) as f64
    }

    fn btn_rect(&self, i: usize, total_w: f64) -> Rect {
        let n = self.options.len().max(1);
        let w = ((total_w - Self::BTN_GAP * (n - 1) as f64) / n as f64).max(20.0);
        let y = (self.bounds.height - Self::BTN_H) * 0.5;
        Rect::new(i as f64 * (w + Self::BTN_GAP), y, w, Self::BTN_H)
    }

    fn hit(&self, pos: Point) -> Option<usize> {
        for i in 0..self.options.len() {
            let r = self.btn_rect(i, self.bounds.width);
            if pos.x >= r.x && pos.x <= r.x + r.width
                && pos.y >= r.y && pos.y <= r.y + r.height { return Some(i); }
        }
        None
    }
}

impl<T: Clone + Copy + PartialEq + 'static> Widget for SegRow<T> {
    fn type_name(&self) -> &'static str { "SegRow" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size {
        // First pass: measure labels so we know how wide each segment needs
        // to be.  Use a generous horizontal budget so text doesn't wrap.
        for lbl in self.labels.iter_mut() {
            let s = lbl.layout(Size::new(available.width, Self::BTN_H));
            lbl.set_bounds(Rect::new(0.0, 0.0, s.width, s.height));
        }
        // Natural "fit" width — never wider than the slot we were given.
        let natural = self.natural_width().min(available.width);
        self.bounds = Rect::new(0.0, 0.0, natural, Self::BTN_H + 6.0);
        // Fire on_change when state changed since last layout.
        let cur = self.state.get();
        if self.last.get() != Some(cur) {
            self.last.set(Some(cur));
            if let Some(cb) = &self.on_change { cb(); }
        }
        Size::new(natural, Self::BTN_H + 6.0)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let current = self.state.get();
        for (i, (_lbl, val)) in self.options.iter().enumerate() {
            let r = self.btn_rect(i, self.bounds.width);
            let active  = *val == current;
            let hovered = self.hovered == Some(i);
            let bg = if active       { v.accent }
                     else if hovered { v.widget_bg_hovered }
                     else            { v.widget_bg };
            ctx.set_fill_color(bg);
            ctx.begin_path();
            ctx.rounded_rect(r.x, r.y, r.width, r.height, 4.0);
            ctx.fill();

            let tc = if active { Color::white() } else { v.text_color };
            self.labels[i].set_color(tc);
            let lw = self.labels[i].bounds().width;
            let lh = self.labels[i].bounds().height;
            let lx = r.x + (r.width - lw) * 0.5;
            let ly = r.y + (r.height - lh) * 0.5;
            self.labels[i].set_bounds(Rect::new(lx, ly, lw, lh));
            ctx.save();
            ctx.translate(lx, ly);
            paint_subtree(&mut self.labels[i], ctx);
            ctx.restore();
        }
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseMove { pos } => {
                self.hovered = self.hit(*pos);
                EventResult::Ignored
            }
            Event::MouseDown { button: MouseButton::Left, pos, .. } => {
                if let Some(i) = self.hit(*pos) {
                    self.state.set(self.options[i].1);
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }
            _ => EventResult::Ignored,
        }
    }
}

// ── OffsetReadout ────────────────────────────────────────────────────────────

pub struct OffsetReadout {
    pub bounds:   Rect,
    pub children: Vec<Box<dyn Widget>>,
    pub font:     Arc<Font>,
    pub offset:   Rc<Cell<f64>>,
    pub max:      Rc<Cell<f64>>,
}
impl Widget for OffsetReadout {
    fn type_name(&self) -> &'static str { "OffsetReadout" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }
    fn layout(&mut self, a: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, a.width, 16.0);
        Size::new(a.width, 16.0)
    }
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let text = format!("Scroll offset: {:.0} / {:.0} px",
            self.offset.get(), self.max.get());
        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(11.0);
        ctx.set_fill_color(v.text_dim);
        ctx.fill_text(&text, 2.0, 3.0);
    }
    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}

// ── MaxScrollWatcher ─────────────────────────────────────────────────────────

pub struct MaxScrollWatcher {
    bounds:   Rect,
    children: Vec<Box<dyn Widget>>,
    max:      Rc<Cell<f64>>,
    last:     Cell<f64>,
    cb:       Rc<dyn Fn()>,
}
impl MaxScrollWatcher {
    pub fn new(max: Rc<Cell<f64>>, cb: Rc<dyn Fn()>) -> Self {
        Self {
            bounds: Rect::default(), children: Vec::new(),
            max, last: Cell::new(f64::NAN), cb,
        }
    }
}
impl Widget for MaxScrollWatcher {
    fn type_name(&self) -> &'static str { "MaxScrollWatcher" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }
    fn show_in_inspector(&self) -> bool { false }
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
    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}

// ── CounterTicker ────────────────────────────────────────────────────────────

pub struct CounterTicker {
    bounds:   Rect,
    children: Vec<Box<dyn Widget>>,
    counter:  Rc<Cell<usize>>,
}
impl CounterTicker {
    pub fn new(c: Rc<Cell<usize>>) -> Self {
        Self { bounds: Rect::default(), children: Vec::new(), counter: c }
    }
}
impl Widget for CounterTicker {
    fn type_name(&self) -> &'static str { "CounterTicker" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }
    fn show_in_inspector(&self) -> bool { false }
    fn layout(&mut self, _: Size) -> Size {
        self.counter.set(self.counter.get() + 1);
        Size::ZERO
    }
    fn paint(&mut self, _: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}

// ── Small label builders ─────────────────────────────────────────────────────

pub fn label(font: Arc<Font>, text: impl Into<String>, size: f64) -> Box<dyn Widget> {
    Box::new(Label::new(text.into(), font).with_font_size(size))
}

pub fn wrapped_label(font: Arc<Font>, text: impl Into<String>, size: f64) -> Box<dyn Widget> {
    Box::new(
        Label::new(text.into(), font)
            .with_font_size(size)
            .with_wrap(true)
    )
}

// ── LiveLabel: a label whose text is produced by a closure each layout ──────

pub struct LiveLabel {
    bounds:   Rect,
    children: Vec<Box<dyn Widget>>,
    font:     Arc<Font>,
    font_size: f64,
    producer: Rc<dyn Fn() -> String>,
    color:    Option<Color>,
}
impl LiveLabel {
    pub fn new(font: Arc<Font>, producer: Rc<dyn Fn() -> String>) -> Self {
        Self {
            bounds: Rect::default(), children: Vec::new(),
            font, font_size: 12.0, producer, color: None,
        }
    }
    pub fn with_font_size(mut self, s: f64) -> Self { self.font_size = s; self }
    #[allow(dead_code)]
    pub fn with_color(mut self, c: Color) -> Self { self.color = Some(c); self }
}
impl Widget for LiveLabel {
    fn type_name(&self) -> &'static str { "LiveLabel" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }
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
    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}
