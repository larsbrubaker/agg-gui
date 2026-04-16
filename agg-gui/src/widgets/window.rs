//! `Window` — a floating, draggable, resizable panel with a title bar.
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
//! # Features
//!
//! - **Drag** — click-drag the title bar to move the window.
//! - **Resize** — drag any of the 8 edges/corners to resize; min size 120×80.
//! - **Collapse** — double-click the title bar to collapse to title-bar-only height.
//! - **Close** — click the × button; syncs with an optional shared `visible_cell`.
//!
//! # Coordinate notes (Y-up)
//!
//! `bounds` stores the window's position in its **parent's** coordinate space.
//! The title bar is at the **top** of the window, i.e. local Y ∈
//! `[height − TITLE_H .. height]`. The content area fills local Y ∈ `[0 .. height − TITLE_H]`.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use web_time::Instant;

use crate::cursor::{CursorIcon, set_cursor_icon};
use crate::event::{Event, EventResult, MouseButton};
use crate::geometry::{Point, Rect, Size};
use crate::draw_ctx::DrawCtx;
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::text::Font;
use crate::widget::{Widget, paint_subtree};
use crate::widgets::label::Label;

/// Round all four components of a Rect to the nearest integer so widgets
/// are always placed on exact pixel boundaries (crisp bitmap blits, no blur).
fn snap(r: Rect) -> Rect {
    Rect::new(r.x.round(), r.y.round(), r.width.round(), r.height.round())
}

const TITLE_H:      f64 = 28.0;
const CORNER_R:     f64 = 8.0;
const SHADOW_BLUR:  f64 = 6.0;
const CLOSE_R:      f64 = 6.0;
const CLOSE_PAD:    f64 = 10.0;
const RESIZE_EDGE:  f64 = 6.0;   // px from the edge that counts as a resize zone
const MIN_W:        f64 = 120.0;
const MIN_H:        f64 = 80.0;
const DBL_CLICK_MS: u128 = 500;  // double-click detection window

// ── Resize direction ───────────────────────────────────────────────────────────

/// Which edge(s) are being dragged during a resize operation.
#[derive(Clone, Copy, Debug, PartialEq)]
enum ResizeDir {
    N, NE, E, SE, S, SW, W, NW,
}

// ── Window state ───────────────────────────────────────────────────────────────

/// Interaction mode for the current drag.
#[derive(Clone, Copy, Debug, PartialEq)]
enum DragMode {
    None,
    Move,
    Resize(ResizeDir),
}

/// A floating panel with a draggable/resizable title bar and a single content child.
pub struct Window {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>, // always exactly 1: the content
    base: WidgetBase,

    font_size: f64,

    visible: bool,
    visible_cell: Option<Rc<Cell<bool>>>,
    reset_to: Option<Rc<Cell<Option<Rect>>>>,
    position_cell: Option<Rc<Cell<Rect>>>,

    collapsed: bool,
    /// Height before collapsing, so we can restore it.
    pre_collapse_h: f64,

    drag_mode: DragMode,
    /// Cursor world position when drag started.
    drag_start_world: Point,
    /// Window bounds when drag started.
    drag_start_bounds: Rect,

    close_hovered: bool,
    on_close: Option<Box<dyn FnMut()>>,

    /// Which resize edge/corner the cursor is currently hovering over.
    /// Cleared to None when the cursor moves into the interior.
    hover_dir: Option<ResizeDir>,

    /// Time of last left-click in the title bar — for double-click collapse.
    last_title_click: Option<Instant>,

    /// Backbuffered title label.  Positioned and painted manually in `paint()`.
    title_label: Label,

    /// Canvas size supplied by the last `layout()` call; used for clamping.
    canvas_size: Size,
    /// When true, the window is kept fully inside the canvas bounds during drag/resize.
    constrain: bool,
}

impl Window {
    /// Create a new window with the given title, font, and content widget.
    ///
    /// Default position: `(60, 60)` with `size = (360, 280)`. Call
    /// [`with_bounds`] to override.
    pub fn new(title: impl Into<String>, font: Arc<Font>, content: Box<dyn Widget>) -> Self {
        let font_size = 13.0;
        let title_str: String = title.into();
        let title_label = Label::new(&title_str, Arc::clone(&font))
            .with_font_size(font_size);
        Self {
            bounds: Rect::new(60.0, 60.0, 360.0, 280.0),
            children: vec![content],
            base: WidgetBase::new(),
            font_size,
            visible: true,
            visible_cell: None,
            reset_to: None,
            position_cell: None,
            collapsed: false,
            pre_collapse_h: 280.0,
            drag_mode: DragMode::None,
            drag_start_world: Point::ORIGIN,
            drag_start_bounds: Rect::default(),
            close_hovered: false,
            on_close: None,
            hover_dir: None,
            last_title_click: None,
            title_label,
            canvas_size: Size::new(1280.0, 720.0),
            constrain: true,
        }
    }

    pub fn with_bounds(mut self, b: Rect) -> Self {
        self.pre_collapse_h = b.height;
        self.bounds = b;
        self
    }
    pub fn with_font_size(mut self, size: f64) -> Self { self.font_size = size; self }

    pub fn with_visible_cell(mut self, cell: Rc<Cell<bool>>) -> Self {
        self.visible_cell = Some(cell);
        self
    }

    pub fn with_reset_cell(mut self, cell: Rc<Cell<Option<Rect>>>) -> Self {
        self.reset_to = Some(cell);
        self
    }

    pub fn with_position_cell(mut self, cell: Rc<Cell<Rect>>) -> Self {
        self.position_cell = Some(cell);
        self
    }

    pub fn with_margin(mut self, m: Insets)    -> Self { self.base.margin   = m; self }
    pub fn with_h_anchor(mut self, h: HAnchor) -> Self { self.base.h_anchor = h; self }
    pub fn with_v_anchor(mut self, v: VAnchor) -> Self { self.base.v_anchor = v; self }
    pub fn with_min_size(mut self, s: Size)    -> Self { self.base.min_size = s; self }
    pub fn with_max_size(mut self, s: Size)    -> Self { self.base.max_size = s; self }

    pub fn with_constrain(mut self, constrain: bool) -> Self { self.constrain = constrain; self }

    pub fn on_close(mut self, cb: impl FnMut() + 'static) -> Self {
        self.on_close = Some(Box::new(cb));
        self
    }

    fn clamp_to_canvas(&mut self) {
        if !self.constrain { return; }
        let cw = self.canvas_size.width;
        let ch = self.canvas_size.height;
        // bounds.height equals TITLE_H when collapsed (we adjust it on toggle),
        // so no special-case is needed here.
        self.bounds.x = self.bounds.x.clamp(0.0, (cw - self.bounds.width).max(0.0)).round();
        self.bounds.y = self.bounds.y.clamp(0.0, (ch - self.bounds.height).max(0.0)).round();
    }

    pub fn show(&mut self) { self.visible = true; }
    pub fn hide(&mut self) { self.visible = false; }
    pub fn toggle(&mut self) { self.visible = !self.visible; }
    pub fn is_visible(&self) -> bool { self.visible }

    fn title_bar_bottom(&self) -> f64 { self.bounds.height - TITLE_H }

    fn in_title_bar(&self, local: Point) -> bool {
        local.y >= self.title_bar_bottom() && local.y <= self.bounds.height
            && local.x >= 0.0 && local.x <= self.bounds.width
    }

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

    // ── Resize zone detection ──────────────────────────────────────────────────

    /// Return the resize direction for `local`, or `None` if the point is in
    /// the interior (or the window is collapsed).
    fn resize_dir(&self, local: Point) -> Option<ResizeDir> {
        if self.collapsed { return None; }
        let w = self.bounds.width;
        let h = self.bounds.height;
        let x = local.x;
        let y = local.y;

        // Outside the window altogether.
        if x < 0.0 || x > w || y < 0.0 || y > h { return None; }

        let on_n = y > h - RESIZE_EDGE;
        let on_s = y < RESIZE_EDGE;
        let on_w = x < RESIZE_EDGE;
        let on_e = x > w - RESIZE_EDGE;

        match (on_n, on_e, on_s, on_w) {
            (true,  true,  _,     _    ) => Some(ResizeDir::NE),
            (true,  _,     _,     true ) => Some(ResizeDir::NW),
            (_,     _,     true,  true ) => Some(ResizeDir::SW),
            (_,     true,  true,  _    ) => Some(ResizeDir::SE),
            (true,  _,     _,     _    ) => Some(ResizeDir::N),
            (_,     true,  _,     _    ) => Some(ResizeDir::E),
            (_,     _,     true,  _    ) => Some(ResizeDir::S),
            (_,     _,     _,     true ) => Some(ResizeDir::W),
            _                            => None,
        }
    }

    /// Apply a mouse-world-space delta to bounds according to the resize direction.
    fn apply_resize(&mut self, world_pos: Point) {
        let dx = world_pos.x - self.drag_start_world.x;
        let dy = world_pos.y - self.drag_start_world.y;
        let sb = self.drag_start_bounds;

        let (mut x, mut y, mut w, mut h) = (sb.x, sb.y, sb.width, sb.height);

        if let DragMode::Resize(dir) = self.drag_mode {
            match dir {
                ResizeDir::N  => { h = (sb.height + dy).max(MIN_H); }
                ResizeDir::S  => { y = sb.y + dy; h = (sb.height - dy).max(MIN_H); if h == MIN_H { y = sb.y + sb.height - MIN_H; } }
                ResizeDir::E  => { w = (sb.width  + dx).max(MIN_W); }
                ResizeDir::W  => { x = sb.x + dx; w = (sb.width  - dx).max(MIN_W); if w == MIN_W { x = sb.x + sb.width - MIN_W; } }
                ResizeDir::NE => { w = (sb.width  + dx).max(MIN_W); h = (sb.height + dy).max(MIN_H); }
                ResizeDir::NW => { x = sb.x + dx; w = (sb.width  - dx).max(MIN_W); if w == MIN_W { x = sb.x + sb.width - MIN_W; } h = (sb.height + dy).max(MIN_H); }
                ResizeDir::SE => { w = (sb.width  + dx).max(MIN_W); y = sb.y + dy; h = (sb.height - dy).max(MIN_H); if h == MIN_H { y = sb.y + sb.height - MIN_H; } }
                ResizeDir::SW => { x = sb.x + dx; w = (sb.width  - dx).max(MIN_W); if w == MIN_W { x = sb.x + sb.width - MIN_W; } y = sb.y + dy; h = (sb.height - dy).max(MIN_H); if h == MIN_H { y = sb.y + sb.height - MIN_H; } }
            }
        }

        self.bounds = snap(Rect::new(x, y, w, h));
        self.clamp_to_canvas();
    }
}

/// Map a resize direction to the appropriate OS cursor icon.
fn resize_cursor(dir: ResizeDir) -> CursorIcon {
    match dir {
        ResizeDir::N  => CursorIcon::ResizeNorth,
        ResizeDir::S  => CursorIcon::ResizeSouth,
        ResizeDir::E  => CursorIcon::ResizeEast,
        ResizeDir::W  => CursorIcon::ResizeWest,
        ResizeDir::NE => CursorIcon::ResizeNorthEast,
        ResizeDir::NW => CursorIcon::ResizeNorthWest,
        ResizeDir::SE => CursorIcon::ResizeSouthEast,
        ResizeDir::SW => CursorIcon::ResizeSouthWest,
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
        if let Some(ref cell) = self.reset_to {
            if let Some(new_b) = cell.get() {
                self.bounds = new_b;
                self.pre_collapse_h = new_b.height;
                self.collapsed = false;
                cell.set(None);
                return;
            }
        }
        if self.bounds.width == 0.0 || self.bounds.height == 0.0 {
            self.bounds = b;
            self.pre_collapse_h = b.height;
        }
    }

    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    /// Clip child painting to the content area (below the title bar).
    /// When collapsed bounds.height == TITLE_H so the content rect has zero height,
    /// preventing any child from drawing outside the visible title-bar strip.
    fn clip_children_rect(&self) -> Option<(f64, f64, f64, f64)> {
        if !self.is_visible() { return None; }
        let w = self.bounds.width;
        let content_h = (self.bounds.height - TITLE_H).max(0.0);
        // Clip to content area: y=0 (bottom) up to content_h, full width.
        Some((0.0, 0.0, w, content_h))
    }

    fn hit_test(&self, local_pos: Point) -> bool {
        if !self.is_visible() { return false; }
        if self.drag_mode != DragMode::None { return true; }
        let b = self.bounds();
        local_pos.x >= 0.0 && local_pos.x <= b.width
            && local_pos.y >= 0.0 && local_pos.y <= b.height
    }

    fn layout(&mut self, available: Size) -> Size {
        if !self.is_visible() {
            return Size::new(self.bounds.width, self.bounds.height);
        }
        // When collapsed, bounds.height == TITLE_H (set during toggle).
        let content_h = (self.bounds.height - TITLE_H).max(0.0);

        if let Some(child) = self.children.first_mut() {
            if !self.collapsed {
                child.layout(Size::new(self.bounds.width, content_h));
                child.set_bounds(Rect::new(0.0, 0.0, self.bounds.width, content_h));
            }
            // When collapsed the child keeps its last bounds but is not visible
            // because hit_test returns false for the content area.
        }

        // Layout the title label so its intrinsic size is known before paint().
        let s = self.title_label.layout(Size::new(self.bounds.width - 48.0, TITLE_H));
        self.title_label.set_bounds(Rect::new(0.0, 0.0, s.width, s.height));

        // Store canvas size for drag clamping, apply passive constraint first,
        // then publish the clamped position so persistence gets the real location.
        self.canvas_size = available;
        self.clamp_to_canvas();
        if let Some(ref cell) = self.position_cell {
            cell.set(self.bounds);
        }

        Size::new(self.bounds.width, self.bounds.height)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        if !self.is_visible() { return; }

        let v  = ctx.visuals();
        let w  = self.bounds.width;
        // bounds.height == TITLE_H when collapsed (adjusted on toggle).
        let h  = self.bounds.height;
        let tb = h - TITLE_H;

        // Shadow.
        ctx.set_fill_color(v.window_shadow);
        ctx.begin_path();
        ctx.rounded_rect(SHADOW_BLUR, -SHADOW_BLUR, w + SHADOW_BLUR, h + SHADOW_BLUR, CORNER_R);
        ctx.fill();

        // Window body.
        ctx.set_fill_color(v.window_fill);
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, w, h, CORNER_R);
        ctx.fill();

        // Title bar background.
        let dragging = self.drag_mode == DragMode::Move;
        let bar_color = if dragging { v.window_title_fill_drag } else { v.window_title_fill };
        ctx.set_fill_color(bar_color);
        ctx.begin_path();
        ctx.rounded_rect(0.0, tb, w, TITLE_H, CORNER_R);
        ctx.fill();
        // Square off the bottom corners of the title bar (only when not collapsed).
        if !self.collapsed {
            ctx.set_fill_color(bar_color);
            ctx.begin_path();
            ctx.rect(0.0, tb, w, CORNER_R);
            ctx.fill();
        }

        // Separator between title bar and content.
        if !self.collapsed {
            ctx.set_fill_color(v.window_stroke);
            ctx.begin_path();
            ctx.rect(0.0, tb - 1.0, w, 1.0);
            ctx.fill();
        }

        // Collapse indicator — small chevron on the left of the title bar.
        let chev_x = 12.0;
        let chev_cy = tb + TITLE_H * 0.5;
        let chev_sz = 4.0;
        ctx.set_stroke_color(v.window_title_text);
        ctx.set_line_width(1.5);
        ctx.begin_path();
        if self.collapsed {
            // ▶ pointing right (collapsed state).
            ctx.move_to(chev_x,            chev_cy - chev_sz);
            ctx.line_to(chev_x + chev_sz,  chev_cy);
            ctx.line_to(chev_x,            chev_cy + chev_sz);
        } else {
            // ▼ pointing down (expanded state).
            ctx.move_to(chev_x - chev_sz,  chev_cy - chev_sz * 0.5);
            ctx.line_to(chev_x,            chev_cy + chev_sz * 0.5);
            ctx.line_to(chev_x + chev_sz,  chev_cy - chev_sz * 0.5);
        }
        ctx.stroke();

        // Title text — rendered through backbuffered Label.
        self.title_label.set_color(v.window_title_text);
        let title_lw = self.title_label.bounds().width;
        let title_lh = self.title_label.bounds().height;
        let title_lx = 24.0; // leave room for chevron
        let title_ly = tb + (TITLE_H - title_lh) * 0.5;
        self.title_label.set_bounds(Rect::new(title_lx, title_ly, title_lw, title_lh));
        ctx.save();
        ctx.translate(title_lx, title_ly);
        paint_subtree(&mut self.title_label, ctx);
        ctx.restore();

        // Close button.
        let cc = self.close_center();
        let close_bg = if self.close_hovered { v.window_close_bg_hovered } else { v.window_close_bg };
        ctx.set_fill_color(close_bg);
        ctx.begin_path();
        ctx.circle(cc.x, cc.y, CLOSE_R);
        ctx.fill();

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

        // Outer border.
        ctx.set_stroke_color(v.window_stroke);
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, w, h, CORNER_R);
        ctx.stroke();

    }

    // paint_overlay: draws the resize handle dots + edge highlights on top of content.
    fn paint_overlay(&mut self, ctx: &mut dyn DrawCtx) {
        if !self.is_visible() || self.collapsed { return; }
        let v = ctx.visuals();
        let w = self.bounds.width;
        let h = self.bounds.height;

        // ── SE corner drag grip (3 diagonal lines, egui-style) ───────────────
        // Highlight when SE is hovered or actively being dragged.
        let is_se_active = matches!(self.drag_mode, DragMode::Resize(ResizeDir::SE));
        let is_se_hover  = self.hover_dir == Some(ResizeDir::SE);
        let grip_color = if is_se_active {
            v.window_resize_active
        } else if is_se_hover {
            v.window_resize_hover
        } else {
            v.window_stroke
        };
        ctx.set_stroke_color(grip_color);
        ctx.set_line_width(1.5);
        let m = 3.0_f64; // margin from corner edge
        for i in 1..=3_i32 {
            let off = i as f64 * 4.0 + m;
            ctx.begin_path();
            ctx.move_to(w - off, m);
            ctx.line_to(w - m, off);
            ctx.stroke();
        }

        // ── Resize edge / corner highlight ────────────────────────────────────
        // Determine the highlighted direction and whether it is actively dragging.
        let (highlight, is_active) = match self.drag_mode {
            DragMode::Resize(d) => (Some(d), true),
            DragMode::Move      => (None, false), // no edge highlight while moving
            DragMode::None      => (self.hover_dir, false),
        };
        let dir = match highlight { Some(d) => d, None => return };

        let color = if is_active { v.window_resize_active } else { v.window_resize_hover };
        ctx.set_stroke_color(color);
        ctx.set_line_width(2.0);

        // Which edges to highlight (derived from direction).
        let (top, bottom, left, right) = match dir {
            ResizeDir::N  => (true,  false, false, false),
            ResizeDir::S  => (false, true,  false, false),
            ResizeDir::E  => (false, false, false, true),
            ResizeDir::W  => (false, false, true,  false),
            ResizeDir::NE => (true,  false, false, true),
            ResizeDir::NW => (true,  false, true,  false),
            ResizeDir::SE => (false, true,  false, true),
            ResizeDir::SW => (false, true,  true,  false),
        };

        // Segments run between the rounded-corner tangent points.
        let cr = CORNER_R;
        if top {
            ctx.begin_path();
            ctx.move_to(cr, h);
            ctx.line_to(w - cr, h);
            ctx.stroke();
        }
        if bottom {
            ctx.begin_path();
            ctx.move_to(cr, 0.0);
            ctx.line_to(w - cr, 0.0);
            ctx.stroke();
        }
        if left {
            ctx.begin_path();
            ctx.move_to(0.0, cr);
            ctx.line_to(0.0, h - cr);
            ctx.stroke();
        }
        if right {
            ctx.begin_path();
            ctx.move_to(w, cr);
            ctx.line_to(w, h - cr);
            ctx.stroke();
        }
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        if !self.is_visible() { return EventResult::Ignored; }

        match event {
            Event::MouseMove { pos } => {
                self.close_hovered = self.in_close_button(*pos);

                match self.drag_mode {
                    DragMode::Move => {
                        let world = Point::new(pos.x + self.bounds.x, pos.y + self.bounds.y);
                        let dx = world.x - self.drag_start_world.x;
                        let dy = world.y - self.drag_start_world.y;
                        self.bounds.x = (self.drag_start_bounds.x + dx).round();
                        self.bounds.y = (self.drag_start_bounds.y + dy).round();
                        self.clamp_to_canvas();
                        self.hover_dir = None;
                        set_cursor_icon(CursorIcon::Grabbing);
                        return EventResult::Consumed;
                    }
                    DragMode::Resize(dir) => {
                        let world = Point::new(pos.x + self.bounds.x, pos.y + self.bounds.y);
                        self.apply_resize(world);
                        set_cursor_icon(resize_cursor(dir));
                        return EventResult::Consumed;
                    }
                    DragMode::None => {
                        // Track which edge/corner the cursor is hovering over so
                        // paint_overlay can draw the appropriate highlight.
                        self.hover_dir = self.resize_dir(*pos);
                        if let Some(dir) = self.hover_dir {
                            set_cursor_icon(resize_cursor(dir));
                        }
                    }
                }
                EventResult::Ignored
            }

            Event::MouseDown { button: MouseButton::Left, pos, .. } => {
                // Close button — highest priority.
                if self.in_close_button(*pos) {
                    self.visible = false;
                    if let Some(ref cell) = self.visible_cell { cell.set(false); }
                    if let Some(cb) = self.on_close.as_mut() { cb(); }
                    return EventResult::Consumed;
                }

                // Resize edge — check before title bar to handle corner overlap.
                if let Some(dir) = self.resize_dir(*pos) {
                    // Only start resize if not in the close button area and not a pure title bar drag.
                    // The N edge overlaps the title bar — prefer resize over drag from the top N px.
                    let world = Point::new(pos.x + self.bounds.x, pos.y + self.bounds.y);
                    self.drag_mode = DragMode::Resize(dir);
                    self.drag_start_world  = world;
                    self.drag_start_bounds = self.bounds;
                    return EventResult::Consumed;
                }

                // Title bar drag + double-click collapse.
                if self.in_title_bar(*pos) {
                    // Double-click detection.
                    let now = Instant::now();
                    let is_double = self.last_title_click
                        .map(|t| now.duration_since(t).as_millis() < DBL_CLICK_MS)
                        .unwrap_or(false);

                    if is_double {
                        // Toggle collapse.
                        // We adjust bounds.y so the title bar (top edge) stays fixed.
                        // In Y-up: top = bounds.y + bounds.height.
                        if self.collapsed {
                            // Expanding: restore full height, keep top edge in place.
                            let top = self.bounds.y + self.bounds.height;
                            self.bounds.height = self.pre_collapse_h;
                            self.bounds.y = (top - self.pre_collapse_h).round();
                            self.collapsed = false;
                        } else {
                            // Collapsing: shrink to title-bar only, keep top edge in place.
                            let top = self.bounds.y + self.bounds.height;
                            self.pre_collapse_h = self.bounds.height;
                            self.bounds.height = TITLE_H;
                            self.bounds.y = (top - TITLE_H).round();
                            self.collapsed = true;
                        }
                        self.clamp_to_canvas();
                        self.last_title_click = None;
                    } else {
                        self.last_title_click = Some(now);
                        let world = Point::new(pos.x + self.bounds.x, pos.y + self.bounds.y);
                        self.drag_mode = DragMode::Move;
                        self.drag_start_world  = world;
                        self.drag_start_bounds = self.bounds;
                    }
                    return EventResult::Consumed;
                }

                // Click on content area: consume so it doesn't fall through.
                if !self.collapsed {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }

            Event::MouseUp { button: MouseButton::Left, .. } => {
                let was_dragging = self.drag_mode != DragMode::None;
                self.drag_mode = DragMode::None;
                if was_dragging { EventResult::Consumed } else { EventResult::Ignored }
            }

            _ => EventResult::Ignored,
        }
    }
}
