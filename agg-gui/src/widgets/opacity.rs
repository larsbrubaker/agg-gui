//! Opacity — paint one child subtree at a shared, animatable opacity.
//!
//! The fade goes through the framework's subtree compositing layer
//! ([`Widget::compositing_layer`], painted by `widget/paint.rs`): the child
//! renders at full strength into a transparent layer that is blended onto the
//! parent once at the opacity, so overlapping shapes and backbuffered text fade
//! evenly instead of each shape fading on its own. The opacity is read on every
//! paint and hit test from a shared source, so an animation (a tween writing a
//! `Cell`, or a closure over one) drives it without rebuilding the tree.
//!
//! The three ranges:
//! - **1** (or more, or NaN): no layer at all, so a widget at rest costs
//!   nothing extra and paints exactly as it would unwrapped.
//! - **between 0 and 1**: one compositing layer at that alpha. Input works as
//!   usual: a panel half way through fading in is already usable.
//! - **0** (or less): nothing is painted ([`Widget::suppresses_paint`]) and the
//!   subtree takes no input: the pointer passes through to whatever is behind,
//!   Tab skips its fields and a field inside it loses keyboard focus. Layout
//!   is unchanged, so neighbours do not move as a fade starts or ends; wrap it
//!   in [`Conditional`](super::Conditional) to give the space back as well.
//!
//! Whoever changes the opacity requests the next frame
//! ([`crate::animation::request_draw`]), as for any shared-cell widget.

use std::cell::Cell;
use std::rc::Rc;

use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Point, Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::widget::{CompositingLayer, Widget};

/// Wraps a single child and paints it at the opacity its source reports.
/// See the module docs for what 1, partial and 0 opacity do.
pub struct Opacity {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>, // exactly one
    base: WidgetBase,
    source: Rc<dyn Fn() -> f64>,
}

impl Opacity {
    /// Paint `child` at the opacity held in `opacity` (`0.0..=1.0`).
    pub fn new(opacity: Rc<Cell<f64>>, child: Box<dyn Widget>) -> Self {
        Self::from_fn(move || opacity.get(), child)
    }

    /// Paint `child` at the opacity `source` returns, asked on every paint and
    /// hit test (an animation's current value, for example).
    pub fn from_fn(source: impl Fn() -> f64 + 'static, child: Box<dyn Widget>) -> Self {
        Self {
            bounds: Rect::default(),
            children: vec![child],
            base: WidgetBase::new(),
            source: Rc::new(source),
        }
    }

    /// The opacity in effect: the source's value clamped to `0.0..=1.0`, NaN
    /// reading as opaque so a bad value never hides the content.
    pub fn opacity(&self) -> f64 {
        let v = (self.source)();
        if v.is_nan() {
            1.0
        } else {
            v.clamp(0.0, 1.0)
        }
    }

    /// Whether the subtree is fully faded out: painted not at all, no input.
    fn faded_out(&self) -> bool {
        self.opacity() <= 0.0
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
}

impl Widget for Opacity {
    crate::widgets::widget_as_any!();
    fn type_name(&self) -> &'static str {
        "Opacity"
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

    /// Takes the child's size at every opacity, so a fade never moves the
    /// widgets around it.
    fn layout(&mut self, available: Size) -> Size {
        let size = match self.children.first_mut() {
            Some(child) => {
                let s = child.layout(available);
                child.set_bounds(Rect::new(0.0, 0.0, s.width, s.height));
                s
            }
            None => Size::new(0.0, 0.0),
        };
        self.bounds = Rect::new(0.0, 0.0, size.width, size.height);
        size
    }

    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}

    fn compositing_layer(&mut self) -> Option<CompositingLayer> {
        let alpha = self.opacity();
        if alpha >= 1.0 || alpha <= 0.0 {
            return None;
        }
        Some(CompositingLayer::new(0.0, 0.0, 0.0, 0.0, alpha))
    }

    fn suppresses_paint(&self) -> bool {
        self.faded_out()
    }

    /// Fully faded out, the pointer passes through to what is behind.
    fn hit_test(&self, local_pos: Point) -> bool {
        if self.faded_out() {
            return false;
        }
        local_pos.x >= 0.0
            && local_pos.x <= self.bounds.width
            && local_pos.y >= 0.0
            && local_pos.y <= self.bounds.height
    }

    /// Fully faded out, focus can neither reach nor stay in the subtree.
    fn blocks_child_interaction(&self) -> bool {
        self.faded_out()
    }

    /// An invisible subtree's animations (a spinner) keep no frames coming.
    fn needs_draw(&self) -> bool {
        !self.faded_out() && self.children.iter().any(|c| c.needs_draw())
    }

    fn next_draw_deadline(&self) -> Option<web_time::Instant> {
        if self.faded_out() {
            return None;
        }
        self.children
            .iter()
            .filter_map(|c| c.next_draw_deadline())
            .min()
    }

    fn properties(&self) -> Vec<(&'static str, String)> {
        vec![("opacity", format!("{:.3}", self.opacity()))]
    }

    fn on_event(&mut self, _e: &Event) -> EventResult {
        EventResult::Ignored
    }
}
