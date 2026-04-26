use std::cell::Cell;
use std::rc::Rc;

use agg_gui::{DrawCtx, Event, EventResult, Rect, Size, Widget};

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
}

impl TopMenuBar {
    pub(crate) fn new(inner_row: Box<dyn Widget>) -> Self {
        Self {
            bounds: Rect::default(),
            children: vec![inner_row],
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
        let h = 36.0_f64;
        self.bounds = Rect::new(0.0, 0.0, available.width, h);
        if let Some(child) = self.children.first_mut() {
            child.layout(Size::new(available.width, h));
            child.set_bounds(Rect::new(0.0, 0.0, available.width, h));
        }
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

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
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
