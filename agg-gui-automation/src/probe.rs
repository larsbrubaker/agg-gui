//! `ProbeWidget`: the stand-in for a plain C# `GuiWidget` in the ported
//! automation tests — a named rectangle with optional background colour and
//! children placed where they were put, that records every event it is sent
//! and can answer them through a callback.
//!
//! C#'s tests build trees of bare `GuiWidget`s (`new GuiWidget { Name =
//! "level0", LocalBounds = ... }`) and hook their `MouseDown`/`MouseEnter`
//! events. agg-gui has no generic widget with public events, so the tests use
//! this. Children keep the bounds they were given (C#'s `GuiWidget` without
//! anchors); the probe itself keeps its bounds unless a parent (or the `App`,
//! for the root) sets them. Used by [`crate::driver::HeadlessWindow`] as its
//! root and by the tests in `tests/`.
//!
//! A probe given [`ProbeWidget::on_click`] has C#'s `Click` semantics
//! (`GuiWidget.OnMouseDown`/`OnMouseUp`): it takes a press that lands on
//! its own surface (not over one of its visible, enabled children), which
//! captures the pointer, and clicks when that press is released inside its
//! bounds and again not over a child. A press or a release over a child is
//! the child's, so neither widget clicks.

use std::cell::RefCell;
use std::rc::Rc;

use agg_gui::draw_ctx::DrawCtx;
use agg_gui::event::{Event, EventResult};
use agg_gui::layout_props::WidgetBase;
use agg_gui::{Color, Point, Rect, Size, Widget};

/// The events a probe has received, in order, shared with the test.
pub type ProbeLog = Rc<RefCell<Vec<Event>>>;

/// How a probe answers an event: the callback's result, or `Ignored`.
type EventHandler = Box<dyn FnMut(&Event) -> EventResult>;

/// C#'s `Click` subscription: called with the release that completed the click.
type ClickHandler = Box<dyn FnMut(&Event)>;

/// A named, optionally filled rectangle that logs its events (C# `GuiWidget`).
pub struct ProbeWidget {
    bounds: Rect,
    base: WidgetBase,
    children: Vec<Box<dyn Widget>>,
    background: Option<Color>,
    visible: bool,
    focusable: bool,
    log: ProbeLog,
    handler: Option<EventHandler>,
    click: Option<ClickHandler>,
    /// C# `MouseDownOnWidget`: a press landed on this probe's own surface
    /// and has not been released yet.
    mouse_down_on_widget: bool,
}

impl ProbeWidget {
    /// A probe named `name` with empty bounds (C# `new GuiWidget { Name }`).
    pub fn new(name: impl Into<String>) -> Self {
        let mut base = WidgetBase::new();
        base.name = Some(name.into());
        Self {
            bounds: Rect::default(),
            base,
            children: Vec::new(),
            background: None,
            visible: true,
            focusable: false,
            log: Rc::default(),
            handler: None,
            click: None,
            mouse_down_on_widget: false,
        }
    }

    /// Place the probe in its parent (C# `LocalBounds` plus
    /// `OriginRelativeParent`; Y-up, parent-local).
    pub fn with_bounds(mut self, bounds: Rect) -> Self {
        self.bounds = bounds;
        self
    }

    /// Fill the probe's bounds with `color` when painted (C#
    /// `BackgroundColor`).
    pub fn with_background(mut self, color: Color) -> Self {
        self.background = Some(color);
        self
    }

    /// Let the probe take keyboard focus.
    pub fn with_focusable(mut self, focusable: bool) -> Self {
        self.focusable = focusable;
        self
    }

    /// Add a child, kept at the bounds it already has (C# `AddChild`).
    pub fn with_child(mut self, child: Box<dyn Widget>) -> Self {
        self.children.push(child);
        self
    }

    /// Add a child to a probe already built.
    pub fn add_child(&mut self, child: Box<dyn Widget>) {
        self.children.push(child);
    }

    /// Answer events with `handler` (C#'s event subscriptions; return
    /// `Consumed` to stop the bubble). Every event is logged first either way.
    pub fn on_event_with(mut self, handler: impl FnMut(&Event) -> EventResult + 'static) -> Self {
        self.handler = Some(Box::new(handler));
        self
    }

    /// C# `Click += ...`: call `handler` with the release of every click on
    /// this probe (see the module docs for when a press and release make
    /// one). Any button clicks, as in C#.
    pub fn on_click(mut self, handler: impl FnMut(&Event) + 'static) -> Self {
        self.click = Some(Box::new(handler));
        self
    }

    /// Whether `pos` (probe-local, Y-up) is over one of the probe's visible,
    /// enabled children — C#'s test of where a press or release belongs.
    fn over_child(&self, pos: Point) -> bool {
        self.children.iter().rev().any(|child| {
            let b = child.bounds();
            child.is_visible()
                && child.is_enabled()
                && child.hit_test(Point::new(pos.x - b.x, pos.y - b.y))
        })
    }

    /// C#'s click bookkeeping for one event; returns `Consumed` for the
    /// press it takes (so the `App` captures the pointer for the probe) and
    /// the release that ends it.
    fn track_click(&mut self, event: &Event) -> EventResult {
        if self.click.is_none() {
            return EventResult::Ignored;
        }
        match event {
            Event::MouseDown { pos, .. } => {
                self.mouse_down_on_widget = !self.over_child(*pos);
                if self.mouse_down_on_widget {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            Event::MouseUp { pos, .. } => {
                if !std::mem::take(&mut self.mouse_down_on_widget) {
                    return EventResult::Ignored;
                }
                if self.hit_test(*pos) && !self.over_child(*pos) {
                    if let Some(click) = self.click.as_mut() {
                        click(event);
                    }
                }
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    /// The probe's event log, to keep before the probe moves into a tree.
    pub fn log(&self) -> ProbeLog {
        Rc::clone(&self.log)
    }

    /// Show or hide the probe and its subtree (C# `Visible`).
    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    /// The probe's background colour, if it has one.
    pub fn background(&self) -> Option<Color> {
        self.background
    }
}

impl Widget for ProbeWidget {
    fn type_name(&self) -> &'static str {
        "ProbeWidget"
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

    fn widget_base(&self) -> Option<&WidgetBase> {
        Some(&self.base)
    }

    fn widget_base_mut(&mut self) -> Option<&mut WidgetBase> {
        Some(&mut self.base)
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }

    fn is_visible(&self) -> bool {
        self.visible
    }

    fn is_focusable(&self) -> bool {
        self.focusable
    }

    /// Children are laid out at their own size and keep their place; the
    /// probe reports its own size (a fixed-size C# `GuiWidget`).
    fn layout(&mut self, _available: Size) -> Size {
        for child in &mut self.children {
            let b = child.bounds();
            child.layout(Size::new(b.width, b.height));
            child.set_bounds(b);
        }
        Size::new(self.bounds.width, self.bounds.height)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        if let Some(color) = self.background {
            ctx.set_fill_color(color);
            ctx.begin_path();
            ctx.rect(0.0, 0.0, self.bounds.width, self.bounds.height);
            ctx.fill();
        }
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        self.log.borrow_mut().push(event.clone());
        let handled = match self.handler.as_mut() {
            Some(handler) => handler(event),
            None => EventResult::Ignored,
        };
        // Whichever took the event answers; both taking it is a loud consume.
        match (handled, self.track_click(event)) {
            (EventResult::Ignored, clicked) => clicked,
            (handled, EventResult::Ignored) => handled,
            _ => EventResult::Consumed,
        }
    }
}
