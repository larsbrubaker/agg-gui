//! Test widgets shared by the rust-only runner tests (`runner_named_tests.rs`,
//! `pointer_reach_tests.rs`): a named rectangle whose visibility and enabled
//! state are switched from another thread or an idle action, and a container
//! that lets every press through to what is under it (the GUI demo's fading
//! window, which C#'s `PointerReach` exists for). A `Switchable` can also
//! clip its children (all, or one slot), paint in the overlay pass and
//! enforce integer bounds, for the paint-geometry lookups.

#![allow(dead_code)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use agg_gui::draw_ctx::DrawCtx;
use agg_gui::event::{Event, EventResult};
use agg_gui::layout_props::WidgetBase;
use agg_gui::{Point, Rect, Size, Widget};

/// A shared on/off switch an idle action can flip (`ui_thread` actions must
/// be `Send`).
pub type Switch = Arc<AtomicBool>;

pub fn switch(on: bool) -> Switch {
    Arc::new(AtomicBool::new(on))
}

pub fn set(switch: &Switch, on: bool) {
    switch.store(on, Ordering::SeqCst);
}

/// A named rectangle at fixed bounds whose `is_visible` / `is_enabled` read
/// shared switches, and whose `hit_test` can refuse every press.
pub struct Switchable {
    bounds: Rect,
    base: WidgetBase,
    children: Vec<Box<dyn Widget>>,
    visible: Switch,
    enabled: Switch,
    takes_presses: bool,
    children_clip: Option<Rect>,
    slot_clips: Vec<(usize, Rect)>,
    defers_paint: bool,
    integer_bounds: bool,
}

impl Switchable {
    pub fn new(name: &str, bounds: Rect) -> Self {
        let mut base = WidgetBase::new();
        base.name = Some(name.to_string());
        Self {
            bounds,
            base,
            children: Vec::new(),
            visible: switch(true),
            enabled: switch(true),
            takes_presses: true,
            children_clip: None,
            slot_clips: Vec::new(),
            defers_paint: false,
            integer_bounds: false,
        }
    }

    pub fn visible_when(mut self, visible: &Switch) -> Self {
        self.visible = Arc::clone(visible);
        self
    }

    pub fn enabled_when(mut self, enabled: &Switch) -> Self {
        self.enabled = Arc::clone(enabled);
        self
    }

    /// Drawn, but every press goes through (C#'s closing window).
    pub fn passing_presses_through(mut self) -> Self {
        self.takes_presses = false;
        self
    }

    pub fn with_child(mut self, child: Box<dyn Widget>) -> Self {
        self.children.push(child);
        self
    }

    /// Clips its children to `rect` (local) rather than its bounds.
    pub fn clipping_children_to(mut self, rect: Rect) -> Self {
        self.children_clip = Some(rect);
        self
    }

    /// Clips child `index` to `rect` (its `child_paint_clip`, a slot).
    pub fn clipping_child(mut self, index: usize, rect: Rect) -> Self {
        self.slot_clips.push((index, rect));
        self
    }

    /// Paints in the global overlay pass (a modal host).
    pub fn deferring_paint_to_overlay(mut self) -> Self {
        self.defers_paint = true;
        self
    }

    /// Enforces integer bounds (painted at rounded, pixel-snapped offsets).
    pub fn with_integer_bounds(mut self) -> Self {
        self.integer_bounds = true;
        self
    }
}

impl Widget for Switchable {
    fn type_name(&self) -> &'static str {
        "Switchable"
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
    fn is_visible(&self) -> bool {
        self.visible.load(Ordering::SeqCst)
    }
    fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }
    fn hit_test(&self, local_pos: Point) -> bool {
        self.takes_presses
            && local_pos.x >= 0.0
            && local_pos.x <= self.bounds.width
            && local_pos.y >= 0.0
            && local_pos.y <= self.bounds.height
    }
    fn layout(&mut self, _available: Size) -> Size {
        for child in &mut self.children {
            let b = child.bounds();
            child.layout(Size::new(b.width, b.height));
            child.set_bounds(b);
        }
        Size::new(self.bounds.width, self.bounds.height)
    }
    fn clip_children_rect(&self) -> Option<(f64, f64, f64, f64)> {
        let r = self.children_clip.unwrap_or(Rect::new(
            0.0,
            0.0,
            self.bounds.width,
            self.bounds.height,
        ));
        Some((r.x, r.y, r.width, r.height))
    }
    fn child_paint_clip(&self, index: usize) -> Option<Rect> {
        self.slot_clips
            .iter()
            .find(|(i, _)| *i == index)
            .map(|(_, r)| *r)
    }
    fn defer_paint_to_overlay(&self) -> bool {
        self.defers_paint
    }
    fn enforce_integer_bounds(&self) -> bool {
        self.integer_bounds
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}
