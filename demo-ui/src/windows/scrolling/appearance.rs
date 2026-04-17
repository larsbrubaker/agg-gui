//! Appearance tab.  Mirrors egui's `ScrollAppearance`:
//!
//! 1. **Presets** row (Solid / Thin / Floating) — each button replaces the
//!    global scroll-bar style so every `ScrollView` in the app restyles.
//! 2. **Details** collapsing header — a grid of compact controls (each ~60 px
//!    wide) with a descriptive label to the right; dragging any control also
//!    writes to the global style.
//! 3. **ScrollBarVisibility** selector (AlwaysHidden / VisibleWhenNeeded /
//!    AlwaysVisible) — drives the global scroll visibility,
//!    since visibility is per-area in egui.
//! 4. **Content length** slider with the numeric value shown to its right.
//! 5. The demo ScrollView with N lorem-ipsum paragraphs.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{
    Checkbox, CollapsingHeader, DragValue, FlexColumn, FlexRow, Font, Insets,
    Label, Rect, ScrollBarColor, ScrollBarKind, ScrollBarStyle,
    ScrollBarVisibility, ScrollView, Separator, Size, SizedBox, Slider, Widget,
    current_scroll_style, set_scroll_style,
};

use super::helpers::{label, wrapped_label, LiveLabel, SegRow};

const CTRL_W: f64 = 70.0;   // Width of each compact control.

// ── Preset type (used only to pick one of three buttons) ─────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum Preset { Solid, Thin, Floating }

// ── LoremStack ──────────────────────────────────────────────────────────────

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
            bounds: Rect::default(), children: Vec::new(),
            font, count, last_count: Cell::new(usize::MAX),
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

// ── Shared refs used by Details controls ────────────────────────────────────
//
// Every control writes its new value into its own cell and then calls a shared
// `apply()` closure which rebuilds a `ScrollBarStyle` from ALL cells and pushes
// it to `set_scroll_style` — so adjusting any one knob updates every
// `ScrollView` in the application at once.

struct StyleCells {
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
}

impl StyleCells {
    fn build_from(s: ScrollBarStyle) -> Self {
        Self {
            kind:           Rc::new(Cell::new(s.kind)),
            color:          Rc::new(Cell::new(s.color)),
            bar_width:      Rc::new(Cell::new(s.bar_width)),
            handle_min:     Rc::new(Cell::new(s.handle_min_length)),
            outer_margin:   Rc::new(Cell::new(s.outer_margin)),
            inner_margin:   Rc::new(Cell::new(s.inner_margin)),
            content_margin: Rc::new(Cell::new(s.content_margin)),
            margin_same:    Rc::new(Cell::new(s.margin_same)),
            fade_strength:  Rc::new(Cell::new(s.fade_strength)),
            fade_size:      Rc::new(Cell::new(s.fade_size)),
        }
    }

    fn compose(&self) -> ScrollBarStyle {
        ScrollBarStyle {
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
        }
    }

    fn load(&self, s: ScrollBarStyle) {
        self.kind.set(s.kind);
        self.color.set(s.color);
        self.bar_width.set(s.bar_width);
        self.handle_min.set(s.handle_min_length);
        self.outer_margin.set(s.outer_margin);
        self.inner_margin.set(s.inner_margin);
        self.content_margin.set(s.content_margin);
        self.margin_same.set(s.margin_same);
        self.fade_strength.set(s.fade_strength);
        self.fade_size.set(s.fade_size);
    }
}

// ── Compact row: a narrow drag-value + descriptive label ─────────────────────

fn drag_row(
    font:     Arc<Font>,
    cell:     Rc<Cell<f64>>,
    min:      f64,
    max:      f64,
    step:     f64,
    decimals: usize,
    description: &'static str,
    apply:    Rc<dyn Fn()>,
) -> Box<dyn Widget> {
    let cb_cell = Rc::clone(&cell);
    let dv = DragValue::new(cell.get(), min, max, Arc::clone(&font))
        .with_font_size(12.0)
        .with_decimals(decimals)
        .with_step(step)
        .on_change(move |v| { cb_cell.set(v); apply(); });

    Box::new(FlexRow::new().with_gap(10.0)
        .add(Box::new(SizedBox::new().with_width(CTRL_W).with_child(Box::new(dv))))
        .add_flex(label(font, description, 12.0), 1.0))
}

fn apply_preset(cells: &StyleCells, preset: Preset, visibility: &Rc<Cell<ScrollBarVisibility>>) {
    let s = match preset {
        Preset::Solid    => ScrollBarStyle::solid(),
        Preset::Thin     => ScrollBarStyle::thin(),
        Preset::Floating => ScrollBarStyle::floating(),
    };
    cells.load(s);
    set_scroll_style(s);
    // Presets also nudge visibility globally so every scrollbar in the app
    // picks up the new preset — Solid/Thin are "always visible" by
    // convention; Floating hides until hovered.
    // With our consolidated `VisibleWhenNeeded`, the Floating preset's
    // "show on hover" behaviour is automatic via `ScrollBarKind::Floating`,
    // so every preset maps to `VisibleWhenNeeded` for normal operation.
    // Solid/Thin users who want the bar to always show regardless of
    // overflow can pick `AlwaysVisible` from the visibility row below.
    let v = ScrollBarVisibility::VisibleWhenNeeded;
    visibility.set(v);
    agg_gui::set_scroll_visibility(v);
}

// ── Public builder ───────────────────────────────────────────────────────────

pub fn build(font: Arc<Font>) -> Box<dyn Widget> {
    let cells      = StyleCells::build_from(current_scroll_style());
    let visibility = Rc::new(Cell::new(ScrollBarVisibility::VisibleWhenNeeded));
    let content_len = Rc::new(Cell::new(2_usize));

    // Closure used by every control to rebuild and publish the global style.
    let cells_rc = Rc::new(cells);
    let apply: Rc<dyn Fn()> = {
        let cells = Rc::clone(&cells_rc);
        Rc::new(move || set_scroll_style(cells.compose()))
    };

    // ── Presets row ─────────────────────────────────────────────────────────
    // Three buttons acting as "momentary" — clicking replaces the global style.
    // We use a SegRow bound to a cell, with an `on_change` hook that resets
    // the cell so the highlight is purely visual for the last-clicked preset.
    let preset_cell: Rc<Cell<Preset>> = Rc::new(Cell::new(Preset::Floating));
    let preset_seg = {
        let cells_for_cb = Rc::clone(&cells_rc);
        let vis_for_cb   = Rc::clone(&visibility);
        let cur = Rc::clone(&preset_cell);
        SegRow::new(
            Arc::clone(&font),
            vec![
                ("Solid",    Preset::Solid),
                ("Thin",     Preset::Thin),
                ("Floating", Preset::Floating),
            ],
            Rc::clone(&preset_cell),
        ).on_change(move || apply_preset(&cells_for_cb, cur.get(), &vis_for_cb))
    };
    let presets_row: Box<dyn Widget> = Box::new(FlexRow::new().with_gap(10.0)
        .add(label(Arc::clone(&font), "Presets:", 12.0))
        .add(Box::new(preset_seg)));

    // ── Details body ────────────────────────────────────────────────────────
    let mut details = FlexColumn::new().with_gap(4.0).with_padding(4.0);

    // Row 1: Type
    {
        let cells_cb = Rc::clone(&cells_rc);
        let apply_cb = Rc::clone(&apply);
        let kind_cell = Rc::clone(&cells_rc.kind);
        let seg = SegRow::new(
            Arc::clone(&font),
            vec![("Solid", ScrollBarKind::Solid), ("Floating", ScrollBarKind::Floating)],
            Rc::clone(&kind_cell),
        ).on_change(move || {
            cells_cb.kind.set(kind_cell.get());
            apply_cb();
        });
        details.push(Box::new(FlexRow::new().with_gap(10.0)
            .add(label(Arc::clone(&font), "Type:", 12.0))
            .add(Box::new(seg))), 0.0);
    }

    // Row 2: Content margin: [ ] same  [N]
    {
        let ms = Rc::clone(&cells_rc.margin_same);
        let apply_ms = Rc::clone(&apply);
        // Wrap the checkbox in a zero-size watcher so a click re-applies.
        // (Checkbox doesn't expose an on_change callback, so we layout-poll.)
        let chk = Checkbox::new("same", Arc::clone(&font), ms.get())
            .with_font_size(12.0)
            .with_state_cell(Rc::clone(&ms));
        let cm = Rc::clone(&cells_rc.content_margin);
        let cm_apply = Rc::clone(&apply);
        let dv = DragValue::new(cm.get(), 0.0, 50.0, Arc::clone(&font))
            .with_font_size(12.0)
            .with_decimals(0)
            .on_change(move |v| { cm.set(v); cm_apply(); });
        details.push(Box::new(FlexRow::new().with_gap(10.0)
            .add(label(Arc::clone(&font), "Content margin:", 12.0))
            .add(Box::new(chk))
            .add(Box::new(SizedBox::new().with_width(CTRL_W).with_child(Box::new(dv))))
            .add(Box::new(WatchCell::new(Rc::clone(&ms), Rc::clone(&apply_ms))))
        ), 0.0);
    }

    // Rows 3..5: Full bar width / Min handle length / Outer margin
    details.push(drag_row(Arc::clone(&font), Rc::clone(&cells_rc.bar_width),
        0.0, 50.0, 1.0, 0, "Full bar width", Rc::clone(&apply)), 0.0);
    details.push(drag_row(Arc::clone(&font), Rc::clone(&cells_rc.handle_min),
        0.0, 80.0, 1.0, 0, "Minimum handle length", Rc::clone(&apply)), 0.0);
    details.push(drag_row(Arc::clone(&font), Rc::clone(&cells_rc.outer_margin),
        0.0, 40.0, 1.0, 0, "Outer margin", Rc::clone(&apply)), 0.0);

    // Row 6: Color
    {
        let cells_cb = Rc::clone(&cells_rc);
        let apply_cb = Rc::clone(&apply);
        let color_cell = Rc::clone(&cells_rc.color);
        let seg = SegRow::new(
            Arc::clone(&font),
            vec![
                ("Background", ScrollBarColor::Background),
                ("Foreground", ScrollBarColor::Foreground),
            ],
            Rc::clone(&color_cell),
        ).on_change(move || {
            cells_cb.color.set(color_cell.get());
            apply_cb();
        });
        details.push(Box::new(FlexRow::new().with_gap(10.0)
            .add(label(Arc::clone(&font), "Color:", 12.0))
            .add(Box::new(seg))), 0.0);
    }

    // Row 7: Inner margin
    details.push(drag_row(Arc::clone(&font), Rc::clone(&cells_rc.inner_margin),
        0.0, 40.0, 1.0, 0, "Inner margin", Rc::clone(&apply)), 0.0);

    details.push(Box::new(Separator::horizontal()), 0.0);

    // Rows 8–9: Fade strength / Fade size
    details.push(drag_row(Arc::clone(&font), Rc::clone(&cells_rc.fade_strength),
        0.0, 1.0, 0.05, 2, "Fade strength", Rc::clone(&apply)), 0.0);
    details.push(drag_row(Arc::clone(&font), Rc::clone(&cells_rc.fade_size),
        0.0, 200.0, 1.0, 0, "Fade size", Rc::clone(&apply)), 0.0);

    // ── Outer tab layout ────────────────────────────────────────────────────
    let mut col = FlexColumn::new().with_gap(6.0).with_padding(10.0);
    col.push(presets_row, 0.0);
    col.push(Box::new(
        CollapsingHeader::new("Details", Arc::clone(&font))
            .default_open(false)
            .with_content(Box::new(details))
    ), 0.0);

    // ScrollBarVisibility — drives the global visibility so every scroll
    // bar in the app (sidebar, other demos, etc) follows the same policy.
    {
        let vis_cell = Rc::clone(&visibility);
        let seg = SegRow::new(
            Arc::clone(&font),
            vec![
                ("AlwaysHidden",       ScrollBarVisibility::AlwaysHidden),
                ("VisibleWhenNeeded",  ScrollBarVisibility::VisibleWhenNeeded),
                ("AlwaysVisible",      ScrollBarVisibility::AlwaysVisible),
            ],
            Rc::clone(&vis_cell),
        ).on_change(move || agg_gui::set_scroll_visibility(vis_cell.get()));
        col.push(Box::new(FlexRow::new().with_gap(10.0)
            .add(label(Arc::clone(&font), "ScrollBarVisibility:", 12.0))
            .add(Box::new(seg))), 0.0);
    }

    col.push(wrapped_label(Arc::clone(&font),
        "When to show scroll bars; resize the window to see the effect.", 11.0), 0.0);

    col.push(Box::new(Separator::horizontal()), 0.0);

    // Content length slider with numeric readout to its right.
    {
        let cl_slider = Rc::clone(&content_len);
        let cl_label  = Rc::clone(&content_len);
        col.push(Box::new(FlexRow::new().with_gap(10.0)
            .add(label(Arc::clone(&font), "Content length", 12.0))
            .add_flex(Box::new(
                Slider::new(2.0, 1.0, 100.0, Arc::clone(&font))
                    .with_step(1.0)
                    .on_change(move |v| cl_slider.set(v.round() as usize))
            ), 1.0)
            .add(Box::new(SizedBox::new().with_width(8.0)))
            .add(Box::new(LiveLabel::new(
                Arc::clone(&font),
                Rc::new(move || format!("{}", cl_label.get())),
            ).with_font_size(12.0)))), 0.0);
    }

    col.push(Box::new(Separator::horizontal()), 0.0);

    // Demo ScrollView — uses the global style AND global visibility.
    let content = LoremStack::new(Arc::clone(&font), Rc::clone(&content_len));
    let scroll = ScrollView::new(Box::new(content));
    col.push(Box::new(scroll), 1.0);

    Box::new(col)
}

// ── WatchCell ─ a zero-size widget that fires a callback when an observed
//                 `Rc<Cell<bool>>` changes value between layouts. ─────────────

struct WatchCell {
    bounds:   Rect,
    children: Vec<Box<dyn Widget>>,
    obs:      Rc<Cell<bool>>,
    last:     Cell<bool>,
    cb:       Rc<dyn Fn()>,
}
impl WatchCell {
    fn new(obs: Rc<Cell<bool>>, cb: Rc<dyn Fn()>) -> Self {
        let v = obs.get();
        Self {
            bounds: Rect::default(), children: Vec::new(),
            obs, last: Cell::new(v), cb,
        }
    }
}
impl Widget for WatchCell {
    fn type_name(&self) -> &'static str { "WatchCell" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }
    fn show_in_inspector(&self) -> bool { false }
    fn layout(&mut self, _: Size) -> Size {
        let cur = self.obs.get();
        if cur != self.last.get() {
            self.last.set(cur);
            (self.cb)();
        }
        Size::ZERO
    }
    fn paint(&mut self, _: &mut dyn agg_gui::DrawCtx) {}
    fn on_event(&mut self, _: &agg_gui::Event) -> agg_gui::EventResult {
        agg_gui::EventResult::Ignored
    }
}
