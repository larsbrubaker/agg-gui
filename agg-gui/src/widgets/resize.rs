//! `Resize` — a nested, user-draggable resizable container.
//!
//! Egui-parity port of `egui::Resize`.  Wraps a single child widget
//! and exposes a bottom-right grip the user can drag to resize the
//! subregion independently of its surrounding layout.  Used inside
//! the Window Resize Test's "↔ auto-sized" window to show a resize-
//! within-auto-size behaviour (the outer window fits its content; the
//! inner `Resize` has its own draggable handle).
//!
//! # Coordinate conventions
//!
//! The widget is pure Y-up: local `(0, 0)` is bottom-left.  The SE
//! grip sits at `(w - HANDLE, 0) .. (w, HANDLE)` — bottom-right in
//! screen space.
//!
//! # Drag bookkeeping
//!
//! A drag captures the mouse position in **parent-relative** coords
//! (`local + bounds.xy`) rather than widget-local.  That keeps the
//! drag stable even when the parent's layout shifts the widget's
//! `bounds.y` because the widget's own height just changed — a
//! common situation inside a `FlexColumn`, where stacking widgets
//! push later siblings down when earlier ones grow.

use std::cell::Cell;
use std::rc::Rc;

use crate::cursor::{set_cursor_icon, CursorIcon};
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult, MouseButton};
use crate::geometry::{Point, Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::widget::Widget;

/// Width (and height) of the bottom-right drag grip, in logical pixels.
const HANDLE: f64 = 14.0;

pub struct Resize {
    bounds: Rect,
    /// Always exactly one child — the wrapped content.
    children: Vec<Box<dyn Widget>>,
    base: WidgetBase,

    /// Current size the user has dragged to.  `None` before the first
    /// drag → layout uses `default_size` clamped against available.
    current_size: Option<Size>,
    min_size: Size,
    max_size: Size,
    default_size: Size,

    /// Optional external cell that mirrors `current_size` for
    /// persistence / inspection.  Written each time the user drags.
    size_cell: Option<Rc<Cell<Size>>>,

    // ── drag state ────────────────────────────────────────────────
    dragging: bool,
    hover_handle: bool,
    /// Mouse position in APP-LEVEL world coords at drag start.  Using
    /// world (not widget-local or parent-relative) coords is required
    /// because a nested `Resize` inside an auto-sized `Window` has
    /// ancestor bounds that shift each frame as layout ripples — so
    /// widget-local event positions shift even when the user's cursor
    /// is stationary.  World coords are the only invariant reference.
    drag_start_world: Point,
    drag_start_size: Size,
}

impl Resize {
    /// Wrap `child` in a user-resizable container.  Defaults: 200×150
    /// initial size, 80×40 min, 1000×800 max — override with the
    /// builder methods below.  The defaults are deliberately "sane
    /// demo-friendly" values; match egui's `Resize::default().show(...)`.
    pub fn new(child: Box<dyn Widget>) -> Self {
        Self {
            bounds: Rect::default(),
            children: vec![child],
            base: WidgetBase::new(),
            current_size: None,
            min_size: Size::new(80.0, 40.0),
            // Generous default max — we want `Resize` to be able to
            // grow up to whatever the surrounding layout allows.
            // Override via `with_max_size_hint` to impose a tighter
            // cap; this value is way beyond any realistic screen.
            max_size: Size::new(8000.0, 6000.0),
            default_size: Size::new(200.0, 150.0),
            size_cell: None,
            dragging: false,
            hover_handle: false,
            drag_start_world: Point::ORIGIN,
            drag_start_size: Size::new(0.0, 0.0),
        }
    }

    pub fn with_default_size(mut self, s: Size) -> Self {
        self.default_size = s;
        self
    }
    pub fn with_min_size_hint(mut self, s: Size) -> Self {
        self.min_size = s;
        self
    }
    pub fn with_max_size_hint(mut self, s: Size) -> Self {
        self.max_size = s;
        self
    }

    /// Bind the current size to a shared `Cell<Size>`.  Reads during
    /// layout (so callers can programmatically drive size), writes
    /// during drag (so callers can persist user-chosen geometry).
    pub fn with_size_cell(mut self, cell: Rc<Cell<Size>>) -> Self {
        // Seed so the first layout picks up any persisted value.
        self.current_size = Some(cell.get());
        self.size_cell = Some(cell);
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

    /// Public accessor for tests and inspector integrations.
    pub fn current_size(&self) -> Size {
        self.current_size.unwrap_or(self.default_size)
    }

    /// Widget-local rect of the SE drag grip (Y-up: bottom-right).
    fn handle_rect(&self) -> Rect {
        Rect::new(
            (self.bounds.width - HANDLE).max(0.0),
            0.0,
            HANDLE.min(self.bounds.width),
            HANDLE.min(self.bounds.height),
        )
    }

    fn in_handle(&self, p: Point) -> bool {
        let h = self.handle_rect();
        p.x >= h.x && p.x <= h.x + h.width && p.y >= h.y && p.y <= h.y + h.height
    }
}

impl Widget for Resize {
    fn type_name(&self) -> &'static str {
        "Resize"
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
    fn min_size(&self) -> Size {
        self.min_size
    }
    fn max_size(&self) -> Size {
        self.max_size
    }

    fn layout(&mut self, available: Size) -> Size {
        let _ = available; // Intentionally ignored — see below.
                           // Pick up the latest cell value each frame so external writes
                           // (e.g. persistence restore) propagate into layout.
        if let Some(c) = &self.size_cell {
            self.current_size = Some(c.get());
        }
        let target = self.current_size.unwrap_or(self.default_size);
        // Clamp only to the explicit min_size / max_size hints — NOT
        // to `available`.  A `Resize` widget is the user's "I want
        // exactly this much space" contract: if the user drags it
        // bigger than its parent's current slot, the widget still
        // reports the bigger size so an auto-sized ancestor can grow
        // to fit on the next layout pass.  Matches egui, where the
        // surrounding Window expands when the inner Resize demands
        // more width or height.
        let w_target = target.width.clamp(self.min_size.width, self.max_size.width);
        let h_target = target
            .height
            .clamp(self.min_size.height, self.max_size.height);

        // Content-bound floor: measure the child at the requested
        // target, and if its natural size is larger (e.g. wrapped
        // text at a narrower width produces taller content), enforce
        // content-natural as the minimum.  The user can never drag
        // the Resize smaller than its content fits — matches egui.
        let natural = if let Some(child) = self.children.first_mut() {
            child.layout(Size::new(w_target, h_target))
        } else {
            Size::new(0.0, 0.0)
        };
        let w = w_target.max(natural.width);
        let h = h_target.max(natural.height);
        let size = Size::new(w, h);
        self.current_size = Some(size);
        self.bounds = Rect::new(0.0, 0.0, w, h);

        if let Some(child) = self.children.first_mut() {
            // Re-layout if enforcement inflated either axis.
            if (w - w_target).abs() > 0.5 || (h - h_target).abs() > 0.5 {
                child.layout(size);
            }
            child.set_bounds(Rect::new(0.0, 0.0, w, h));
        }
        size
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let w = self.bounds.width;
        let h = self.bounds.height;

        // 1-px outline so users see the resizable region even before
        // they grab the handle.  Matches egui's `Resize` subtle frame.
        ctx.set_stroke_color(v.widget_stroke);
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.rounded_rect(0.5, 0.5, (w - 1.0).max(0.0), (h - 1.0).max(0.0), 3.0);
        ctx.stroke();

        // SE grip — three stacked diagonals in the bottom-right corner
        // (Y-up: y = 0 is the bottom edge).  Highlight when hovered or
        // actively dragging so the interaction is obvious.
        let grip_color = if self.dragging {
            v.window_resize_active
        } else if self.hover_handle {
            v.window_resize_hover
        } else {
            v.widget_stroke
        };
        ctx.set_stroke_color(grip_color);
        ctx.set_line_width(1.5);
        let m = 3.0_f64;
        for i in 1..=3_i32 {
            let off = i as f64 * 4.0 + m;
            ctx.begin_path();
            ctx.move_to(w - off, m);
            ctx.line_to(w - m, off);
            ctx.stroke();
        }
        // `h` used only by the stroke above; mark silenced for any
        // future refactor that comments out the outline.
        let _ = h;
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseMove { pos } => {
                if self.dragging {
                    // Use APP-LEVEL world coords.  Widget-local and
                    // parent-relative positions both shift between
                    // events here because we're typically nested
                    // inside an auto-sized `Window` whose layout
                    // ripples each frame as our size changes, moving
                    // every ancestor frame in the tree.  World coords
                    // come from `App` via a thread-local set by the
                    // same entry point that dispatched this event, so
                    // they're stable against ancestor reshuffling.
                    let world = crate::widget::current_mouse_world().unwrap_or_else(|| {
                        Point::new(pos.x + self.bounds.x, pos.y + self.bounds.y)
                    });
                    let dx = world.x - self.drag_start_world.x;
                    let dy = world.y - self.drag_start_world.y;
                    // SE handle semantics:
                    //   cursor right  → width  grows  (+dx)
                    //   cursor down   → height grows  (in Y-up, down = dy<0 → -dy)
                    let new_w = (self.drag_start_size.width + dx)
                        .clamp(self.min_size.width, self.max_size.width);
                    let new_h = (self.drag_start_size.height - dy)
                        .clamp(self.min_size.height, self.max_size.height);
                    let new_sz = Size::new(new_w, new_h);
                    self.current_size = Some(new_sz);
                    if let Some(c) = &self.size_cell {
                        c.set(new_sz);
                    }
                    set_cursor_icon(CursorIcon::ResizeNwSe);
                    crate::animation::request_draw();
                    return EventResult::Consumed;
                }
                let was = self.hover_handle;
                self.hover_handle = self.in_handle(*pos);
                if self.hover_handle {
                    set_cursor_icon(CursorIcon::ResizeNwSe);
                }
                if was != self.hover_handle {
                    crate::animation::request_draw();
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }
            Event::MouseDown {
                pos,
                button: MouseButton::Left | MouseButton::Middle,
                ..
            } if self.in_handle(*pos) => {
                self.dragging = true;
                // Snapshot the world cursor pos at drag start.  If
                // unavailable (a unit test dispatching events directly
                // without going through `App`), fall back to parent-
                // relative — widget-local drag semantics work when no
                // ancestor layout ripple is happening.
                self.drag_start_world = crate::widget::current_mouse_world()
                    .unwrap_or_else(|| Point::new(pos.x + self.bounds.x, pos.y + self.bounds.y));
                self.drag_start_size = Size::new(self.bounds.width, self.bounds.height);
                set_cursor_icon(CursorIcon::ResizeNwSe);
                crate::animation::request_draw();
                EventResult::Consumed
            }
            Event::MouseUp { .. } if self.dragging => {
                self.dragging = false;
                crate::animation::request_draw();
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    fn hit_test(&self, local_pos: Point) -> bool {
        local_pos.x >= 0.0
            && local_pos.x <= self.bounds.width
            && local_pos.y >= 0.0
            && local_pos.y <= self.bounds.height
    }

    fn properties(&self) -> Vec<(&'static str, String)> {
        let s = self.current_size();
        vec![
            ("current_w", format!("{:.1}", s.width)),
            ("current_h", format!("{:.1}", s.height)),
            ("min_w", format!("{:.1}", self.min_size.width)),
            ("max_w", format!("{:.1}", self.max_size.width)),
            ("dragging", self.dragging.to_string()),
        ]
    }
}
