//! `RadioGroup` — a set of mutually exclusive radio buttons.
//!
//! Each option label is rendered through a backbuffered [`Label`] child,
//! so glyph rasterization is cached and only repeated when text or color changes.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult, Key, MouseButton};
use crate::geometry::{Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::text::{measure_advance, Font};
use crate::widget::Widget;
use crate::widgets::label::Label;

const DOT_R: f64 = 7.0; // outer circle radius
const GAP: f64 = 8.0;
const ROW_H: f64 = 22.0;
/// Horizontal spacing between wrapped items in horizontal-wrap mode.
const HWRAP_SPACING: f64 = 6.0;
/// Left/right slack reserved so the circle's 1.5-px stroke (and its AA
/// fringe) and the focus-ring outline stay INSIDE the widget's bounds.
/// Without it, the parent container's `clip_children_rect` (which
/// defaults to the widget's bounds rect) chops the leftmost stroke
/// pixel off whenever the RadioGroup is placed flush against a
/// container edge — see `paint::paint_subtree_direct_inner`.
const LEFT_INSET: f64 = 2.0;

/// A group of mutually-exclusive radio options.
///
/// Each option is a `(label, value_string)` pair. `selected` is the index of
/// the currently chosen option.  Each option's text is held as a real
/// `Label` child in `children` so the inspector tree mirrors the visible
/// row structure (RadioGroup → Label × N) and the framework recurses
/// into the labels naturally — RadioGroup's `paint()` only draws the
/// dot circles.
pub struct RadioGroup {
    bounds: Rect,
    /// One `Label` child per option, stored as `Box<dyn Widget>` so the
    /// framework's tree walks (paint / hit-test / inspector) recurse into
    /// them.  Mutated through `set_label_color` (Widget trait method) to
    /// retint per frame without rebuilding.
    children: Vec<Box<dyn Widget>>,
    base: WidgetBase,
    options: Vec<String>,
    selected: usize,
    hovered: Option<usize>,
    focused: bool,
    font: Arc<Font>,
    font_size: f64,
    on_change: Option<Box<dyn FnMut(usize)>>,
    /// Optional external mirror of `selected` — same bidirectional-binding
    /// pattern as `Slider::with_value_cell` / `ToggleSwitch::with_state_cell`.
    selected_cell: Option<Rc<Cell<usize>>>,
    /// When `true`, options flow left-to-right and wrap to new lines instead
    /// of stacking vertically. Backs egui's "64 radio buttons" wrapped row.
    horizontal: bool,
    /// Per-option layout computed during a horizontal-wrap `layout` pass:
    /// `(dot_cx, dot_cy, label_x, has_label, hit_box)` in widget-local
    /// coordinates (x-left / y-up). Empty in vertical mode, where
    /// `row_center_y`/`row_for_y` are used instead.
    hwrap_items: Vec<(f64, f64, f64, bool, Rect)>,
}

impl RadioGroup {
    pub fn new(options: Vec<impl Into<String>>, selected: usize, font: Arc<Font>) -> Self {
        let font_size = 14.0;
        let opts: Vec<String> = options.into_iter().map(|s| s.into()).collect();
        let children: Vec<Box<dyn Widget>> = opts
            .iter()
            .map(|text| {
                Box::new(Label::new(text.as_str(), Arc::clone(&font)).with_font_size(font_size))
                    as Box<dyn Widget>
            })
            .collect();
        Self {
            bounds: Rect::default(),
            children,
            base: WidgetBase::new(),
            options: opts,
            selected,
            hovered: None,
            focused: false,
            font,
            font_size,
            on_change: None,
            selected_cell: None,
            horizontal: false,
            hwrap_items: Vec::new(),
        }
    }

    /// Lay the options out left-to-right, wrapping to new lines when they run
    /// out of horizontal room (egui's `horizontal_wrapped` radio row). Options
    /// with empty labels render as bare dots, which is how egui's 64-radio demo
    /// is built.
    pub fn with_horizontal_wrap(mut self, on: bool) -> Self {
        self.horizontal = on;
        self
    }

    /// Bind this group's selection to an external `Rc<Cell<usize>>`.  The
    /// cell is read each layout and written on every selection change, so
    /// two RadioGroups sharing one cell stay in lock-step.
    pub fn with_selected_cell(mut self, cell: Rc<Cell<usize>>) -> Self {
        let n = self.options.len();
        let v = cell.get();
        if n > 0 {
            self.selected = v.min(n - 1);
        }
        self.selected_cell = Some(cell);
        self
    }

    pub fn with_font_size(mut self, size: f64) -> Self {
        self.font_size = size;
        // Rebuild label children with new font size.
        self.children = self
            .options
            .iter()
            .map(|text| {
                Box::new(Label::new(text.as_str(), Arc::clone(&self.font)).with_font_size(size))
                    as Box<dyn Widget>
            })
            .collect();
        self
    }

    pub fn with_margin(mut self, m: Insets) -> Self {
        self.base.margin = m;
        self
    }
    pub fn with_h_anchor(mut self, h: HAnchor) -> Self {
        self.base.h_anchor = h;
        self
    }
    pub fn with_v_anchor(mut self, v: VAnchor) -> Self {
        self.base.v_anchor = v;
        self
    }
    pub fn with_min_size(mut self, s: Size) -> Self {
        self.base.min_size = s;
        self
    }
    pub fn with_max_size(mut self, s: Size) -> Self {
        self.base.max_size = s;
        self
    }

    pub fn on_change(mut self, cb: impl FnMut(usize) + 'static) -> Self {
        self.on_change = Some(Box::new(cb));
        self
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn set_selected(&mut self, idx: usize) {
        if idx < self.options.len() {
            self.selected = idx;
            if let Some(cell) = &self.selected_cell {
                cell.set(idx);
            }
        }
    }

    fn fire(&mut self) {
        let idx = self.selected;
        if let Some(cell) = &self.selected_cell {
            cell.set(idx);
        }
        if let Some(cb) = self.on_change.as_mut() {
            cb(idx);
        }
    }

    /// Y coordinate (bottom-left) of the center of row `i` in Y-up space.
    fn row_center_y(&self, i: usize, total_h: f64) -> f64 {
        let n = self.options.len();
        if n == 0 {
            return total_h * 0.5;
        }
        // rows are stacked top-to-bottom, so row 0 is at the top.
        // In Y-up, top row has the largest Y.
        let row_top_y = total_h - (i as f64) * ROW_H;
        row_top_y - ROW_H * 0.5
    }

    fn row_for_y(&self, pos_y: f64) -> Option<usize> {
        let h = self.bounds.height;
        for i in 0..self.options.len() {
            let cy = self.row_center_y(i, h);
            if pos_y >= cy - ROW_H * 0.5 && pos_y < cy + ROW_H * 0.5 {
                return Some(i);
            }
        }
        None
    }

    /// Compute horizontal-wrap geometry for the given available width.
    ///
    /// Returns one `(dot_cx, dot_cy, label_x, has_label, hit_box)` per option
    /// plus the total height. Coordinates are widget-local (x grows right,
    /// y grows up, so the first wrapped line sits at the top). Pure w.r.t.
    /// widget state — used by both `layout` and `measure_min_height`.
    fn compute_hwrap(&self, available_w: f64) -> (Vec<(f64, f64, f64, bool, Rect)>, f64) {
        let n = self.options.len();
        if n == 0 {
            return (Vec::new(), 0.0);
        }
        let dot_extent = LEFT_INSET + DOT_R * 2.0;
        // First pass (top-down rows): assign each option a row and x offset.
        let mut placed: Vec<(usize, f64, f64, f64, bool)> = Vec::with_capacity(n);
        let mut x = 0.0_f64;
        let mut row = 0usize;
        for opt in &self.options {
            let has_label = !opt.is_empty();
            let label_w = if has_label {
                measure_advance(&self.font, opt, self.font_size)
            } else {
                0.0
            };
            let item_w = dot_extent + if has_label { GAP + label_w } else { 0.0 };
            if x > 0.0 && x + item_w > available_w {
                row += 1;
                x = 0.0;
            }
            placed.push((row, x, item_w, label_w, has_label));
            x += item_w + HWRAP_SPACING;
        }
        let rows = row + 1;
        let h = rows as f64 * ROW_H;
        // Second pass: convert row index to Y-up centre and build hit boxes.
        let items = placed
            .into_iter()
            .map(|(r, x_left, item_w, _label_w, has_label)| {
                let cy = h - (r as f64) * ROW_H - ROW_H * 0.5;
                let dot_cx = LEFT_INSET + DOT_R + x_left;
                let label_x = x_left + dot_extent + GAP;
                let hit = Rect::new(x_left, cy - ROW_H * 0.5, item_w, ROW_H);
                (dot_cx, cy, label_x, has_label, hit)
            })
            .collect();
        (items, h)
    }

    /// Locate the option whose clickable box contains `pos` (horizontal mode).
    fn hwrap_item_at(&self, pos_x: f64, pos_y: f64) -> Option<usize> {
        self.hwrap_items.iter().position(|(_, _, _, _, hit)| {
            pos_x >= hit.x
                && pos_x < hit.x + hit.width
                && pos_y >= hit.y
                && pos_y < hit.y + hit.height
        })
    }

    /// Horizontal-wrap layout pass: caches item geometry, positions the label
    /// children beside their dots, and sizes the widget to the wrapped height.
    fn layout_horizontal(&mut self, available: Size) -> Size {
        let (items, h) = self.compute_hwrap(available.width);
        self.bounds = Rect::new(0.0, 0.0, available.width, h);
        for (i, child) in self.children.iter_mut().enumerate() {
            let Some(&(_dot_cx, cy, label_x, has_label, _hit)) = items.get(i) else {
                continue;
            };
            if !has_label {
                // Bare dot: give the empty label a zero-width slot at the dot.
                child.set_bounds(Rect::new(label_x, cy, 0.0, 0.0));
                let _ = child.layout(Size::new(0.0, ROW_H));
                continue;
            }
            let s = child.layout(Size::new((available.width - label_x).max(0.0), ROW_H));
            let ly = cy - s.height * 0.5;
            child.set_bounds(Rect::new(label_x, ly, s.width, s.height));
        }
        self.hwrap_items = items;
        Size::new(available.width, h)
    }
}

impl Widget for RadioGroup {
    fn type_name(&self) -> &'static str {
        "RadioGroup"
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

    fn is_focusable(&self) -> bool {
        true
    }

    fn margin(&self) -> Insets {
        self.base.margin
    }
    fn widget_base(&self) -> Option<&WidgetBase> {
        Some(&self.base)
    }
    fn widget_base_mut(&mut self) -> Option<&mut WidgetBase> {
        Some(&mut self.base)
    }
    fn h_anchor(&self) -> HAnchor {
        self.base.h_anchor
    }
    fn v_anchor(&self) -> VAnchor {
        self.base.v_anchor
    }
    fn min_size(&self) -> Size {
        self.base.min_size
    }
    fn max_size(&self) -> Size {
        self.base.max_size
    }

    /// One [`ROW_H`]-tall row per option — matches the height [`layout`]
    /// produces.  Without this override the trait default returns `0`, and
    /// an ancestor `Window::with_tight_content_fit` would size the window
    /// too short by the radio's full height.
    fn measure_min_height(&self, available_w: f64) -> f64 {
        if self.horizontal {
            let (_, h) = self.compute_hwrap(available_w);
            return h;
        }
        self.options.len() as f64 * ROW_H
    }

    fn layout(&mut self, available: Size) -> Size {
        // Pick up external-cell writes every frame (e.g. the System
        // window's typeface radio driving this demo's radio).
        if let Some(cell) = &self.selected_cell {
            let n = self.options.len();
            if n > 0 {
                let v = cell.get().min(n - 1);
                self.selected = v;
            }
        }
        if self.horizontal {
            return self.layout_horizontal(available);
        }
        let h = self.options.len() as f64 * ROW_H;
        self.bounds = Rect::new(0.0, 0.0, available.width, h);
        // `LEFT_INSET` shifts the circle inward; the label moves the
        // same amount so the visual gap between dot and label is preserved.
        let circle_extent = LEFT_INSET + DOT_R * 2.0;
        let label_avail_w = (available.width - circle_extent - GAP).max(0.0);
        let lx = circle_extent + GAP;
        for (i, child) in self.children.iter_mut().enumerate() {
            let s = child.layout(Size::new(label_avail_w, ROW_H));
            // Position the label child in the row's vertical centre,
            // offset right of the radio dot.  In Y-up the first row
            // (i=0) sits at the TOP of the widget — see `row_center_y`.
            let row_top_y = h - (i as f64) * ROW_H;
            let cy = row_top_y - ROW_H * 0.5;
            let ly = cy - s.height * 0.5;
            child.set_bounds(Rect::new(lx, ly, s.width, s.height));
        }
        Size::new(available.width, h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let h = self.bounds.height;

        // Focus outline around whole widget — drawn JUST INSIDE bounds so
        // the parent's clip_children_rect (defaults to widget bounds)
        // doesn't chop the leftmost stroke pixel.
        if self.focused {
            ctx.set_stroke_color(v.accent_focus);
            ctx.set_line_width(1.5);
            ctx.begin_path();
            ctx.rounded_rect(0.75, 0.75, self.bounds.width - 1.5, h - 1.5, 4.0);
            ctx.stroke();
        }

        // Paint just the radio dot for each row — the row's text is a
        // real `Label` child that the framework recurses into after this
        // method returns (positioned by `layout`).  Setting label colour
        // here through the `set_label_color` Widget-trait method keeps
        // the foreground theme-aware without rebuilding the Label.
        let text_color = v.text_color;
        for i in 0..self.options.len() {
            // Dot centre differs by layout mode: vertical rows use the fixed
            // left column; horizontal-wrap uses the cached per-item centre.
            let (dot_cx, cy) = if self.horizontal {
                match self.hwrap_items.get(i) {
                    Some(&(cx, cy, _, _, _)) => (cx, cy),
                    None => continue,
                }
            } else {
                (LEFT_INSET + DOT_R, self.row_center_y(i, h))
            };
            let checked = i == self.selected;
            let hovered = self.hovered == Some(i);

            let border = if checked {
                v.accent
            } else if hovered {
                v.widget_bg_hovered
            } else {
                v.widget_stroke
            };
            let bg = if checked { v.accent } else { v.widget_bg };

            ctx.set_fill_color(bg);
            ctx.begin_path();
            ctx.circle(dot_cx, cy, DOT_R);
            ctx.fill();

            ctx.set_stroke_color(border);
            ctx.set_line_width(1.5);
            ctx.begin_path();
            ctx.circle(dot_cx, cy, DOT_R);
            ctx.stroke();

            // Inner dot when checked — always widget_bg so it stays
            // readable on the accent surface.
            if checked {
                ctx.set_fill_color(v.widget_bg);
                ctx.begin_path();
                ctx.circle(dot_cx, cy, DOT_R * 0.45);
                ctx.fill();
            }

            if let Some(child) = self.children.get_mut(i) {
                child.set_label_color(text_color);
            }
        }
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseMove { pos } => {
                let was = self.hovered;
                self.hovered = if self.horizontal {
                    self.hwrap_item_at(pos.x, pos.y)
                } else {
                    self.row_for_y(pos.y)
                };
                if was != self.hovered {
                    crate::animation::request_draw();
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }
            Event::MouseDown {
                button: MouseButton::Left,
                pos,
                ..
            } => {
                let hit = if self.horizontal {
                    self.hwrap_item_at(pos.x, pos.y)
                } else {
                    self.row_for_y(pos.y)
                };
                if let Some(i) = hit {
                    let was = self.selected;
                    self.selected = i;
                    self.fire();
                    if was != i {
                        crate::animation::request_draw();
                    }
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }
            Event::KeyDown { key, .. } => {
                let n = self.options.len();
                let changed = match key {
                    Key::ArrowUp | Key::ArrowLeft => {
                        if self.selected > 0 {
                            self.selected -= 1;
                            true
                        } else {
                            false
                        }
                    }
                    Key::ArrowDown | Key::ArrowRight => {
                        if self.selected + 1 < n {
                            self.selected += 1;
                            true
                        } else {
                            false
                        }
                    }
                    _ => false,
                };
                if changed {
                    self.fire();
                    crate::animation::request_draw();
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            Event::FocusGained => {
                let was = self.focused;
                self.focused = true;
                if !was {
                    crate::animation::request_draw();
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            Event::FocusLost => {
                let was = self.focused;
                self.focused = false;
                if was {
                    crate::animation::request_draw();
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            _ => EventResult::Ignored,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Color;
    use crate::draw_ctx::{FillRule, GlPaint, LinearGradientPaint, RadialGradientPaint};
    use crate::event::Modifiers;
    use crate::geometry::Point;
    use crate::text::TextMetrics;
    use crate::theme::{current_visuals, set_visuals, Visuals};
    use agg_rust::comp_op::CompOp;
    use agg_rust::math_stroke::{LineCap, LineJoin};
    use agg_rust::trans_affine::TransAffine;

    const FONT_BYTES: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");

    fn test_font() -> Arc<Font> {
        Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
    }

    /// Records the fill colour of every filled *circle* so a test can assert
    /// how many radio dots are painted with the accent (selected) surface.
    /// A circle is "filled" when `fill()` is called while the most recent
    /// path primitive was `circle()`; we snapshot the current fill colour at
    /// that moment. RadioGroup draws exactly one accent-filled outer circle —
    /// the selected dot — plus per-dot strokes and a `widget_bg` inner dot.
    struct CircleFillRecorder {
        transform: TransAffine,
        stack: Vec<TransAffine>,
        fill_color: Color,
        last_was_circle: bool,
        filled_circles: Vec<Color>,
    }

    impl CircleFillRecorder {
        fn new() -> Self {
            Self {
                transform: TransAffine::new(),
                stack: Vec::new(),
                fill_color: Color::rgba(0.0, 0.0, 0.0, 0.0),
                last_was_circle: false,
                filled_circles: Vec::new(),
            }
        }
    }

    impl DrawCtx for CircleFillRecorder {
        fn set_fill_color(&mut self, color: Color) {
            self.fill_color = color;
        }
        fn set_stroke_color(&mut self, _color: Color) {}
        fn set_fill_linear_gradient(&mut self, _gradient: LinearGradientPaint) {}
        fn set_fill_radial_gradient(&mut self, _gradient: RadialGradientPaint) {}
        fn set_line_width(&mut self, _w: f64) {}
        fn set_line_join(&mut self, _join: LineJoin) {}
        fn set_line_cap(&mut self, _cap: LineCap) {}
        fn set_miter_limit(&mut self, _limit: f64) {}
        fn set_line_dash(&mut self, _dashes: &[f64], _offset: f64) {}
        fn set_blend_mode(&mut self, _mode: CompOp) {}
        fn set_global_alpha(&mut self, _alpha: f64) {}
        fn set_fill_rule(&mut self, _rule: FillRule) {}
        fn set_font(&mut self, _font: Arc<Font>) {}
        fn set_font_size(&mut self, _size: f64) {}
        fn clip_rect(&mut self, _x: f64, _y: f64, _w: f64, _h: f64) {}
        fn reset_clip(&mut self) {}
        fn clear(&mut self, _color: Color) {}
        fn begin_path(&mut self) {
            self.last_was_circle = false;
        }
        fn move_to(&mut self, _x: f64, _y: f64) {
            self.last_was_circle = false;
        }
        fn line_to(&mut self, _x: f64, _y: f64) {
            self.last_was_circle = false;
        }
        fn cubic_to(&mut self, _cx1: f64, _cy1: f64, _cx2: f64, _cy2: f64, _x: f64, _y: f64) {
            self.last_was_circle = false;
        }
        fn quad_to(&mut self, _cx: f64, _cy: f64, _x: f64, _y: f64) {
            self.last_was_circle = false;
        }
        fn arc_to(&mut self, _cx: f64, _cy: f64, _r: f64, _s: f64, _e: f64, _ccw: bool) {
            self.last_was_circle = false;
        }
        fn circle(&mut self, _cx: f64, _cy: f64, _r: f64) {
            self.last_was_circle = true;
        }
        fn rect(&mut self, _x: f64, _y: f64, _w: f64, _h: f64) {
            self.last_was_circle = false;
        }
        fn rounded_rect(&mut self, _x: f64, _y: f64, _w: f64, _h: f64, _r: f64) {
            self.last_was_circle = false;
        }
        fn close_path(&mut self) {}
        fn fill(&mut self) {
            if self.last_was_circle {
                self.filled_circles.push(self.fill_color);
            }
        }
        fn stroke(&mut self) {}
        fn fill_and_stroke(&mut self) {}
        fn draw_triangles_aa(&mut self, _vertices: &[[f32; 3]], _indices: &[u32], _color: Color) {}
        fn fill_text(&mut self, _text: &str, _x: f64, _y: f64) {}
        fn fill_text_gsv(&mut self, _text: &str, _x: f64, _y: f64, _size: f64) {}
        fn measure_text(&self, _text: &str) -> Option<TextMetrics> {
            Some(TextMetrics {
                width: 40.0,
                ascent: 10.0,
                descent: 3.0,
                line_height: 16.0,
            })
        }
        fn transform(&self) -> TransAffine {
            self.transform
        }
        fn save(&mut self) {
            self.stack.push(self.transform);
        }
        fn restore(&mut self) {
            if let Some(t) = self.stack.pop() {
                self.transform = t;
            }
        }
        fn translate(&mut self, tx: f64, ty: f64) {
            self.transform
                .premultiply(&TransAffine::new_translation(tx, ty));
        }
        fn rotate(&mut self, radians: f64) {
            self.transform
                .premultiply(&TransAffine::new_rotation(radians));
        }
        fn scale(&mut self, sx: f64, sy: f64) {
            self.transform.premultiply(&TransAffine::new_scaling(sx, sy));
        }
        fn set_transform(&mut self, m: TransAffine) {
            self.transform = m;
        }
        fn reset_transform(&mut self) {
            self.transform = TransAffine::new();
        }
        fn gl_paint(&mut self, _screen_rect: Rect, _painter: &mut dyn GlPaint) {}
    }

    /// Paint the group and count how many outer dots are filled with the
    /// theme accent (i.e. painted as "selected").
    fn accent_dot_count(g: &mut RadioGroup) -> usize {
        let v = current_visuals();
        let mut ctx = CircleFillRecorder::new();
        g.paint(&mut ctx);
        ctx.filled_circles
            .iter()
            .filter(|c| **c == v.accent)
            .count()
    }

    /// Regression: exactly ONE radio dot may be painted with the accent
    /// (selected) surface — the currently-selected option. This pins the
    /// "every option looks selected" report: neither hover state nor the
    /// horizontal-wrap layout may leak the accent fill onto other dots.
    #[test]
    fn exactly_one_dot_painted_selected() {
        set_visuals(Visuals::dark());
        for sel in 0..3 {
            let mut g = RadioGroup::new(vec!["First", "Second", "Third"], sel, test_font());
            g.layout(Size::new(200.0, 0.0));
            assert_eq!(
                accent_dot_count(&mut g),
                1,
                "vertical group with selected={sel} must paint exactly one accent dot"
            );
        }
    }

    /// The same invariant must hold in horizontal-wrap mode, and hovering a
    /// *different* option must not add a second accent dot.
    #[test]
    fn hover_does_not_add_a_second_selected_dot() {
        set_visuals(Visuals::dark());
        let mut g = RadioGroup::new(vec!["First", "Second", "Third"], 0, test_font())
            .with_horizontal_wrap(true);
        g.layout(Size::new(400.0, 0.0));
        // Hover the second item's centre.
        let (cx, cy, _, _, _) = g.hwrap_items[1];
        let _ = g.on_event(&Event::MouseMove {
            pos: Point::new(cx, cy),
        });
        assert_eq!(
            accent_dot_count(&mut g),
            1,
            "hovering an unselected option must not paint it as selected"
        );
    }

    fn empty_group(n: usize) -> RadioGroup {
        let opts: Vec<String> = (0..n).map(|_| String::new()).collect();
        RadioGroup::new(opts, 0, test_font()).with_horizontal_wrap(true)
    }

    #[test]
    fn empty_labels_wrap_onto_multiple_rows() {
        let mut g = empty_group(64);
        // Narrow width forces many rows; a single dot is ~16px + spacing.
        let size = g.layout(Size::new(80.0, 0.0));
        assert!(size.height > ROW_H, "64 dots at 80px wide must wrap");
        assert_eq!(g.hwrap_items.len(), 64);
        // Row count is height / ROW_H; must be > 1 for a wrapped layout.
        let rows = (size.height / ROW_H).round() as usize;
        assert!(rows > 1, "expected multiple rows, got {rows}");
    }

    #[test]
    fn single_wide_row_does_not_wrap() {
        let mut g = empty_group(4);
        let size = g.layout(Size::new(2000.0, 0.0));
        assert_eq!(
            (size.height / ROW_H).round() as usize,
            1,
            "4 dots in 2000px must stay on one row"
        );
    }

    #[test]
    fn click_selects_the_hit_dot() {
        let mut g = empty_group(8);
        g.layout(Size::new(2000.0, 0.0)); // one row, all dots side by side
        // Aim at the 3rd dot's centre.
        let (cx, cy, _, _, _) = g.hwrap_items[2];
        let down = Event::MouseDown {
            pos: Point::new(cx, cy),
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        };
        let r = g.on_event(&down);
        assert_eq!(r, EventResult::Consumed);
        assert_eq!(g.selected(), 2);
    }

    #[test]
    fn vertical_mode_is_unaffected() {
        let mut g = RadioGroup::new(vec!["a", "b", "c"], 0, test_font());
        let size = g.layout(Size::new(200.0, 0.0));
        assert_eq!(size.height, 3.0 * ROW_H);
        assert!(g.hwrap_items.is_empty());
    }
}
