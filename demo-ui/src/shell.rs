use std::cell::Cell;
use std::rc::Rc;

use agg_gui::{
    widget::current_mouse_world, DrawCtx, Event, EventResult, MouseButton, Rect, Size, Widget,
};

// ── Canvas background ──────────────────────────────────────────────────────────

pub(crate) struct CanvasBg {
    pub(crate) bounds: Rect,
    pub(crate) children: Vec<Box<dyn Widget>>,
}

impl CanvasBg {
    pub(crate) fn new() -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
        }
    }
}

impl Widget for CanvasBg {
    fn type_name(&self) -> &'static str {
        "CanvasBg"
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
    fn layout(&mut self, available: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
        available
    }
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        ctx.set_fill_color(ctx.visuals().bg_color);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, self.bounds.width, self.bounds.height);
        ctx.fill();
    }
    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

// ── Top menu bar ──────────────────────────────────────────────────────────────

/// Thin bar at the top of the window — mirrors egui's `Panel::top("menu_bar")`.
/// Contains a theme-toggle row on the right (☀ / 🌙 / System).
// Layout: a single FlexRow child fills the bar.
pub(crate) struct TopMenuBar {
    pub(crate) bounds: Rect,
    pub(crate) children: Vec<Box<dyn Widget>>,
    h_offset: f64,
    content_width: f64,
    middle_dragging: bool,
    middle_start_world_x: f64,
    middle_start_h_offset: f64,
}

impl TopMenuBar {
    const H: f64 = 36.0;
    const MOBILE_BREAKPOINT: f64 = 720.0;
    const DESKTOP_CONTENT_W: f64 = 562.0;
    const MOBILE_CONTENT_W: f64 = 662.0;

    pub(crate) fn new(inner_row: Box<dyn Widget>) -> Self {
        Self {
            bounds: Rect::default(),
            children: vec![inner_row],
            h_offset: 0.0,
            content_width: 0.0,
            middle_dragging: false,
            middle_start_world_x: 0.0,
            middle_start_h_offset: 0.0,
        }
    }

    fn min_content_width(viewport_width: f64) -> f64 {
        if viewport_width < Self::MOBILE_BREAKPOINT {
            Self::MOBILE_CONTENT_W
        } else {
            Self::DESKTOP_CONTENT_W
        }
    }

    fn max_scroll(&self) -> f64 {
        (self.content_width - self.bounds.width).max(0.0)
    }

    fn clamp_offset(&mut self) {
        self.h_offset = self.h_offset.clamp(0.0, self.max_scroll());
    }

    fn update_child_bounds(&mut self) {
        if let Some(child) = self.children.first_mut() {
            child.set_bounds(Rect::new(
                -self.h_offset.round(),
                0.0,
                self.content_width,
                Self::H,
            ));
        }
    }
}

impl Widget for TopMenuBar {
    fn type_name(&self) -> &'static str {
        "TopMenuBar"
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

    fn layout(&mut self, available: Size) -> Size {
        let h = Self::H;
        self.bounds = Rect::new(0.0, 0.0, available.width, h);
        self.content_width = available
            .width
            .max(Self::min_content_width(available.width));
        self.clamp_offset();
        if let Some(child) = self.children.first_mut() {
            child.layout(Size::new(self.content_width, h));
        }
        self.update_child_bounds();
        Size::new(available.width, h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        ctx.set_fill_color(v.top_bar_bg);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, self.bounds.width, self.bounds.height);
        ctx.fill();
        // Bottom separator line — match the `Separator` widget colour so
        // horizontal and vertical splits share the same tone.  Y-up: the
        // bar's local y=0 is its BOTTOM edge (where it meets the body
        // below), so the line goes there, not at `height - 1` (which is
        // the top of the bar, flush with the window edge).
        ctx.set_fill_color(v.separator);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, self.bounds.width, 1.0);
        ctx.fill();
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseWheel {
                delta_x,
                delta_y,
                modifiers,
                ..
            } => {
                let delta = if delta_x.abs() > f64::EPSILON {
                    *delta_x
                } else if modifiers.shift {
                    *delta_y
                } else {
                    0.0
                };
                if delta.abs() <= f64::EPSILON || self.max_scroll() <= 0.0 {
                    return EventResult::Ignored;
                }
                let before = self.h_offset;
                self.h_offset += delta * 40.0;
                self.clamp_offset();
                if (self.h_offset - before).abs() > f64::EPSILON {
                    self.update_child_bounds();
                    agg_gui::animation::request_draw();
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }
            Event::MouseDown {
                button: MouseButton::Middle,
                ..
            } if self.max_scroll() > 0.0 => {
                self.middle_dragging = true;
                self.middle_start_world_x = current_mouse_world().map(|p| p.x).unwrap_or(0.0);
                self.middle_start_h_offset = self.h_offset;
                EventResult::Consumed
            }
            Event::MouseMove { pos } if self.middle_dragging => {
                let world_x = current_mouse_world().map(|p| p.x).unwrap_or(pos.x);
                self.h_offset = self.middle_start_h_offset - (world_x - self.middle_start_world_x);
                self.clamp_offset();
                self.update_child_bounds();
                agg_gui::animation::request_draw();
                EventResult::Consumed
            }
            Event::MouseUp {
                button: MouseButton::Middle,
                ..
            } if self.middle_dragging => {
                self.middle_dragging = false;
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }
}

// ── Backend panel pane ────────────────────────────────────────────────────────

/// Wraps the backend panel; returns zero width when hidden so FlexRow collapses it.
pub(crate) struct BackendPane {
    pub(crate) bounds: Rect,
    pub(crate) children: Vec<Box<dyn Widget>>,
    pub(crate) show: Rc<Cell<bool>>,
}

impl BackendPane {
    const PANEL_W: f64 = 240.0;
}

impl Widget for BackendPane {
    fn type_name(&self) -> &'static str {
        "BackendPane"
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

    fn layout(&mut self, available: Size) -> Size {
        if !self.show.get() {
            self.bounds = Rect::new(0.0, 0.0, 0.0, available.height);
            return Size::new(0.0, available.height);
        }
        let w = Self::PANEL_W.min(available.width);
        self.bounds = Rect::new(0.0, 0.0, w, available.height);
        if let Some(child) = self.children.first_mut() {
            child.layout(Size::new(w, available.height));
            child.set_bounds(Rect::new(0.0, 0.0, w, available.height));
        }
        Size::new(w, available.height)
    }

    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn paint_overlay(&mut self, ctx: &mut dyn DrawCtx) {
        if !self.show.get() {
            return;
        }
        // 1-px vertical separator line on the right edge, matched to the
        // `Separator` widget colour so horizontal and vertical splits
        // share the same tone.  Drawn in `paint_overlay` so it sits above
        // the child `FlexColumn`'s panel_bg fill.
        let v = ctx.visuals();
        ctx.set_fill_color(v.separator);
        ctx.begin_path();
        ctx.rect(self.bounds.width - 1.0, 0.0, 1.0, self.bounds.height);
        ctx.fill();
    }
    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

// ── Sidebar pane ──────────────────────────────────────────────────────────────

/// Fixed-width wrapper for the right sidebar that also paints a 1-px
/// separator line on its left edge.
pub(crate) struct SidebarPane {
    pub(crate) bounds: Rect,
    pub(crate) children: Vec<Box<dyn Widget>>,
    pub(crate) mobile_menu_open: Rc<Cell<bool>>,
}

impl SidebarPane {
    const PANEL_W: f64 = 220.0;
    const MOBILE_BREAKPOINT: f64 = 720.0;
    const MOBILE_PANEL_W: f64 = 300.0;

    pub(crate) fn new(inner: Box<dyn Widget>, mobile_menu_open: Rc<Cell<bool>>) -> Self {
        Self {
            bounds: Rect::default(),
            children: vec![inner],
            mobile_menu_open,
        }
    }

    fn mobile_mode(available: Size) -> bool {
        available.width < Self::MOBILE_BREAKPOINT
    }
}

impl Widget for SidebarPane {
    fn type_name(&self) -> &'static str {
        "SidebarPane"
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

    fn layout(&mut self, available: Size) -> Size {
        let mobile = Self::mobile_mode(available);
        if mobile && !self.mobile_menu_open.get() {
            self.bounds = Rect::new(0.0, 0.0, 0.0, available.height);
            return Size::new(0.0, available.height);
        }
        let target_w = if mobile {
            Self::MOBILE_PANEL_W
        } else {
            Self::PANEL_W
        };
        let w = target_w.min(available.width);
        self.bounds = Rect::new(0.0, 0.0, w, available.height);
        if let Some(child) = self.children.first_mut() {
            // Inner content starts 1 px in so the separator sits at x=0.
            let inner_w = (w - 1.0).max(0.0);
            child.layout(Size::new(inner_w, available.height));
            child.set_bounds(Rect::new(1.0, 0.0, inner_w, available.height));
        }
        Size::new(w, available.height)
    }

    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn paint_overlay(&mut self, ctx: &mut dyn DrawCtx) {
        if self.bounds.width <= 0.5 {
            return;
        }
        // Uses `separator` to match the `Separator` widget tone used by
        // horizontal splits elsewhere.  Drawn in `paint_overlay` so the
        // sidebar's panel_bg fill can't cover it.
        let v = ctx.visuals();
        ctx.set_fill_color(v.separator);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, 1.0, self.bounds.height);
        ctx.fill();
    }

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}
