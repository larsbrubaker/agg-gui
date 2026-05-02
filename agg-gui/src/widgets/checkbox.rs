//! `Checkbox` — a boolean toggle with a label.
//!
//! # Composition
//!
//! The checkbox label is rendered through a [`Label`] child with backbuffer
//! caching enabled (the default).  The box + checkmark are drawn directly via
//! path commands; only the text goes through the Label path.
//!
//! ```text
//! Checkbox (box + checkmark drawn via paths)
//!   └── Label (text, backbuffered)
//! ```

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult, Key, MouseButton};
use crate::geometry::{Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::text::Font;
use crate::widget::Widget;
use crate::widgets::label::Label;

const BOX_SIZE: f64 = 16.0;
const FOCUS_PAD: f64 = 2.0;
const GAP: f64 = 8.0;
const BOX_STROKE_WIDTH: f64 = 1.5;

/// Inspector-visible properties of a [`Checkbox`].  See [`SliderProps`] for
/// the rationale of the companion-props pattern.
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
#[derive(Clone, Debug, Default)]
pub struct CheckboxProps {
    pub checked: bool,
    pub font_size: f64,
    /// Explicit label colour override; `None` → follow active visuals.
    pub label_color: Option<Color>,
}

/// A boolean toggle with a square box and a text label.
pub struct Checkbox {
    bounds: Rect,
    /// `children[0]` is the [`Label`] that renders the text — composed as a
    /// real child so the framework's paint walk handles it.
    children: Vec<Box<dyn Widget>>,
    base: WidgetBase,
    font: Arc<Font>,
    pub props: CheckboxProps,
    /// When set, this cell is the authoritative checked state.  `paint` reads
    /// from it and `toggle` writes to it so the checkbox stays in sync with
    /// external state changes (e.g. a window's close button setting it to false).
    state_cell: Option<Rc<Cell<bool>>>,
    hovered: bool,
    focused: bool,
    on_change: Option<Box<dyn FnMut(bool)>>,
    /// Tracked label text — used for empty-check during layout and to rebuild
    /// the Label child when font size changes.
    label_text: String,
}

impl Checkbox {
    pub fn new(label: impl Into<String>, font: Arc<Font>, checked: bool) -> Self {
        let label_text: String = label.into();
        let font_size = 14.0;
        let label_widget = Label::new(&label_text, Arc::clone(&font)).with_font_size(font_size);
        Self {
            bounds: Rect::default(),
            children: vec![Box::new(label_widget)],
            base: WidgetBase::new(),
            font,
            props: CheckboxProps {
                checked,
                font_size,
                label_color: None,
            },
            state_cell: None,
            hovered: false,
            focused: false,
            on_change: None,
            label_text,
        }
    }

    pub fn with_font_size(mut self, size: f64) -> Self {
        self.props.font_size = size;
        self.children[0] =
            Box::new(Label::new(&self.label_text, Arc::clone(&self.font)).with_font_size(size));
        self
    }
    pub fn with_label_color(mut self, c: Color) -> Self {
        self.props.label_color = Some(c);
        self
    }

    /// Bind checked state to a shared cell.
    ///
    /// When set, `paint` reads from the cell (so external changes — e.g. a
    /// window's close button — are reflected immediately), and `toggle` writes
    /// to it so both directions stay in sync.
    pub fn with_state_cell(mut self, cell: Rc<Cell<bool>>) -> Self {
        self.state_cell = Some(cell);
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

    pub fn on_change(mut self, cb: impl FnMut(bool) + 'static) -> Self {
        self.on_change = Some(Box::new(cb));
        self
    }

    pub fn checked(&self) -> bool {
        self.props.checked
    }
    pub fn set_checked(&mut self, v: bool) {
        self.props.checked = v;
    }

    fn toggle(&mut self) {
        let new_val = !self.effective_checked();
        self.props.checked = new_val;
        if let Some(ref cell) = self.state_cell {
            cell.set(new_val);
        }
        if let Some(cb) = self.on_change.as_mut() {
            cb(new_val);
        }
    }

    /// Returns the authoritative checked state: the cell value if bound, else
    /// the internal `checked` field.
    #[inline]
    fn effective_checked(&self) -> bool {
        if let Some(ref cell) = self.state_cell {
            cell.get()
        } else {
            self.props.checked
        }
    }

    fn unchecked_colors(v: &crate::theme::Visuals, hovered: bool) -> (Color, Color) {
        let luma = v.bg_color.r * 0.299 + v.bg_color.g * 0.587 + v.bg_color.b * 0.114;
        if luma < 0.5 {
            let fill = if hovered { v.widget_bg } else { v.window_fill };
            (fill, Color::rgba(1.0, 1.0, 1.0, 0.34))
        } else {
            let fill = if hovered {
                v.widget_bg_hovered
            } else {
                v.widget_bg
            };
            (fill, v.widget_stroke)
        }
    }
}

impl Widget for Checkbox {
    fn type_name(&self) -> &'static str {
        "Checkbox"
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

    #[cfg(feature = "reflect")]
    fn as_reflect(&self) -> Option<&dyn bevy_reflect::Reflect> {
        Some(&self.props)
    }
    #[cfg(feature = "reflect")]
    fn as_reflect_mut(&mut self) -> Option<&mut dyn bevy_reflect::Reflect> {
        Some(&mut self.props)
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
        let box_slot_w = BOX_SIZE + FOCUS_PAD * 2.0;
        let h = (BOX_SIZE + FOCUS_PAD * 2.0).max(self.props.font_size * 1.25);
        let label_avail_w = (available.width - box_slot_w - GAP).max(0.0);
        let s = self.children[0].layout(Size::new(label_avail_w, h));
        let lx = if self.label_text.is_empty() {
            box_slot_w
        } else {
            box_slot_w + GAP
        };
        let ly = (h - s.height) * 0.5;
        self.children[0]
            .set_bounds(Rect::new(lx, ly, s.width, s.height));
        let natural_w = if self.label_text.is_empty() {
            box_slot_w
        } else {
            box_slot_w + GAP + s.width
        };
        let w = natural_w.min(available.width);
        self.bounds = Rect::new(0.0, 0.0, w, h);
        Size::new(w, h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let h = self.bounds.height;
        let box_x = FOCUS_PAD;
        let box_y = (h - BOX_SIZE) * 0.5;

        // Focus ring
        if self.focused {
            ctx.set_stroke_color(v.accent_focus);
            ctx.set_line_width(2.0);
            ctx.begin_path();
            ctx.rounded_rect(
                box_x - 1.5,
                box_y - 1.5,
                BOX_SIZE + 3.0,
                BOX_SIZE + 3.0,
                4.0,
            );
            ctx.stroke();
        }

        let checked = self.effective_checked();

        // Box background
        let (unchecked_bg, unchecked_border) = Self::unchecked_colors(&v, self.hovered);
        let bg = if checked { v.accent } else { unchecked_bg };
        ctx.set_fill_color(bg);
        ctx.begin_path();
        ctx.rounded_rect(box_x, box_y, BOX_SIZE, BOX_SIZE, 3.0);
        ctx.fill();

        // Box border
        let border = if checked {
            v.widget_stroke_active
        } else {
            unchecked_border
        };
        ctx.set_stroke_color(border);
        ctx.set_line_width(BOX_STROKE_WIDTH);
        ctx.begin_path();
        let stroke_inset = BOX_STROKE_WIDTH * 0.5;
        ctx.rounded_rect(
            box_x + stroke_inset,
            box_y + stroke_inset,
            BOX_SIZE - BOX_STROKE_WIDTH,
            BOX_SIZE - BOX_STROKE_WIDTH,
            3.0,
        );
        ctx.stroke();

        // Checkmark — coordinates in Y-up space (origin = box bottom-left).
        if checked {
            ctx.set_stroke_color(Color::white());
            ctx.set_line_width(2.0);
            ctx.begin_path();
            let bx = box_x;
            let by = box_y;
            ctx.move_to(bx + 3.0, by + BOX_SIZE * 0.55);
            ctx.line_to(bx + BOX_SIZE * 0.42, by + BOX_SIZE * 0.28);
            ctx.line_to(bx + BOX_SIZE - 3.0, by + BOX_SIZE * 0.75);
            ctx.stroke();
        }

        // Label colour — child paints itself via the framework's tree walk.
        let label_color = self.props.label_color.unwrap_or(v.text_color);
        self.children[0].set_label_color(label_color);
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseMove { pos } => {
                let was = self.hovered;
                self.hovered = self.hit_test(*pos);
                if was != self.hovered {
                    crate::animation::request_draw();
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }
            Event::MouseDown {
                button: MouseButton::Left,
                ..
            } => EventResult::Consumed,
            Event::MouseUp {
                button: MouseButton::Left,
                pos,
                ..
            } => {
                if self.hit_test(*pos) {
                    self.toggle();
                    crate::animation::request_draw();
                }
                EventResult::Consumed
            }
            Event::KeyDown {
                key: Key::Char(' '),
                ..
            } => {
                self.toggle();
                crate::animation::request_draw();
                EventResult::Consumed
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
