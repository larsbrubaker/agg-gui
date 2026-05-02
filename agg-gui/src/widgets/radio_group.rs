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
use crate::text::Font;
use crate::widget::Widget;
use crate::widgets::label::Label;

const DOT_R: f64 = 7.0; // outer circle radius
const GAP: f64 = 8.0;
const ROW_H: f64 = 22.0;
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
}

impl RadioGroup {
    pub fn new(options: Vec<impl Into<String>>, selected: usize, font: Arc<Font>) -> Self {
        let font_size = 14.0;
        let opts: Vec<String> = options.into_iter().map(|s| s.into()).collect();
        let children: Vec<Box<dyn Widget>> = opts
            .iter()
            .map(|text| {
                Box::new(
                    Label::new(text.as_str(), Arc::clone(&font)).with_font_size(font_size),
                ) as Box<dyn Widget>
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
        }
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
                Box::new(
                    Label::new(text.as_str(), Arc::clone(&self.font))
                        .with_font_size(size),
                ) as Box<dyn Widget>
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
            let cy = self.row_center_y(i, h);
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
            ctx.circle(LEFT_INSET + DOT_R, cy, DOT_R);
            ctx.fill();

            ctx.set_stroke_color(border);
            ctx.set_line_width(1.5);
            ctx.begin_path();
            ctx.circle(LEFT_INSET + DOT_R, cy, DOT_R);
            ctx.stroke();

            // Inner dot when checked — always widget_bg so it stays
            // readable on the accent surface.
            if checked {
                ctx.set_fill_color(v.widget_bg);
                ctx.begin_path();
                ctx.circle(LEFT_INSET + DOT_R, cy, DOT_R * 0.45);
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
                self.hovered = self.row_for_y(pos.y);
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
                if let Some(i) = self.row_for_y(pos.y) {
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
