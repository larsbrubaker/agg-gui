//! Appearance tab: Details collapsible with the full egui `spacing.scroll`
//! configuration, plus ScrollBarVisibility selector and Content length slider.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{
    Checkbox, CollapsingHeader, DragValue, FlexColumn, FlexRow, Font, Insets,
    Label, Rect, ScrollBarColor, ScrollBarKind, ScrollBarStyle,
    ScrollBarVisibility, ScrollView, Separator, Size, SizedBox, Slider, Widget,
};

use super::helpers::{label, wrapped_label, LiveLabel, SegRow};

// ── LoremStack ──────────────────────────────────────────────────────────────

/// Column that lazily rebuilds N lorem-ipsum labels when `count` changes.
struct LoremStack {
    bounds:     Rect,
    children:   Vec<Box<dyn Widget>>,
    font:       Arc<Font>,
    count:      Rc<Cell<usize>>,
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
                Label::new(super::helpers::LOREM_IPSUM_LONG, Arc::clone(&self.font))
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
        let total = y;
        for child in &mut self.children {
            let b = child.bounds();
            child.set_bounds(Rect::new(0.0, total - b.y - b.height, b.width, b.height));
        }
        self.bounds = Rect::new(0.0, 0.0, available.width, total.max(1.0));
        Size::new(available.width, total)
    }

    fn paint(&mut self, _: &mut dyn agg_gui::DrawCtx) {}
    fn on_event(&mut self, _: &agg_gui::Event) -> agg_gui::EventResult {
        agg_gui::EventResult::Ignored
    }
}

// ── Reusable row helper ──────────────────────────────────────────────────────

fn row_value_label(font: Arc<Font>, value_cell: Rc<Cell<f64>>, text: &str, decimals: usize) -> Box<dyn Widget> {
    let cell = value_cell;
    let t = text.to_owned();
    Box::new(FlexRow::new()
        .with_gap(8.0)
        .add(Box::new(
            DragValue::new(cell.get(), 0.0, 500.0, Arc::clone(&font))
                .with_font_size(12.0)
                .with_decimals(decimals)
                .on_change(move |v| cell.set(v))
        ))
        .add_flex(label(Arc::clone(&font), t, 12.0), 1.0))
}

// ── Public builder ───────────────────────────────────────────────────────────

pub fn build(font: Arc<Font>) -> Box<dyn Widget> {
    // Every setting is backed by its own cell so the UI can drive the bar live
    // through `ScrollBarStyle`.
    let kind       = Rc::new(Cell::new(ScrollBarKind::Floating));
    let color      = Rc::new(Cell::new(ScrollBarColor::Background));
    let visibility = Rc::new(Cell::new(ScrollBarVisibility::VisibleOnHover));

    let bar_width      = Rc::new(Cell::new(15.0_f64));
    let handle_min     = Rc::new(Cell::new(10.0_f64));
    let outer_margin   = Rc::new(Cell::new( 5.0_f64));
    let inner_margin   = Rc::new(Cell::new( 7.0_f64));
    let content_margin = Rc::new(Cell::new( 5.0_f64));
    let margin_same    = Rc::new(Cell::new(true));
    let fade_strength  = Rc::new(Cell::new(1.0_f64));
    let fade_size      = Rc::new(Cell::new(45.0_f64));
    let content_len    = Rc::new(Cell::new(2_usize));

    // Shared style cell — built from the individual cells in a Watcher widget.
    let style_cell: Rc<Cell<ScrollBarStyle>> =
        Rc::new(Cell::new(ScrollBarStyle::default()));

    let mut details = FlexColumn::new().with_gap(4.0).with_padding(6.0);

    // Row 1: Type (Solid / Floating)
    details.push(Box::new(FlexRow::new().with_gap(8.0)
        .add(label(Arc::clone(&font), "Type", 12.0))
        .add(Box::new(SegRow::new(
            Arc::clone(&font),
            vec![("Solid", ScrollBarKind::Solid), ("Floating", ScrollBarKind::Floating)],
            Rc::clone(&kind),
        )))), 0.0);

    // Row 2: Content margin: [same checkbox] [value]
    {
        let ms = Rc::clone(&margin_same);
        let same_cb = Checkbox::new("same", Arc::clone(&font), ms.get())
            .with_font_size(12.0)
            .with_state_cell(Rc::clone(&ms));
        let cm = Rc::clone(&content_margin);
        let dv = DragValue::new(cm.get(), 0.0, 50.0, Arc::clone(&font))
            .with_font_size(12.0)
            .with_decimals(0)
            .on_change(move |v| cm.set(v));
        details.push(Box::new(FlexRow::new().with_gap(8.0)
            .add(label(Arc::clone(&font), "Content margin:", 12.0))
            .add(Box::new(same_cb))
            .add(Box::new(dv))), 0.0);
    }

    // Rows 3–5: Full bar width / Minimum handle length / Outer margin
    details.push(row_value_label(Arc::clone(&font), Rc::clone(&bar_width),  "Full bar width",       0), 0.0);
    details.push(row_value_label(Arc::clone(&font), Rc::clone(&handle_min), "Minimum handle length",0), 0.0);
    details.push(row_value_label(Arc::clone(&font), Rc::clone(&outer_margin), "Outer margin",       0), 0.0);

    // Row 6: Color (Background / Foreground)
    details.push(Box::new(FlexRow::new().with_gap(8.0)
        .add(label(Arc::clone(&font), "Color", 12.0))
        .add(Box::new(SegRow::new(
            Arc::clone(&font),
            vec![
                ("Background", ScrollBarColor::Background),
                ("Foreground", ScrollBarColor::Foreground),
            ],
            Rc::clone(&color),
        )))), 0.0);

    // Row 7: Inner margin
    details.push(row_value_label(Arc::clone(&font), Rc::clone(&inner_margin), "Inner margin", 0), 0.0);

    details.push(Box::new(Separator::horizontal()), 0.0);

    // Rows 8–9: Fade strength / Fade size
    {
        let fs = Rc::clone(&fade_strength);
        let fade_dv = DragValue::new(fs.get(), 0.0, 1.0, Arc::clone(&font))
            .with_font_size(12.0)
            .with_decimals(2)
            .with_step(0.05)
            .on_change(move |v| fs.set(v));
        details.push(Box::new(FlexRow::new().with_gap(8.0)
            .add(Box::new(fade_dv))
            .add_flex(label(Arc::clone(&font), "Fade strength", 12.0), 1.0)), 0.0);
    }
    details.push(row_value_label(Arc::clone(&font), Rc::clone(&fade_size), "Fade size", 0), 0.0);

    // Apply-cells-to-style-cell watcher — runs every layout.
    details.push(Box::new(StyleComposer {
        bounds: Rect::default(), children: Vec::new(),
        kind:            Rc::clone(&kind),
        color:           Rc::clone(&color),
        bar_width:       Rc::clone(&bar_width),
        handle_min:      Rc::clone(&handle_min),
        outer_margin:    Rc::clone(&outer_margin),
        inner_margin:    Rc::clone(&inner_margin),
        content_margin:  Rc::clone(&content_margin),
        margin_same:     Rc::clone(&margin_same),
        fade_strength:   Rc::clone(&fade_strength),
        fade_size:       Rc::clone(&fade_size),
        out:             Rc::clone(&style_cell),
    }), 0.0);

    // ── Outer tab layout ──
    let mut col = FlexColumn::new().with_gap(6.0).with_padding(10.0);

    col.push(Box::new(
        CollapsingHeader::new("Details", Arc::clone(&font))
            .default_open(true)
            .with_content(Box::new(details))
    ), 0.0);

    // ScrollBarVisibility selector (includes VisibleWhenNeeded like egui).
    col.push(Box::new(FlexRow::new().with_gap(8.0)
        .add(label(Arc::clone(&font), "ScrollBarVisibility:", 12.0))
        .add_flex(Box::new(SegRow::new(
            Arc::clone(&font),
            vec![
                ("AlwaysHidden",       ScrollBarVisibility::AlwaysHidden),
                ("VisibleWhenNeeded",  ScrollBarVisibility::VisibleWhenNeeded),
                ("VisibleOnHover",     ScrollBarVisibility::VisibleOnHover),
                ("AlwaysVisible",      ScrollBarVisibility::AlwaysVisible),
            ],
            Rc::clone(&visibility),
        )), 1.0)), 0.0);

    col.push(wrapped_label(Arc::clone(&font),
        "When to show scroll bars; resize the window to see the effect.", 11.0), 0.0);

    col.push(Box::new(Separator::horizontal()), 0.0);

    // Content length slider with numeric readout AFTER the slider.
    {
        let cl_for_slider = Rc::clone(&content_len);
        let cl_for_label  = Rc::clone(&content_len);
        col.push(Box::new(FlexRow::new().with_gap(8.0)
            .add(label(Arc::clone(&font), "Content length", 12.0))
            .add_flex(Box::new(
                Slider::new(2.0, 1.0, 100.0, Arc::clone(&font))
                    .with_step(1.0)
                    .on_change(move |v| cl_for_slider.set(v.round() as usize))
            ), 1.0)
            .add(Box::new(SizedBox::new().with_width(8.0)))
            .add(Box::new(LiveLabel::new(
                Arc::clone(&font),
                Rc::new(move || format!("{}", cl_for_label.get())),
            ).with_font_size(12.0)))), 0.0);
    }

    col.push(Box::new(Separator::horizontal()), 0.0);

    // The demo ScrollView whose style is driven live by the cells above.
    let scroll_content = LoremStack::new(Arc::clone(&font), Rc::clone(&content_len));
    let scroll_area = ScrollView::new(Box::new(scroll_content))
        .with_bar_visibility_cell(Rc::clone(&visibility))
        .with_style_cell(Rc::clone(&style_cell));
    col.push(Box::new(scroll_area), 1.0);

    Box::new(col)
}

// ── StyleComposer: packs individual cells into the ScrollBarStyle cell ───────

struct StyleComposer {
    bounds:          Rect,
    children:        Vec<Box<dyn Widget>>,
    kind:            Rc<Cell<ScrollBarKind>>,
    color:           Rc<Cell<ScrollBarColor>>,
    bar_width:       Rc<Cell<f64>>,
    handle_min:      Rc<Cell<f64>>,
    outer_margin:    Rc<Cell<f64>>,
    inner_margin:    Rc<Cell<f64>>,
    content_margin:  Rc<Cell<f64>>,
    margin_same:     Rc<Cell<bool>>,
    fade_strength:   Rc<Cell<f64>>,
    fade_size:       Rc<Cell<f64>>,
    out:             Rc<Cell<ScrollBarStyle>>,
}

impl Widget for StyleComposer {
    fn type_name(&self) -> &'static str { "StyleComposer" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }
    fn layout(&mut self, _: Size) -> Size {
        let s = ScrollBarStyle {
            bar_width:         self.bar_width.get(),
            handle_min_length: self.handle_min.get(),
            outer_margin:      self.outer_margin.get(),
            inner_margin:      self.inner_margin.get(),
            content_margin:    self.content_margin.get(),
            margin_same:       self.margin_same.get(),
            kind:              self.kind.get(),
            color:             self.color.get(),
            fade_strength:     self.fade_strength.get(),
            fade_size:         self.fade_size.get(),
        };
        self.out.set(s);
        Size::ZERO
    }
    fn paint(&mut self, _: &mut dyn agg_gui::DrawCtx) {}
    fn on_event(&mut self, _: &agg_gui::Event) -> agg_gui::EventResult {
        agg_gui::EventResult::Ignored
    }
}
