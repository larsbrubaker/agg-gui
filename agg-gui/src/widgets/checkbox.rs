//! `Checkbox` — a boolean toggle with a label.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use crate::color::Color;
use crate::event::{Event, EventResult, Key, MouseButton};
use crate::geometry::{Rect, Size};
use crate::draw_ctx::DrawCtx;
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::text::Font;
use crate::widget::Widget;

const BOX_SIZE: f64 = 16.0;
const GAP: f64 = 8.0;

/// A boolean toggle with a square box and a text label.
pub struct Checkbox {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>, // always empty
    base: WidgetBase,
    label: String,
    font: Arc<Font>,
    font_size: f64,
    /// `None` → use `ctx.visuals().text_color` at paint time.
    label_color: Option<Color>,
    checked: bool,
    /// When set, this cell is the authoritative checked state.  `paint` reads
    /// from it and `toggle` writes to it so the checkbox stays in sync with
    /// external state changes (e.g. a window's close button setting it to false).
    state_cell: Option<Rc<Cell<bool>>>,
    hovered: bool,
    focused: bool,
    on_change: Option<Box<dyn FnMut(bool)>>,
}

impl Checkbox {
    pub fn new(label: impl Into<String>, font: Arc<Font>, checked: bool) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            base: WidgetBase::new(),
            label: label.into(),
            font,
            font_size: 14.0,
            label_color: None,
            checked,
            state_cell: None,
            hovered: false,
            focused: false,
            on_change: None,
        }
    }

    pub fn with_font_size(mut self, size: f64) -> Self { self.font_size = size; self }
    pub fn with_label_color(mut self, c: Color) -> Self { self.label_color = Some(c); self }

    /// Bind checked state to a shared cell.
    ///
    /// When set, `paint` reads from the cell (so external changes — e.g. a
    /// window's close button — are reflected immediately), and `toggle` writes
    /// to it so both directions stay in sync.
    pub fn with_state_cell(mut self, cell: Rc<Cell<bool>>) -> Self {
        self.state_cell = Some(cell);
        self
    }

    pub fn with_margin(mut self, m: Insets)    -> Self { self.base.margin   = m; self }
    pub fn with_h_anchor(mut self, h: HAnchor) -> Self { self.base.h_anchor = h; self }
    pub fn with_v_anchor(mut self, v: VAnchor) -> Self { self.base.v_anchor = v; self }
    pub fn with_min_size(mut self, s: Size)    -> Self { self.base.min_size = s; self }
    pub fn with_max_size(mut self, s: Size)    -> Self { self.base.max_size = s; self }

    pub fn on_change(mut self, cb: impl FnMut(bool) + 'static) -> Self {
        self.on_change = Some(Box::new(cb));
        self
    }

    pub fn checked(&self) -> bool { self.checked }
    pub fn set_checked(&mut self, v: bool) { self.checked = v; }

    fn toggle(&mut self) {
        let new_val = !self.effective_checked();
        self.checked = new_val;
        if let Some(ref cell) = self.state_cell { cell.set(new_val); }
        if let Some(cb) = self.on_change.as_mut() { cb(new_val); }
    }

    /// Returns the authoritative checked state: the cell value if bound, else
    /// the internal `checked` field.
    #[inline]
    fn effective_checked(&self) -> bool {
        if let Some(ref cell) = self.state_cell { cell.get() } else { self.checked }
    }
}

impl Widget for Checkbox {
    fn type_name(&self) -> &'static str { "Checkbox" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn is_focusable(&self) -> bool { true }

    fn margin(&self)   -> Insets  { self.base.margin }
    fn h_anchor(&self) -> HAnchor { self.base.h_anchor }
    fn v_anchor(&self) -> VAnchor { self.base.v_anchor }
    fn min_size(&self) -> Size    { self.base.min_size }
    fn max_size(&self) -> Size    { self.base.max_size }

    fn layout(&mut self, available: Size) -> Size {
        let h = BOX_SIZE.max(self.font_size * 1.5);
        Size::new(available.width, h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let h = self.bounds.height;
        let box_y = (h - BOX_SIZE) * 0.5;

        // Focus ring
        if self.focused {
            ctx.set_stroke_color(v.accent_focus);
            ctx.set_line_width(2.0);
            ctx.begin_path();
            ctx.rounded_rect(-1.5, box_y - 1.5, BOX_SIZE + 3.0, BOX_SIZE + 3.0, 4.0);
            ctx.stroke();
        }

        let checked = self.effective_checked();

        // Box background
        let bg = if checked {
            v.accent
        } else if self.hovered {
            v.widget_bg_hovered
        } else {
            v.widget_bg
        };
        ctx.set_fill_color(bg);
        ctx.begin_path();
        ctx.rounded_rect(0.0, box_y, BOX_SIZE, BOX_SIZE, 3.0);
        ctx.fill();

        // Box border
        let border = if checked { v.widget_stroke_active } else { v.widget_stroke };
        ctx.set_stroke_color(border);
        ctx.set_line_width(1.5);
        ctx.begin_path();
        ctx.rounded_rect(0.0, box_y, BOX_SIZE, BOX_SIZE, 3.0);
        ctx.stroke();

        // Checkmark — coordinates in Y-up space (origin = box bottom-left).
        // Fractions are (1 - Y-down-fraction) so the tick reads correctly.
        if checked {
            ctx.set_stroke_color(Color::white());
            ctx.set_line_width(2.0);
            ctx.begin_path();
            let bx = 0.0;
            let by = box_y;
            ctx.move_to(bx + 3.0,              by + BOX_SIZE * 0.55); // left, mid-high
            ctx.line_to(bx + BOX_SIZE * 0.42,  by + BOX_SIZE * 0.28); // bend at bottom
            ctx.line_to(bx + BOX_SIZE - 3.0,   by + BOX_SIZE * 0.75); // right, upper
            ctx.stroke();
        }

        // Label text
        let label_color = self.label_color.unwrap_or(v.text_color);
        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(self.font_size);
        ctx.set_fill_color(label_color);
        let tx = BOX_SIZE + GAP;
        if let Some(m) = ctx.measure_text(&self.label) {
            let ty = h * 0.5 - (m.ascent - m.descent) * 0.5 + m.descent;
            ctx.fill_text(&self.label, tx, ty);
        }
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseMove { pos } => {
                self.hovered = self.hit_test(*pos);
                EventResult::Ignored
            }
            Event::MouseDown { button: MouseButton::Left, .. } => {
                EventResult::Consumed
            }
            Event::MouseUp { button: MouseButton::Left, pos, .. } => {
                if self.hit_test(*pos) { self.toggle(); }
                EventResult::Consumed
            }
            Event::KeyDown { key: Key::Char(' '), .. } => {
                self.toggle();
                EventResult::Consumed
            }
            Event::FocusGained => { self.focused = true;  EventResult::Ignored }
            Event::FocusLost   => { self.focused = false; EventResult::Ignored }
            _ => EventResult::Ignored,
        }
    }
}
