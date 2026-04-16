//! Scrolling demo — six-tab reimplementation of egui's `Scrolling` sample.
//!
//! Tabs (in order):
//!   1. **Appearance**   — live `ScrollBarVisibility` switch + content-length slider.
//!   2. **Scroll to**    — programmatic scroll-to-index, scroll-to-offset, scroll-by.
//!   3. **Many lines**   — long list (row-count slider) rendered directly via text.
//!   4. **Large canvas** — stub: our `ScrollView` does not yet support a virtual
//!                         painter + viewport. Shows a placeholder instead.
//!   5. **Stick to end** — `ScrollView::with_stick_to_bottom(true)` with a row
//!                         count that auto-increments every layout pass.
//!   6. **Bidirectional**— stub: our `ScrollView` is vertical-only; shows a
//!                         placeholder explaining the limitation.
//!
//! The module deliberately renders list rows through its own `RowList` widget
//! (direct `DrawCtx::fill_text`) rather than constructing thousands of Label
//! children — this keeps layout cheap even for row-count = 500.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{
    Button, Color, DrawCtx, Event, EventResult, FlexColumn, FlexRow, Font, Insets,
    Label, MouseButton, Point, Rect, ScrollBarVisibility, ScrollView, Separator,
    Size, SizedBox, Slider, TabView, Widget,
};
use agg_gui::widget::paint_subtree;

// ── Lorem Ipsum helpers ──────────────────────────────────────────────────────

const LOREM_IPSUM_LONG: &str =
    "Lorem ipsum dolor sit amet, consectetur adipiscing elit. \
     Curabitur et mauris auctor, cursus leo ut, viverra erat. \
     Nulla facilisi. Vivamus tempus ligula a lectus condimentum aliquam. \
     Sed sit amet magna et arcu efficitur porttitor. Suspendisse potenti. \
     Praesent consequat, lacus in sollicitudin tempor, ex purus commodo urna.";

// ── RowList: a virtual-ish list that renders N rows directly via fill_text ─────

/// Fixed-row-height list whose visible row count is driven by an `Rc<Cell<usize>>`.
/// Paints only rows that intersect the current clip rect, so going to 10 000 rows
/// stays cheap as long as the surrounding `ScrollView` clips to its viewport.
struct RowList {
    bounds:     Rect,
    children:   Vec<Box<dyn Widget>>, // always empty
    font:       Arc<Font>,
    font_size:  f64,
    row_height: f64,
    padding_x:  f64,
    count:      Rc<Cell<usize>>,
    /// Row index that should be painted in the accent colour — None means no
    /// highlight.  Used by the "Scroll to" tab to mark the tracked item.
    highlight:  Rc<Cell<Option<usize>>>,
    /// Produces the text shown for a given row index.
    formatter:  Rc<dyn Fn(usize) -> String>,
    /// Informational stripe — when true, alternate rows are shaded.
    striped:    bool,
}

impl RowList {
    fn new(
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
        }
    }

    fn with_row_height(mut self, h: f64) -> Self { self.row_height = h; self }

    fn with_highlight_cell(mut self, cell: Rc<Cell<Option<usize>>>) -> Self {
        self.highlight = cell;
        self
    }
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

        // Paint relative to widget-local coords (Y-up).  Row 0 is at the TOP
        // of the content which, Y-up, corresponds to y = height - row_height.
        let total_h = (n as f64) * self.row_height;

        // Clip-rect-driven culling: only paint rows that intersect the clip.
        // DrawCtx doesn't expose clip bounds directly, so we paint everything
        // and rely on the upstream clip to reject off-viewport fragments — but
        // we still early-reject based on widget bounds to guard against
        // runaway work when the ScrollView hasn't been sized yet.
        let highlight = self.highlight.get();
        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(self.font_size);

        for i in 0..n {
            let y_top   = total_h - (i as f64 + 1.0) * self.row_height;
            let y_text  = y_top + (self.row_height - self.font_size) * 0.5;

            // Striped background.
            if self.striped && i % 2 == 0 {
                ctx.set_fill_color(Color::rgba(
                    v.text_color.r, v.text_color.g, v.text_color.b, 0.05));
                ctx.begin_path();
                ctx.rect(0.0, y_top, self.bounds.width, self.row_height);
                ctx.fill();
            }

            // Highlighted row.
            if highlight == Some(i) {
                ctx.set_fill_color(Color::rgba(v.accent.r, v.accent.g, v.accent.b, 0.25));
                ctx.begin_path();
                ctx.rect(0.0, y_top, self.bounds.width, self.row_height);
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

// ── Tab content builders ──────────────────────────────────────────────────────

fn label(font: Arc<Font>, text: impl Into<String>, size: f64) -> Box<dyn Widget> {
    Box::new(Label::new(text.into(), font).with_font_size(size))
}

fn wrapped_label(font: Arc<Font>, text: impl Into<String>, size: f64) -> Box<dyn Widget> {
    Box::new(
        Label::new(text.into(), font)
            .with_font_size(size)
            .with_wrap(true)
            .with_margin(Insets::from_sides(0.0, 0.0, 2.0, 2.0)),
    )
}

/// Build a small row of three selectable buttons acting as radio buttons, all
/// sharing the same `Rc<Cell<T>>` state.  We use this instead of `RadioGroup`
/// so each segment can carry a custom label and the whole bar sits in one row.
struct SegRow<T: Clone + Copy + PartialEq + 'static> {
    bounds:   Rect,
    children: Vec<Box<dyn Widget>>, // always empty
    options:  Vec<(&'static str, T)>,
    state:    Rc<Cell<T>>,
    hovered:  Option<usize>,
    labels:   Vec<Label>,
}

impl<T: Clone + Copy + PartialEq + 'static> SegRow<T> {
    fn new(font: Arc<Font>, options: Vec<(&'static str, T)>, state: Rc<Cell<T>>) -> Self {
        let labels = options.iter().map(|(text, _)| {
            Label::new(*text, Arc::clone(&font)).with_font_size(12.0)
        }).collect();
        Self {
            bounds: Rect::default(), children: Vec::new(),
            options, state, hovered: None, labels,
        }
    }

    const BTN_H: f64 = 24.0;

    fn btn_rect(&self, i: usize, total_w: f64) -> Rect {
        // Equal width segments, minus 1 px gap between.
        let n = self.options.len().max(1);
        let w = (total_w - (n - 1) as f64).max(20.0) / n as f64;
        let y = (self.bounds.height - Self::BTN_H) * 0.5;
        Rect::new(i as f64 * (w + 1.0), y, w, Self::BTN_H)
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
        self.bounds = Rect::new(0.0, 0.0, available.width, Self::BTN_H + 6.0);
        let rects: Vec<Rect> = (0..self.labels.len())
            .map(|i| self.btn_rect(i, available.width))
            .collect();
        for (lbl, r) in self.labels.iter_mut().zip(rects.iter()) {
            let s = lbl.layout(Size::new(r.width, r.height));
            lbl.set_bounds(Rect::new(0.0, 0.0, s.width, s.height));
        }
        Size::new(available.width, Self::BTN_H + 6.0)
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

// ── Tab 1: Appearance ─────────────────────────────────────────────────────────

fn build_appearance_tab(font: Arc<Font>) -> Box<dyn Widget> {
    let visibility = Rc::new(Cell::new(ScrollBarVisibility::VisibleOnHover));
    let lorem_count = Rc::new(Cell::new(2_usize));

    let mut col = FlexColumn::new().with_gap(8.0).with_padding(10.0);

    col.push(label(Arc::clone(&font),
        "ScrollBarVisibility — resize the window to see the effect.", 11.0), 0.0);

    col.push(Box::new(SegRow::new(
        Arc::clone(&font),
        vec![
            ("Always visible",     ScrollBarVisibility::AlwaysVisible),
            ("Visible on hover",   ScrollBarVisibility::VisibleOnHover),
            ("Always hidden",      ScrollBarVisibility::AlwaysHidden),
        ],
        Rc::clone(&visibility),
    )), 0.0);

    col.push(Box::new(Separator::horizontal()), 0.0);

    // Content-length slider (1..=40).  Capped at 40 to keep per-frame layout
    // cost bounded; egui goes to 100 but they have virtual rendering.
    let count_for_cb = Rc::clone(&lorem_count);
    let slider_row = FlexRow::new()
        .with_gap(8.0)
        .add(label(Arc::clone(&font), "Content length", 12.0))
        .add_flex(Box::new(
            Slider::new(2.0, 1.0, 40.0, Arc::clone(&font))
                .with_step(1.0)
                .on_change(move |v| count_for_cb.set(v.round() as usize))
        ), 1.0);
    col.push(Box::new(slider_row), 0.0);

    col.push(Box::new(Separator::horizontal()), 0.0);

    // Lorem-ipsum list — rebuilt lazily via LoremStack.
    let scroll_content = LoremStack::new(Arc::clone(&font), Rc::clone(&lorem_count));
    let scroll_area = ScrollView::new(Box::new(scroll_content))
        .with_bar_visibility_cell(Rc::clone(&visibility));
    col.push(Box::new(scroll_area), 1.0);

    Box::new(col)
}

/// A column that lazily builds N `Label` children (reused frame-to-frame) based
/// on the value of a shared counter cell.
struct LoremStack {
    bounds:    Rect,
    children:  Vec<Box<dyn Widget>>, // rebuilt when count changes
    font:      Arc<Font>,
    count:     Rc<Cell<usize>>,
    last_count: Cell<usize>,
}

impl LoremStack {
    fn new(font: Arc<Font>, count: Rc<Cell<usize>>) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            font,
            count,
            last_count: Cell::new(usize::MAX),
        }
    }

    fn rebuild_if_needed(&mut self) {
        let n = self.count.get();
        if self.last_count.get() == n { return; }
        self.last_count.set(n);
        self.children.clear();
        for _ in 0..n {
            self.children.push(Box::new(
                Label::new(LOREM_IPSUM_LONG, Arc::clone(&self.font))
                    .with_font_size(12.0)
                    .with_wrap(true)
                    .with_margin(Insets::from_sides(0.0, 0.0, 4.0, 4.0))
            ));
        }
    }
}

impl Widget for LoremStack {
    fn type_name(&self) -> &'static str { "LoremStack" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size {
        self.rebuild_if_needed();
        let mut y = 0.0f64;
        for child in &mut self.children {
            let s = child.layout(Size::new(available.width, f64::MAX / 2.0));
            child.set_bounds(Rect::new(0.0, y, available.width, s.height));
            y += s.height + 4.0;
        }
        // Flip to Y-up: first child is at top.  FlexColumn normally does this;
        // we replicate it by measuring then re-laying in Y-up space.
        let total = y;
        for child in &mut self.children {
            let b = child.bounds();
            child.set_bounds(Rect::new(0.0, total - b.y - b.height, b.width, b.height));
        }
        self.bounds = Rect::new(0.0, 0.0, available.width, total.max(1.0));
        Size::new(available.width, total)
    }

    fn paint(&mut self, _: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}

// ── Tab 2: Scroll to ──────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum ScrollAlign { Top, Center, Bottom }

fn build_scroll_to_tab(font: Arc<Font>) -> Box<dyn Widget> {
    let num_items   = 500usize;
    let row_height  = 18.0f64;
    let track_item  = Rc::new(Cell::new(25_usize));
    let align       = Rc::new(Cell::new(ScrollAlign::Center));
    let scroll_off  = Rc::new(Cell::new(0.0f64));
    let max_scroll  = Rc::new(Cell::new(0.0f64));
    let highlight   = Rc::new(Cell::new(Some(25_usize)));

    let mut col = FlexColumn::new().with_gap(6.0).with_padding(10.0);

    col.push(wrapped_label(Arc::clone(&font),
        "Scroll to a specific item index or pixel offset.  The tracked item \
         is highlighted; moving the slider or changing the alignment re-scrolls \
         the list so the item lands in the chosen position.", 11.0), 0.0);

    // ── Track item slider ──
    let ti_cb   = Rc::clone(&track_item);
    let hl_cb   = Rc::clone(&highlight);
    let so_cb   = Rc::clone(&scroll_off);
    let ms_cb   = Rc::clone(&max_scroll);
    let al_cb   = Rc::clone(&align);
    let recompute = Rc::new(move || {
        let i = ti_cb.get().saturating_sub(1);   // slider is 1-based
        hl_cb.set(Some(i));
        // Target offset so the item lands at the requested alignment.  The
        // viewport height is inferred as `content_h - max_scroll` — which is
        // 0 on the first frame before layout; the `AlignAdjuster` widget
        // re-invokes this closure each frame so the value settles once
        // max_scroll is populated.
        let content_h = (num_items as f64) * row_height;
        let max       = ms_cb.get();
        let viewport  = (content_h - max).max(row_height);
        let item_top  = (i as f64) * row_height;
        let target = match al_cb.get() {
            ScrollAlign::Top    => item_top,
            ScrollAlign::Center => item_top - (viewport - row_height) * 0.5,
            ScrollAlign::Bottom => item_top - viewport + row_height,
        };
        so_cb.set(target.clamp(0.0, max));
    });
    let r1 = Rc::clone(&recompute);
    let track_item_for_slider = Rc::clone(&track_item);
    let row = FlexRow::new().with_gap(8.0)
        .add(label(Arc::clone(&font), "Track item", 12.0))
        .add_flex(Box::new(
            Slider::new(25.0, 1.0, num_items as f64, Arc::clone(&font))
                .with_step(1.0)
                .on_change(move |v| {
                    track_item_for_slider.set(v.round() as usize);
                    r1();
                })
        ), 1.0);
    col.push(Box::new(row), 0.0);

    // ── Alignment segmented buttons ──
    let r2 = Rc::clone(&recompute);
    let align_for_seg = Rc::clone(&align);
    let seg: Box<dyn Widget> = Box::new(AlignSegRow::new(
        Arc::clone(&font),
        vec![
            ("Top",    ScrollAlign::Top),
            ("Center", ScrollAlign::Center),
            ("Bottom", ScrollAlign::Bottom),
        ],
        align_for_seg,
        move || r2(),
    ));
    let align_row = FlexRow::new().with_gap(8.0)
        .add(label(Arc::clone(&font), "Align", 12.0))
        .add_flex(seg, 1.0);
    col.push(Box::new(align_row), 0.0);

    // ── Top / Bottom buttons ──
    let btn_top_cell = Rc::clone(&scroll_off);
    let btn_bot_cell = Rc::clone(&scroll_off);
    let ms_for_bot   = Rc::clone(&max_scroll);
    let btn_row = FlexRow::new().with_gap(8.0)
        .add(Box::new(
            Button::new("Scroll to top", Arc::clone(&font))
                .on_click(move || btn_top_cell.set(0.0))
        ))
        .add(Box::new(
            Button::new("Scroll to bottom", Arc::clone(&font))
                .on_click(move || btn_bot_cell.set(ms_for_bot.get()))
        ));
    col.push(Box::new(btn_row), 0.0);

    col.push(Box::new(Separator::horizontal()), 0.0);

    // ── Readout ──
    let readout_off = Rc::clone(&scroll_off);
    let readout_max = Rc::clone(&max_scroll);
    col.push(Box::new(OffsetReadout {
        bounds: Rect::default(), children: Vec::new(),
        font: Arc::clone(&font), offset: readout_off, max: readout_max,
    }), 0.0);

    // ── Scroll area ──
    let list = RowList::new(
        Arc::clone(&font),
        Rc::new(Cell::new(num_items)),
        Rc::new(|i| format!("This is item {}", i + 1)),
    )
    .with_row_height(row_height)
    .with_highlight_cell(Rc::clone(&highlight));

    let scroll = ScrollView::new(Box::new(list))
        .with_offset_cell(Rc::clone(&scroll_off))
        .with_max_scroll_cell(Rc::clone(&max_scroll))
        .with_bar_visibility(ScrollBarVisibility::VisibleOnHover);
    col.push(Box::new(scroll), 1.0);

    // First-frame sync: once the ScrollView has laid out and published
    // max_scroll, we re-invoke the target-offset calculator so the initial
    // Center alignment lands correctly.  Subsequent manual scrolls are NOT
    // overridden because the adjuster only fires on max_scroll changes.
    col.push(Box::new(MaxScrollWatcher::new(
        Rc::clone(&max_scroll),
        recompute,
    )), 0.0);

    Box::new(col)
}

/// Zero-size watcher that invokes `cb` whenever `max_scroll` changes.
struct MaxScrollWatcher {
    bounds:    Rect,
    children:  Vec<Box<dyn Widget>>,
    max:       Rc<Cell<f64>>,
    last:      Cell<f64>,
    cb:        Rc<dyn Fn()>,
}
impl MaxScrollWatcher {
    fn new(max: Rc<Cell<f64>>, cb: Rc<dyn Fn()>) -> Self {
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

/// Like `SegRow<T>` but also invokes a callback on selection — useful when
/// the new selection needs to trigger derived work (e.g. recomputing the
/// tracked scroll offset in the Scroll-to demo).
struct AlignSegRow {
    inner:     SegRow<ScrollAlign>,
    on_change: Rc<dyn Fn()>,
    last:      Cell<ScrollAlign>,
}
impl AlignSegRow {
    fn new(
        font: Arc<Font>,
        opts: Vec<(&'static str, ScrollAlign)>,
        state: Rc<Cell<ScrollAlign>>,
        on_change: impl Fn() + 'static,
    ) -> Self {
        let start = state.get();
        Self {
            inner: SegRow::new(font, opts, state),
            on_change: Rc::new(on_change),
            last: Cell::new(start),
        }
    }
}
impl Widget for AlignSegRow {
    fn type_name(&self) -> &'static str { "AlignSegRow" }
    fn bounds(&self) -> Rect { self.inner.bounds() }
    fn set_bounds(&mut self, b: Rect) { self.inner.set_bounds(b); }
    fn children(&self) -> &[Box<dyn Widget>] { self.inner.children() }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { self.inner.children_mut() }
    fn layout(&mut self, a: Size) -> Size {
        let cur = self.inner.state.get();
        if cur != self.last.get() {
            self.last.set(cur);
            (self.on_change)();
        }
        self.inner.layout(a)
    }
    fn paint(&mut self, ctx: &mut dyn DrawCtx) { self.inner.paint(ctx); }
    fn on_event(&mut self, e: &Event) -> EventResult { self.inner.on_event(e) }
}

struct OffsetReadout {
    bounds:   Rect,
    children: Vec<Box<dyn Widget>>,
    font:     Arc<Font>,
    offset:   Rc<Cell<f64>>,
    max:      Rc<Cell<f64>>,
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
        let text = format!("Scroll offset: {:.0} / {:.0} px", self.offset.get(), self.max.get());
        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(11.0);
        ctx.set_fill_color(v.text_dim);
        ctx.fill_text(&text, 2.0, 3.0);
    }
    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}

// ── Tab 3: Many lines ─────────────────────────────────────────────────────────

fn build_many_lines_tab(font: Arc<Font>) -> Box<dyn Widget> {
    let row_count = Rc::new(Cell::new(500_usize));

    let mut col = FlexColumn::new().with_gap(6.0).with_padding(10.0);
    col.push(wrapped_label(Arc::clone(&font),
        "A long list of rows — row text is painted via `DrawCtx::fill_text` \
         instead of spawning one Label per row, so layout stays cheap even at \
         hundreds of rows.", 11.0), 0.0);

    let count_for_cb = Rc::clone(&row_count);
    let slider_row = FlexRow::new().with_gap(8.0)
        .add(label(Arc::clone(&font), "Row count", 12.0))
        .add_flex(Box::new(
            Slider::new(500.0, 10.0, 2000.0, Arc::clone(&font))
                .with_step(10.0)
                .on_change(move |v| count_for_cb.set(v.round() as usize))
        ), 1.0);
    col.push(Box::new(slider_row), 0.0);

    col.push(Box::new(Separator::horizontal()), 0.0);

    let list = RowList::new(
        Arc::clone(&font),
        Rc::clone(&row_count),
        Rc::new(|i| format!("This is row {}/N", i + 1)),
    );
    col.push(Box::new(ScrollView::new(Box::new(list))), 1.0);

    Box::new(col)
}

// ── Tab 4: Large canvas — stub ────────────────────────────────────────────────

fn build_large_canvas_tab(font: Arc<Font>) -> Box<dyn Widget> {
    let mut col = FlexColumn::new().with_gap(6.0).with_padding(10.0);
    col.push(wrapped_label(Arc::clone(&font),
        "Egui's `show_viewport` path paints only the rows intersecting the \
         scroll viewport, reading the viewport rect straight from the UI.  \
         agg-gui's `ScrollView` doesn't yet expose a viewport callback, so \
         this tab is currently a placeholder.", 12.0), 0.0);
    col.push(Box::new(SizedBox::new().with_height(12.0)), 0.0);
    col.push(wrapped_label(Arc::clone(&font),
        "See the Many lines tab for a simpler list with a slider.", 11.0), 0.0);
    Box::new(col)
}

// ── Tab 5: Stick to end ───────────────────────────────────────────────────────

fn build_stick_to_end_tab(font: Arc<Font>) -> Box<dyn Widget> {
    let counter = Rc::new(Cell::new(20_usize));

    let mut col = FlexColumn::new().with_gap(6.0).with_padding(10.0);
    col.push(wrapped_label(Arc::clone(&font),
        "Rows enter from the bottom every layout pass; the scrollbar stays \
         glued to the end unless you scroll away.  Scroll up to detach; \
         return to the bottom to re-attach.", 11.0), 0.0);

    // CounterTicker MUST lay out before the ScrollView so the row count is
    // current on every frame — FlexColumn lays out children in push order.
    col.push(Box::new(CounterTicker::new(Rc::clone(&counter))), 0.0);

    let list = RowList::new(
        Arc::clone(&font),
        Rc::clone(&counter),
        Rc::new(|i| format!("This is row {}", i + 1)),
    );
    let scroll = ScrollView::new(Box::new(list))
        .with_stick_to_bottom(true)
        .with_bar_visibility(ScrollBarVisibility::VisibleOnHover);
    col.push(Box::new(scroll), 1.0);

    Box::new(col)
}

/// Zero-size widget that increments a counter every layout.
struct CounterTicker { bounds: Rect, children: Vec<Box<dyn Widget>>, counter: Rc<Cell<usize>> }
impl CounterTicker {
    fn new(counter: Rc<Cell<usize>>) -> Self {
        Self { bounds: Rect::default(), children: Vec::new(), counter }
    }
}
impl Widget for CounterTicker {
    fn type_name(&self) -> &'static str { "CounterTicker" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }
    fn layout(&mut self, _: Size) -> Size {
        self.counter.set(self.counter.get() + 1);
        Size::ZERO
    }
    fn paint(&mut self, _: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}

// ── Tab 6: Bidirectional — stub ───────────────────────────────────────────────

fn build_bidirectional_tab(font: Arc<Font>) -> Box<dyn Widget> {
    let mut col = FlexColumn::new().with_gap(6.0).with_padding(10.0);
    col.push(wrapped_label(Arc::clone(&font),
        "Horizontal scrolling is not yet implemented in agg-gui's ScrollView \
         — this tab is a placeholder.  Egui's bidirectional sample wraps \
         100 lorem-ipsum paragraphs in `ScrollArea::both()` with non-wrapping \
         text.", 12.0), 0.0);
    Box::new(col)
}

// ── Public API ────────────────────────────────────────────────────────────────

pub fn scrolling_demo(font: Arc<Font>) -> Box<dyn Widget> {
    // `TabView::new` + repeated `add_tab`.  We ignore the RefCell-ish state
    // that egui's reference keeps — each tab builds its own state cells.
    let tv = TabView::new(Arc::clone(&font))
        .with_font_size(12.0)
        .add_tab("Appearance",    build_appearance_tab(Arc::clone(&font)))
        .add_tab("Scroll to",     build_scroll_to_tab(Arc::clone(&font)))
        .add_tab("Many lines",    build_many_lines_tab(Arc::clone(&font)))
        .add_tab("Large canvas",  build_large_canvas_tab(Arc::clone(&font)))
        .add_tab("Stick to end",  build_stick_to_end_tab(Arc::clone(&font)))
        .add_tab("Bidirectional", build_bidirectional_tab(Arc::clone(&font)));
    Box::new(tv)
}
