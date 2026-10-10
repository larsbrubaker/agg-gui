//! `Clickable` — makes any child widget one click target.
//!
//! [`Button`](super::Button) holds a single label; a card made of an icon,
//! several labels and a progress bar needs the whole composite to be the
//! target. `Clickable` wraps one child (typically a fit-height
//! [`Container`](super::Container)), sizes to it, and claims the pointer for
//! its whole area so the child's widgets are visual only. It follows
//! `Button`'s interaction rules (`button_events.rs`): a click needs the press
//! and the release on the widget, Tab focuses it, Enter / Space activate it,
//! and an `enabled_fn` gate disables it. Hover / press paint a translucent
//! tint over the child (in `paint_overlay`, so an opaque card background
//! cannot hide it) and keyboard focus draws a ring. Name it with
//! [`Widget::with_name`] for GUI automation.

use std::rc::Rc;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult, Key, MouseButton};
use crate::geometry::{Point, Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::widget::Widget;

/// Width of the keyboard-focus ring, matching `ButtonTheme`'s default.
const FOCUS_RING_W: f64 = 2.5;
/// Alpha of the theme-text-coloured tint while hovered / pressed — the
/// same faint overlay a ghost `Button` uses.
const HOVER_TINT: f32 = 0.10;
const PRESSED_TINT: f32 = 0.16;

/// One child widget acting as a single clickable target.
pub struct Clickable {
    bounds: Rect,
    /// Exactly one child (empty only if a caller took it via `children_mut`).
    children: Vec<Box<dyn Widget>>,
    base: WidgetBase,
    on_click: Option<Box<dyn FnMut()>>,
    /// When `Some`, enabled only while the closure returns `true`.
    enabled_fn: Option<Rc<dyn Fn() -> bool>>,
    /// Idle fill painted under the child; transparent by default.
    background: Color,
    corner_radius: f64,
    hovered: bool,
    pressed: bool,
    focused: bool,
}

impl Clickable {
    pub fn new(child: Box<dyn Widget>) -> Self {
        Self {
            bounds: Rect::default(),
            children: vec![child],
            base: WidgetBase::new(),
            on_click: None,
            enabled_fn: None,
            background: Color::rgba(0.0, 0.0, 0.0, 0.0),
            corner_radius: 0.0,
            hovered: false,
            pressed: false,
            focused: false,
        }
    }

    pub fn on_click(mut self, cb: impl FnMut() + 'static) -> Self {
        self.on_click = Some(Box::new(cb));
        self
    }

    /// Gate on a live predicate: while it returns `false` the widget ignores
    /// pointer and keyboard input, cannot take focus, and paints dimmed.
    pub fn with_enabled_fn(mut self, f: impl Fn() -> bool + 'static) -> Self {
        self.enabled_fn = Some(Rc::new(f));
        self
    }

    /// Idle fill painted under the child (transparent by default).
    pub fn with_background(mut self, color: Color) -> Self {
        self.background = color;
        self
    }

    /// Corner radius of the background, hover tint and focus ring — set it
    /// to the wrapped card's radius so the highlight follows its shape.
    pub fn with_corner_radius(mut self, r: f64) -> Self {
        self.corner_radius = r;
        self
    }

    /// Whether a pointer click gives this widget keyboard focus (default
    /// `true`); see [`Button::with_focus_on_click`](super::Button::with_focus_on_click).
    pub fn with_focus_on_click(mut self, focus_on_click: bool) -> Self {
        self.base.focus_on_click = focus_on_click;
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

    pub fn is_hovered(&self) -> bool {
        self.hovered
    }
    pub fn is_pressed(&self) -> bool {
        self.pressed
    }

    fn fire_click(&mut self) {
        if let Some(cb) = self.on_click.as_mut() {
            cb();
        }
        // The handler almost always changes state the next paint shows.
        crate::animation::request_draw();
    }

    /// Set hover / press, requesting a repaint when either changed.
    fn set_state(&mut self, hovered: bool, pressed: bool) -> bool {
        let changed = self.hovered != hovered || self.pressed != pressed;
        self.hovered = hovered;
        self.pressed = pressed;
        if changed {
            crate::animation::request_draw();
        }
        changed
    }

    fn handle_event(&mut self, event: &Event) -> EventResult {
        if !self.is_enabled() {
            self.hovered = false;
            self.pressed = false;
            return EventResult::Ignored;
        }
        match event {
            Event::MouseMove { pos } => {
                let hovered = self.hit_test(*pos);
                let pressed = self.pressed && hovered;
                if self.set_state(hovered, pressed) {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            Event::MouseLeave => {
                self.set_state(false, false);
                EventResult::Ignored
            }
            Event::MouseDown {
                button: MouseButton::Left,
                ..
            } => {
                // `hovered` is not set here: touch has no hover phase and
                // nothing would clear it (see `button_events.rs`).
                self.set_state(self.hovered, true);
                EventResult::Consumed
            }
            Event::MouseUp {
                pos,
                button: MouseButton::Left,
                ..
            } => {
                let was_pressed = self.pressed;
                self.set_state(self.hovered, false);
                // The release position decides, as for `Button`.
                if was_pressed && self.hit_test(*pos) {
                    self.fire_click();
                    // The focus ring is a keyboard aid; a pointer click drops it.
                    self.focused = false;
                }
                EventResult::Consumed
            }
            Event::KeyDown {
                key: Key::Enter | Key::Char(' '),
                ..
            } => {
                self.fire_click();
                EventResult::Consumed
            }
            Event::FocusGained => {
                let was = self.focused;
                self.focused = true;
                if was {
                    EventResult::Ignored
                } else {
                    crate::animation::request_draw();
                    EventResult::Consumed
                }
            }
            Event::FocusLost => {
                let changed = self.focused || self.pressed;
                self.focused = false;
                self.pressed = false;
                if changed {
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

impl Widget for Clickable {
    crate::widgets::widget_as_any!();
    fn type_name(&self) -> &'static str {
        "Clickable"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }

    /// The whole area is one target: the child's widgets never take the
    /// pointer, so hover and clicks land here wherever they fall.
    fn claims_pointer_exclusively(&self, _local_pos: Point) -> bool {
        true
    }

    fn is_enabled(&self) -> bool {
        self.enabled_fn.as_ref().map(|f| f()).unwrap_or(true)
    }
    fn is_focusable(&self) -> bool {
        self.is_enabled()
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

    /// Size to the child, laid out at this widget's origin.
    fn layout(&mut self, available: Size) -> Size {
        let Some(child) = self.children.first_mut() else {
            return Size::ZERO;
        };
        let size = child.layout(available);
        child.set_bounds(Rect::new(0.0, 0.0, size.width, size.height));
        size
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        if self.background.a > 0.001 {
            ctx.set_fill_color(self.background);
            ctx.begin_path();
            ctx.rounded_rect(
                0.0,
                0.0,
                self.bounds.width,
                self.bounds.height,
                self.corner_radius,
            );
            ctx.fill();
        }
    }

    fn paint_overlay(&mut self, ctx: &mut dyn DrawCtx) {
        let (w, h, r) = (self.bounds.width, self.bounds.height, self.corner_radius);
        let v = ctx.visuals();
        let tint = if !self.is_enabled() {
            // Dim the content toward the panel colour.
            Some(v.bg_color.with_alpha(0.45))
        } else if self.pressed {
            Some(v.text_color.with_alpha(PRESSED_TINT))
        } else if self.hovered {
            Some(v.text_color.with_alpha(HOVER_TINT))
        } else {
            None
        };
        if let Some(color) = tint {
            ctx.set_fill_color(color);
            ctx.begin_path();
            ctx.rounded_rect(0.0, 0.0, w, h, r);
            ctx.fill();
        }
        // Focus ring just inside the bounds, as `Button` draws it, so a
        // parent clip at the widget edge cannot cut it.
        if self.focused && self.is_enabled() {
            let inset = FOCUS_RING_W * 0.5;
            ctx.set_stroke_color(v.accent_focus);
            ctx.set_line_width(FOCUS_RING_W);
            ctx.begin_path();
            ctx.rounded_rect(
                inset,
                inset,
                (w - FOCUS_RING_W).max(0.0),
                (h - FOCUS_RING_W).max(0.0),
                (r - inset).max(0.0),
            );
            ctx.stroke();
        }
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        self.handle_event(event)
    }

    fn properties(&self) -> Vec<(&'static str, String)> {
        vec![
            ("hovered", self.hovered.to_string()),
            ("pressed", self.pressed.to_string()),
            ("focused", self.focused.to_string()),
        ]
    }
}
