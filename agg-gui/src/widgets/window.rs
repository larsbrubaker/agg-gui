//! `Window` — a floating, draggable panel with a title bar.
//!
//! # Usage
//!
//! Create a `Window` and place it as the **last** child of a [`Stack`] so it
//! paints on top of everything and receives hit-test priority.
//!
//! ```ignore
//! let win = Window::new("Inspector", font, Box::new(my_content));
//! Stack::new()
//!     .add(Box::new(main_ui))
//!     .add(Box::new(win))
//! ```
//!
//! # Coordinate notes (Y-up)
//!
//! `bounds` stores the window's position in its **parent's** coordinate space.
//! The title bar is at the **top** of the window, i.e. local Y ∈
//! `[height − TITLE_H .. height]`. The content area fills local Y ∈ `[0 .. height − TITLE_H]`.
//!
//! Drag uses world-space anchoring: `drag_start_world = bounds.xy + click_local`,
//! `drag_start_bounds = bounds at click time`. Every subsequent MouseMove
//! re-derives world pos (`pos + current_bounds.xy`) and applies the offset, so
//! the dragged point stays exactly under the cursor even as the window moves.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;


use crate::event::{Event, EventResult, MouseButton};
use crate::geometry::{Point, Rect, Size};
use crate::draw_ctx::DrawCtx;
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::text::Font;
use crate::widget::Widget;

const TITLE_H: f64 = 28.0;
const CORNER_R: f64 = 8.0;
const SHADOW_BLUR: f64 = 6.0; // extra size of shadow rect on each side
const CLOSE_R: f64 = 6.0;     // radius of close button circle
const CLOSE_PAD: f64 = 10.0;  // padding from right edge to close button center

/// A floating panel with a draggable title bar and a single content child.
pub struct Window {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>, // always exactly 1: the content
    base: WidgetBase,

    title: String,
    font: Arc<Font>,
    font_size: f64,

    visible: bool,
    /// When set, `is_visible()` delegates to this cell; the close button also
    /// writes `false` here so the sidebar checkbox stays in sync.
    visible_cell: Option<Rc<Cell<bool>>>,
    /// When the cell holds `Some(rect)`, the next `set_bounds` call moves the
    /// window to that rect and clears the cell.  Used by "Organize windows".
    reset_to: Option<Rc<Cell<Option<Rect>>>>,

    dragging: bool,
    /// Cursor world position when drag started.
    drag_start_world: Point,
    /// Window bounds when drag started.
    drag_start_bounds: Rect,

    close_hovered: bool,
    /// Called once when the close button is clicked.
    on_close: Option<Box<dyn FnMut()>>,
}

impl Window {
    /// Create a new window with the given title, font, and content widget.
    ///
    /// Default position: `(60, 60)` with `size = (360, 280)`. Call
    /// [`with_bounds`] to override.
    pub fn new(title: impl Into<String>, font: Arc<Font>, content: Box<dyn Widget>) -> Self {
        Self {
            bounds: Rect::new(60.0, 60.0, 360.0, 280.0),
            children: vec![content],
            base: WidgetBase::new(),
            title: title.into(),
            font,
            font_size: 13.0,
            visible: true,
            visible_cell: None,
            reset_to: None,
            dragging: false,
            drag_start_world: Point::ORIGIN,
            drag_start_bounds: Rect::default(),
            close_hovered: false,
            on_close: None,
        }
    }

    pub fn with_bounds(mut self, b: Rect) -> Self { self.bounds = b; self }
    pub fn with_font_size(mut self, size: f64) -> Self { self.font_size = size; self }

    /// Bind window visibility to a shared cell.
    ///
    /// `is_visible()` reads from the cell; the close button writes `false` to it
    /// so the sidebar checkbox stays in sync without an `on_close` callback.
    pub fn with_visible_cell(mut self, cell: Rc<Cell<bool>>) -> Self {
        self.visible_cell = Some(cell);
        self
    }

    /// Provide a "reset position" cell for the "Organize windows" feature.
    ///
    /// When the cell holds `Some(rect)`, the next layout pass moves the window
    /// to that rect and clears the cell to `None`.
    pub fn with_reset_cell(mut self, cell: Rc<Cell<Option<Rect>>>) -> Self {
        self.reset_to = Some(cell);
        self
    }

    pub fn with_margin(mut self, m: Insets)    -> Self { self.base.margin   = m; self }
    pub fn with_h_anchor(mut self, h: HAnchor) -> Self { self.base.h_anchor = h; self }
    pub fn with_v_anchor(mut self, v: VAnchor) -> Self { self.base.v_anchor = v; self }
    pub fn with_min_size(mut self, s: Size)    -> Self { self.base.min_size = s; self }
    pub fn with_max_size(mut self, s: Size)    -> Self { self.base.max_size = s; self }

    /// Register a callback fired once when the close button is clicked.
    pub fn on_close(mut self, cb: impl FnMut() + 'static) -> Self {
        self.on_close = Some(Box::new(cb));
        self
    }

    pub fn show(&mut self) { self.visible = true; }
    pub fn hide(&mut self) { self.visible = false; }
    pub fn toggle(&mut self) { self.visible = !self.visible; }
    pub fn is_visible(&self) -> bool { self.visible }

    // Local Y of the title bar bottom edge (Y-up: title bar top = height).
    fn title_bar_bottom(&self) -> f64 {
        self.bounds.height - TITLE_H
    }

    fn in_title_bar(&self, local: Point) -> bool {
        local.y >= self.title_bar_bottom() && local.y <= self.bounds.height
            && local.x >= 0.0 && local.x <= self.bounds.width
    }

    // Center of the close button in local coords.
    fn close_center(&self) -> Point {
        Point::new(
            self.bounds.width - CLOSE_PAD,
            self.bounds.height - TITLE_H * 0.5,
        )
    }

    fn in_close_button(&self, local: Point) -> bool {
        let c = self.close_center();
        let dx = local.x - c.x;
        let dy = local.y - c.y;
        dx * dx + dy * dy <= (CLOSE_R + 3.0) * (CLOSE_R + 3.0)
    }
}

impl Widget for Window {
    fn type_name(&self) -> &'static str { "Window" }
    fn is_visible(&self) -> bool {
        if let Some(ref cell) = self.visible_cell { cell.get() } else { self.visible }
    }
    fn bounds(&self) -> Rect { self.bounds }

    fn margin(&self)   -> Insets  { self.base.margin }
    fn h_anchor(&self) -> HAnchor { self.base.h_anchor }
    fn v_anchor(&self) -> VAnchor { self.base.v_anchor }
    fn min_size(&self) -> Size    { self.base.min_size }
    fn max_size(&self) -> Size    { self.base.max_size }

    fn set_bounds(&mut self, b: Rect) {
        // "Organize windows" reset: if the reset cell holds a target rect, jump to it.
        if let Some(ref cell) = self.reset_to {
            if let Some(new_b) = cell.get() {
                self.bounds = new_b;
                cell.set(None);
                return;
            }
        }
        // Preserve our position — only initialise from parent when bounds are zero.
        if self.bounds.width == 0.0 || self.bounds.height == 0.0 {
            self.bounds = b;
        }
        // Otherwise keep our self-managed floating position unchanged.
    }

    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn hit_test(&self, local_pos: Point) -> bool {
        if !self.visible { return false; }
        // Keep capturing during drag even when cursor leaves.
        if self.dragging { return true; }
        let b = self.bounds();
        local_pos.x >= 0.0 && local_pos.x <= b.width
            && local_pos.y >= 0.0 && local_pos.y <= b.height
    }

    fn layout(&mut self, _available: Size) -> Size {
        if !self.visible {
            return Size::new(self.bounds.width, self.bounds.height);
        }
        let content_h = (self.bounds.height - TITLE_H).max(0.0);
        if let Some(child) = self.children.first_mut() {
            child.layout(Size::new(self.bounds.width, content_h));
            // Content sits at the bottom of the window (Y-up: y=0).
            child.set_bounds(Rect::new(0.0, 0.0, self.bounds.width, content_h));
        }
        Size::new(self.bounds.width, self.bounds.height)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        if !self.visible { return; }

        let v = ctx.visuals();
        let w = self.bounds.width;
        let h = self.bounds.height;
        let tb = self.title_bar_bottom();

        // Shadow (painted slightly offset and larger, semi-transparent).
        ctx.set_fill_color(v.window_shadow);
        ctx.begin_path();
        ctx.rounded_rect(
            SHADOW_BLUR, -SHADOW_BLUR,
            w + SHADOW_BLUR, h + SHADOW_BLUR,
            CORNER_R,
        );
        ctx.fill();

        // Window body background (content area).
        ctx.set_fill_color(v.window_fill);
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, w, h, CORNER_R);
        ctx.fill();

        // Title bar.
        let bar_color = if self.dragging {
            v.window_title_fill_drag
        } else {
            v.window_title_fill
        };
        ctx.set_fill_color(bar_color);
        ctx.begin_path();
        // Draw only the top-rounded portion for the title bar.
        // We paint a full rounded rect then cover the bottom corners with a plain rect.
        ctx.rounded_rect(0.0, tb, w, TITLE_H, CORNER_R);
        ctx.fill();
        // Square off the bottom edge of the title bar.
        ctx.set_fill_color(bar_color);
        ctx.begin_path();
        ctx.rect(0.0, tb, w, CORNER_R);
        ctx.fill();

        // Thin separator line between title bar and content.
        ctx.set_fill_color(v.window_stroke);
        ctx.begin_path();
        ctx.rect(0.0, tb - 1.0, w, 1.0);
        ctx.fill();

        // Title text.
        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(self.font_size);
        ctx.set_fill_color(v.window_title_text);
        let title_cy = tb + TITLE_H * 0.5;
        if let Some(m) = ctx.measure_text(&self.title) {
            let tx = 12.0;
            let ty = title_cy - (m.ascent - m.descent) * 0.5 + m.descent;
            ctx.fill_text(&self.title, tx, ty);
        }

        // Close button.
        let cc = self.close_center();
        let close_bg = if self.close_hovered {
            v.window_close_bg_hovered
        } else {
            v.window_close_bg
        };
        ctx.set_fill_color(close_bg);
        ctx.begin_path();
        ctx.circle(cc.x, cc.y, CLOSE_R);
        ctx.fill();

        // × glyph on close button.
        let arm = CLOSE_R * 0.5;
        ctx.set_stroke_color(v.window_close_fg);
        ctx.set_line_width(1.5);
        ctx.begin_path();
        ctx.move_to(cc.x - arm, cc.y - arm);
        ctx.line_to(cc.x + arm, cc.y + arm);
        ctx.stroke();
        ctx.begin_path();
        ctx.move_to(cc.x + arm, cc.y - arm);
        ctx.line_to(cc.x - arm, cc.y + arm);
        ctx.stroke();

        // Thin border around the whole window.
        ctx.set_stroke_color(v.window_stroke);
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, w, h, CORNER_R);
        ctx.stroke();
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        if !self.is_visible() { return EventResult::Ignored; }

        match event {
            Event::MouseMove { pos } => {
                self.close_hovered = self.in_close_button(*pos);

                if self.dragging {
                    // Derive world position from local pos + current bounds.
                    let world = Point::new(pos.x + self.bounds.x, pos.y + self.bounds.y);
                    let dx = world.x - self.drag_start_world.x;
                    let dy = world.y - self.drag_start_world.y;
                    self.bounds.x = self.drag_start_bounds.x + dx;
                    self.bounds.y = self.drag_start_bounds.y + dy;
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }

            Event::MouseDown { button: MouseButton::Left, pos, .. } => {
                if self.in_close_button(*pos) {
                    // Close button: hide the window, sync the shared cell, and fire callback.
                    self.visible = false;
                    if let Some(ref cell) = self.visible_cell { cell.set(false); }
                    if let Some(cb) = self.on_close.as_mut() { cb(); }
                    return EventResult::Consumed;
                }
                if self.in_title_bar(*pos) {
                    self.dragging = true;
                    self.drag_start_world = Point::new(
                        pos.x + self.bounds.x,
                        pos.y + self.bounds.y,
                    );
                    self.drag_start_bounds = self.bounds;
                    return EventResult::Consumed;
                }
                // Click on content area: consume so it doesn't fall through.
                EventResult::Consumed
            }

            Event::MouseUp { button: MouseButton::Left, .. } => {
                let was_dragging = self.dragging;
                self.dragging = false;
                if was_dragging { EventResult::Consumed } else { EventResult::Ignored }
            }

            _ => EventResult::Ignored,
        }
    }
}
