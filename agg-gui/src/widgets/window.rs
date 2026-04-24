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
//! - **Collapse** — click the chevron on the left of the title bar to collapse
//!   to title-bar-only height (click again to expand).
//! - **Maximize** — double-click the title bar (or click the maximize button)
//!   to toggle between maximised and restored size.
//! - **Close** — click the × button; syncs with an optional shared `visible_cell`.
//!
//! # Coordinate notes (Y-up)
//!
//! `bounds` stores the window's position in its **parent's** coordinate space.
//! The title bar is at the **top** of the window, i.e. local Y ∈
//! `[height − TITLE_H .. height]`. The content area fills local Y ∈ `[0 .. height − TITLE_H]`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use web_time::Instant;

use crate::color::Color;
use crate::cursor::{CursorIcon, set_cursor_icon};
use crate::event::{Event, EventResult, MouseButton};
use crate::geometry::{Point, Rect, Size};
use crate::draw_ctx::DrawCtx;
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::text::Font;
use crate::widget::{Widget, paint_subtree};
use crate::widgets::window_title_bar::{TitleBarView, WindowTitleBar};

/// Round all four components of a Rect to the nearest integer so widgets
/// are always placed on exact pixel boundaries (crisp bitmap blits, no blur).
fn snap(r: Rect) -> Rect {
    Rect::new(r.x.round(), r.y.round(), r.width.round(), r.height.round())
}

const TITLE_H:      f64 = 28.0;
const CORNER_R:     f64 = 8.0;
/// Shadow blur radius in pixels (egui default Shadow::blur is ≈16; we use 14
/// for a slightly tighter falloff since windows live on a panel background).
const SHADOW_BLUR:  f64 = 14.0;
/// Shadow offset from the window (Y-down visually → −y in Y-up space).
const SHADOW_DX:    f64 = 2.0;
const SHADOW_DY:    f64 = 6.0;
/// Number of stacked layers approximating a Gaussian blur falloff.
const SHADOW_STEPS: usize = 10;
const CLOSE_R:      f64 = 6.0;
const CLOSE_PAD:    f64 = 10.0;
/// Horizontal distance from the right edge to the maximize button centre.
/// = CLOSE_PAD + CLOSE_R*2 + 4 px gap
const MAX_PAD:      f64 = CLOSE_PAD + CLOSE_R * 2.0 + 4.0; // 26 px
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

    /// Snapshot of `is_visible()` from the previous `layout()` call.  Used
    /// to detect the false→true transition (demo toggled on in the
    /// sidebar) so we can request the parent `Stack` raise us to the top.
    last_visible: Cell<bool>,
    /// Set to `true` on a visibility rising edge; read + cleared by
    /// `take_raise_request` on the next parent-layout pass.
    raise_request: Cell<bool>,

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

    /// Whether the window is currently maximized (fills the full canvas).
    maximized: bool,
    /// Bounds saved before maximizing so we can restore them.
    pre_maximize_bounds: Rect,
    maximize_hovered: bool,

    /// Which resize edge/corner the cursor is currently hovering over.
    /// Cleared to None when the cursor moves into the interior.
    hover_dir: Option<ResizeDir>,

    /// Time of last left-click in the title bar — for double-click collapse.
    last_title_click: Option<Instant>,

    /// Title-bar sub-widget — owns the bar fill, separator, chevron,
    /// title label, maximize/close buttons.  Painted manually from
    /// `paint()` so `clip_children_rect` can keep content clipped to the
    /// body area.  Display state is written into `title_state` every
    /// layout pass; the sub-widget reads it at paint time.
    title_bar:   WindowTitleBar,
    title_state: Rc<RefCell<TitleBarView>>,

    /// Canvas size supplied by the last `layout()` call; used for clamping.
    canvas_size: Size,
    /// When true, the window is kept fully inside the canvas bounds during drag/resize.
    constrain: bool,

    /// When true, the window bounds adopt the content's preferred size each
    /// layout pass (width + height).  Keeps the title-bar top edge pinned so
    /// the window appears to grow/shrink downward.  User resize is disabled
    /// while auto-size is active (dragging still works).
    auto_size: bool,

    /// Whether the user can resize the window by dragging its edges.  When
    /// `false`, no resize handles are active regardless of `resizable_h` /
    /// `resizable_v` — matches egui's `.resizable(false)`.  Defaults to
    /// `true` to preserve existing behaviour for call sites that don't
    /// explicitly opt out.
    resizable: bool,
    /// Fine-grained axis control.  Both default to `true`; setting just
    /// one to `false` produces an egui `.resizable([true, false])`-style
    /// uni-axis resizable window.  Only consulted when `resizable` is
    /// `true`.
    resizable_h: bool,
    resizable_v: bool,
    /// Content-bound resize floor + ceiling.  When `true`, the
    /// window's height is locked to its content's required height
    /// each layout (snap pre-pass) AND `apply_resize` refuses to
    /// drag it smaller than content.  Matches egui's no-scroll-no-
    /// clip-no-whitespace W4 contract.  Off by default.
    tight_content_fit: bool,
    /// Floor-only variant of [`tight_content_fit`].  Same minimum-
    /// height enforcement, but allows the user to grow the window
    /// past the content (whitespace below).  Used by W5 where a
    /// `TextArea` flex-fills extra space and the user can pull the
    /// window taller than the wrapped text.  Off by default.
    floor_content_height: bool,
    /// Most recently observed content required height (via
    /// `Widget::measure_min_height`).  Updated each layout pass so
    /// `apply_resize` and the tight-fit pre-pass see a current value
    /// even when the content tree contains a flex-fill widget.
    last_content_natural_h: Cell<f64>,

    /// Window title string — stored so external callers (z-order
    /// persistence, inspector display, etc.) can identify this window
    /// without going through the inner `title_bar` sub-widget.
    title: String,
    /// Optional callback invoked whenever this window requests a raise
    /// (click-to-front or visibility rising-edge from the sidebar).
    /// Receives the window title.  Used by the demo's z-order tracker
    /// to record "most recently raised" so the stacking order survives
    /// a save/restore round-trip.
    on_raised: Option<Box<dyn FnMut(&str)>>,
}

impl Window {
    /// Create a new window with the given title, font, and content widget.
    ///
    /// Default position: `(60, 60)` with `size = (360, 280)`. Call
    /// [`with_bounds`] to override.
    pub fn new(title: impl Into<String>, font: Arc<Font>, content: Box<dyn Widget>) -> Self {
        let font_size = 13.0;
        let title_str: String = title.into();
        let title_state = Rc::new(RefCell::new(TitleBarView::default_visuals()));
        let title_bar = WindowTitleBar::new(&title_str, Arc::clone(&font), Rc::clone(&title_state));
        Self {
            bounds: Rect::new(60.0, 60.0, 360.0, 280.0),
            children: vec![content],
            base: WidgetBase::new(),
            font_size,
            visible: true,
            visible_cell: None,
            reset_to: None,
            position_cell: None,
            // Seed `last_visible` to `true` (matches `visible` above) so a
            // window that's open on first frame doesn't spuriously request
            // a raise before the user has interacted with it.
            last_visible: Cell::new(true),
            raise_request: Cell::new(false),
            collapsed: false,
            pre_collapse_h: 280.0,
            drag_mode: DragMode::None,
            drag_start_world: Point::ORIGIN,
            drag_start_bounds: Rect::default(),
            close_hovered: false,
            on_close: None,
            maximized: false,
            pre_maximize_bounds: Rect::new(60.0, 60.0, 360.0, 280.0),
            maximize_hovered: false,
            hover_dir: None,
            last_title_click: None,
            title_bar,
            title_state,
            // Seed as "unknown" so `layout()`'s shrink-detect guard
            // (`had_prior = prev.w > 0 && prev.h > 0`) correctly skips the
            // clamp on the very first layout pass.  The old default
            // `(1280, 720)` was treated as prior, so the first-frame
            // transition from 1280×720 → <smaller> incorrectly looked like
            // an OS-window shrink and pulled saved Y-up positions down into
            // the transient canvas.  Real-value `canvas_size` is populated
            // by `layout()` before any drag/resize/collapse hit-test runs.
            canvas_size: Size::new(0.0, 0.0),
            constrain: true,
            auto_size: false,
            resizable: true,
            resizable_h: true,
            resizable_v: true,
            tight_content_fit: false,
            floor_content_height: false,
            last_content_natural_h: Cell::new(0.0),
            title: title_str,
            on_raised: None,
        }
    }

    /// Returns the window title as it was passed to [`Window::new`].
    pub fn title(&self) -> &str { &self.title }

    /// Register a callback fired whenever this window requests a raise
    /// (click-to-front or visibility rising-edge from the sidebar).
    /// Receives the window title.  The demo uses this to feed a shared
    /// z-order tracker that gets persisted to disk.
    pub fn on_raised(mut self, cb: impl FnMut(&str) + 'static) -> Self {
        self.on_raised = Some(Box::new(cb));
        self
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

    /// Make the window size itself to the content's preferred size every frame.
    /// Top-left pin: as content grows/shrinks, the title bar stays where it is.
    pub fn with_auto_size(mut self, auto: bool) -> Self { self.auto_size = auto; self }

    /// Toggle user-dragged resize.  `false` hides every edge/corner handle
    /// and disables resize hit-tests.  Default: `true`.  Matches egui's
    /// `Window::resizable(bool)`.
    pub fn with_resizable(mut self, on: bool) -> Self { self.resizable = on; self }

    /// Fine-grained axis-locking of the resize handles — pass `(true, false)`
    /// for a horizontally-only resizable window, etc.  Implies
    /// `with_resizable(true)`.  Matches egui's `Window::resizable([h, v])`.
    pub fn with_resizable_axes(mut self, h: bool, v: bool) -> Self {
        self.resizable   = h || v;
        self.resizable_h = h;
        self.resizable_v = v;
        self
    }

    /// Lock the window's height to its content's required height.
    /// The user can grab a vertical resize handle but the next
    /// layout snaps back — egui's W4 "no scroll, no clip, no
    /// whitespace" contract.  Requires the content tree to expose
    /// its required height via [`Widget::measure_min_height`]; our
    /// `FlexColumn`, `Label`, `TextArea`, and `Container::with_fit_height`
    /// all do.
    pub fn with_tight_content_fit(mut self, on: bool) -> Self {
        self.tight_content_fit = on;
        self
    }

    /// Floor-only variant of [`with_tight_content_fit`]: refuses to
    /// shrink past content but allows the user to pull the window
    /// taller (whitespace below).  Used for windows whose content
    /// includes a flex-fill child like a multiline `TextArea` —
    /// matches egui's W5 where the TextEdit fills extra height and
    /// the user can grow the window further.
    pub fn with_height_floor_to_content(mut self, on: bool) -> Self {
        self.floor_content_height = on;
        self
    }

    /// Wrap the window's content in a built-in vertical [`ScrollView`].
    /// Matches egui's `Window::vscroll(true)`: lets the user shrink the
    /// window below content height without the caller having to wrap the
    /// content in a `ScrollView` manually.  Eager — happens at builder
    /// time so the rest of the layout / event / paint paths see a single
    /// child as usual.  Has no effect when called with `false` (matches
    /// the default).
    ///
    /// Don't combine with [`with_auto_size`]: the ScrollView claims its
    /// full available height, which would make auto-sizing grow the
    /// window to the canvas.  egui's demo never combines the two flags
    /// either.
    pub fn with_vscroll(mut self, vscroll: bool) -> Self {
        if vscroll {
            if let Some(content) = self.children.pop() {
                let scroll = crate::widgets::ScrollView::new(content)
                    .vertical(true)
                    .horizontal(false);
                self.children.push(Box::new(scroll));
            }
        }
        self
    }

    pub fn on_close(mut self, cb: impl FnMut() + 'static) -> Self {
        self.on_close = Some(Box::new(cb));
        self
    }

    fn clamp_to_canvas(&mut self) {
        if !self.constrain { return; }
        let cw = self.canvas_size.width;
        let ch = self.canvas_size.height;
        // **Policy: keep the TITLE BAR grabbable**, not the whole window.
        // Horizontally we keep at least `MIN_H_VISIBLE` pixels of the title
        // bar inside the canvas so the user can always drag the window back
        // on-screen.  Vertically (Y-up) we keep the FULL title bar inside
        // the canvas — the body may extend above/below, but the drag handle
        // is always fully reachable.  This matches how native OS window
        // managers constrain child windows against their host monitor.
        const MIN_H_VISIBLE: f64 = 40.0;

        let min_x = MIN_H_VISIBLE - self.bounds.width;
        let max_x = (cw - MIN_H_VISIBLE).max(min_x);
        self.bounds.x = self.bounds.x.clamp(min_x, max_x).round();

        // Title bar Y range in parent coords: [bounds.y + h - TITLE_H, bounds.y + h].
        // Full title bar visible → `bounds.y >= TITLE_H - h` AND `bounds.y <= ch - h`.
        // `bounds.height` collapses to `TITLE_H` when the user folds the
        // window, so the collapsed case naturally falls out of the same math.
        let min_y = TITLE_H - self.bounds.height;
        let max_y = (ch - self.bounds.height).max(min_y);
        self.bounds.y = self.bounds.y.clamp(min_y, max_y).round();
    }

    pub fn show(&mut self) { self.visible = true; }
    pub fn hide(&mut self) { self.visible = false; }
    pub fn toggle(&mut self) { self.visible = !self.visible; }
    /// Current visibility — honours an optional shared `visible_cell` when
    /// wired (sidebar toggles, programmatic show/hide).  The inherent
    /// `self.visible` field is a fallback for windows that aren't wired to
    /// a cell.  Must match the Widget-trait impl below so rising-edge
    /// detection in `layout()` observes sidebar toggles.
    pub fn is_visible(&self) -> bool {
        if let Some(ref cell) = self.visible_cell { cell.get() } else { self.visible }
    }

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

    fn maximize_center(&self) -> Point {
        Point::new(
            self.bounds.width - MAX_PAD,
            self.bounds.height - TITLE_H * 0.5,
        )
    }

    fn in_maximize_button(&self, local: Point) -> bool {
        let c = self.maximize_center();
        let dx = local.x - c.x;
        let dy = local.y - c.y;
        dx * dx + dy * dy <= (CLOSE_R + 3.0) * (CLOSE_R + 3.0)
    }

    /// Hit-box for the collapse / expand chevron on the LEFT of the title bar.
    /// Kept in sync with the paint geometry in
    /// `WindowTitleBar::paint` (chevron at `x = 12`, half-size 4).  A padded
    /// square around that point gives users a click target big enough to
    /// hit without pixel precision.
    fn in_chevron_button(&self, local: Point) -> bool {
        let cx = 12.0;
        let cy = self.bounds.height - TITLE_H * 0.5;
        let half = 8.0;
        local.x >= cx - half && local.x <= cx + half
            && local.y >= cy - half && local.y <= cy + half
    }

    /// Toggle collapsed <-> expanded, keeping the top edge of the window
    /// fixed in place.  Factored out of the event path so both the chevron
    /// click and any future keyboard shortcut go through the same math.
    fn toggle_collapse(&mut self) {
        let top = self.bounds.y + self.bounds.height;
        if self.collapsed {
            self.bounds.height = self.pre_collapse_h;
            self.bounds.y = (top - self.pre_collapse_h).round();
            self.collapsed = false;
        } else {
            self.pre_collapse_h = self.bounds.height;
            self.bounds.height = TITLE_H;
            self.bounds.y = (top - TITLE_H).round();
            self.collapsed = true;
        }
        self.clamp_to_canvas();
    }

    fn toggle_maximize(&mut self) {
        if self.maximized {
            self.bounds = self.pre_maximize_bounds;
            self.maximized = false;
        } else {
            self.pre_maximize_bounds = self.bounds;
            self.bounds = snap(Rect::new(
                0.0, 0.0,
                self.canvas_size.width,
                self.canvas_size.height,
            ));
            self.maximized = true;
        }
    }

    // ── Resize zone detection ──────────────────────────────────────────────────

    /// Return the resize direction for `local`, or `None` if the point is in
    /// the interior (or the window is collapsed).
    fn resize_dir(&self, local: Point) -> Option<ResizeDir> {
        if self.collapsed || self.auto_size { return None; }
        if !self.resizable { return None; }
        let w = self.bounds.width;
        let h = self.bounds.height;
        let x = local.x;
        let y = local.y;

        // Outside the window altogether.
        if x < 0.0 || x > w || y < 0.0 || y > h { return None; }

        // Mask each edge to the axes the window is allowed to resize on.
        let on_n = self.resizable_v && y > h - RESIZE_EDGE;
        let on_s = self.resizable_v && y < RESIZE_EDGE;
        let on_w = self.resizable_h && x < RESIZE_EDGE;
        let on_e = self.resizable_h && x > w - RESIZE_EDGE;

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

    /// Effective minimum height for this resize pass.  Honours
    /// either `tight_content_fit` (lock + floor) or
    /// `floor_content_height` (floor only) so a window whose content
    /// has a natural height > MIN_H can never be dragged smaller
    /// than its content.
    fn effective_min_h(&self) -> f64 {
        if self.tight_content_fit || self.floor_content_height {
            let content_min = self.last_content_natural_h.get() + TITLE_H;
            MIN_H.max(content_min)
        } else {
            MIN_H
        }
    }

    /// Apply a mouse-world-space delta to bounds according to the resize direction.
    fn apply_resize(&mut self, world_pos: Point) {
        let dx = world_pos.x - self.drag_start_world.x;
        let dy = world_pos.y - self.drag_start_world.y;
        let sb = self.drag_start_bounds;
        let min_h = self.effective_min_h();

        let (mut x, mut y, mut w, mut h) = (sb.x, sb.y, sb.width, sb.height);

        if let DragMode::Resize(dir) = self.drag_mode {
            match dir {
                ResizeDir::N  => { h = (sb.height + dy).max(min_h); }
                ResizeDir::S  => { y = sb.y + dy; h = (sb.height - dy).max(min_h); if h == min_h { y = sb.y + sb.height - min_h; } }
                ResizeDir::E  => { w = (sb.width  + dx).max(MIN_W); }
                ResizeDir::W  => { x = sb.x + dx; w = (sb.width  - dx).max(MIN_W); if w == MIN_W { x = sb.x + sb.width - MIN_W; } }
                ResizeDir::NE => { w = (sb.width  + dx).max(MIN_W); h = (sb.height + dy).max(min_h); }
                ResizeDir::NW => { x = sb.x + dx; w = (sb.width  - dx).max(MIN_W); if w == MIN_W { x = sb.x + sb.width - MIN_W; } h = (sb.height + dy).max(min_h); }
                ResizeDir::SE => { w = (sb.width  + dx).max(MIN_W); y = sb.y + dy; h = (sb.height - dy).max(min_h); if h == min_h { y = sb.y + sb.height - min_h; } }
                ResizeDir::SW => { x = sb.x + dx; w = (sb.width  - dx).max(MIN_W); if w == MIN_W { x = sb.x + sb.width - MIN_W; } y = sb.y + dy; h = (sb.height - dy).max(min_h); if h == min_h { y = sb.y + sb.height - min_h; } }
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
    /// External identity for z-order persistence, inspector lookup, etc.
    fn id(&self) -> Option<&str> { Some(&self.title) }

    fn is_visible(&self) -> bool {
        if let Some(ref cell) = self.visible_cell { cell.get() } else { self.visible }
    }

    /// A collapsed window paints only its title bar — nothing inside the
    /// content area is visible, so no child can legitimately request a
    /// repaint.  Closing (`is_visible` false) also short-circuits, matching
    /// the default trait impl.  Without these overrides a cursor blink or
    /// hover tween inside a collapsed/closed window would keep the host
    /// loop awake despite being invisible.
    fn needs_paint(&self) -> bool {
        if !self.is_visible() || self.collapsed { return false; }
        self.children().iter().any(|c| c.needs_paint())
    }

    fn next_paint_deadline(&self) -> Option<web_time::Instant> {
        if !self.is_visible() || self.collapsed { return None; }
        let mut best: Option<web_time::Instant> = None;
        for c in self.children() {
            if let Some(t) = c.next_paint_deadline() {
                best = Some(match best { Some(b) if b <= t => b, _ => t });
            }
        }
        best
    }

    fn bounds(&self) -> Rect { self.bounds }

    fn margin(&self)   -> Insets  { self.base.margin }
    fn h_anchor(&self) -> HAnchor { self.base.h_anchor }
    fn v_anchor(&self) -> VAnchor { self.base.v_anchor }
    fn min_size(&self) -> Size    { self.base.min_size }
    fn max_size(&self) -> Size    { self.base.max_size }

    /// Pop this window to the top of the parent `Stack` when the
    /// false→true visibility edge fires (see `layout`).
    fn take_raise_request(&mut self) -> bool {
        let pending = self.raise_request.get();
        self.raise_request.set(false);
        pending
    }

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
        // Rising-edge visibility detection → request parent raise.  The
        // sidebar toggles `visible_cell`; we observe the transition here
        // and set `raise_request`, which the parent `Stack` drains on its
        // next layout (one-frame delay, invisible to the user).
        let now_visible = self.is_visible();
        if now_visible && !self.last_visible.get() {
            self.raise_request.set(true);
            if let Some(cb) = self.on_raised.as_mut() { cb(&self.title); }
            // Un-maximize on reopen.  Clicking a sidebar checkbox is "open
            // this window for use" — the user expects the window to come
            // up at its normal size, not still stretched to fill the canvas
            // from the last session's maximise.  Restore `pre_maximize_bounds`
            // which `toggle_maximize` saved when the user maximised.
            if self.maximized {
                self.bounds    = self.pre_maximize_bounds;
                self.maximized = false;
            }
        }
        self.last_visible.set(now_visible);

        if !now_visible {
            return Size::new(self.bounds.width, self.bounds.height);
        }

        // Auto-size: measure the child's preferred size, then adopt it as the
        // new window size (pinning the top edge — Y-up → adjust `bounds.y` so
        // the title bar stays put when the height changes).  Skip while
        // collapsed: the user toggled a fixed TITLE_H height.
        //
        // We cap the measurement request by `child.max_size()` when finite
        // (otherwise by the canvas size): flex containers return their given
        // `available.width` rather than an intrinsic natural width, so without
        // a cap we'd produce an infinite/canvas-wide window.  Callers wanting
        // a content-fitted window set `with_max_size(Size::new(w, f64::MAX))`
        // on their root widget.
        if self.auto_size && !self.collapsed && !self.maximized {
            if let Some(child) = self.children.first_mut() {
                let max_sz = child.max_size();
                // `Size::MAX` uses `f64::MAX / 2.0` as its sentinel so
                // widgets can add-without-overflow (see `geometry.rs`).
                // That value is *technically* finite, so a plain
                // `.is_finite()` check wrongly treats it as a real cap
                // and cascades an ~`f64::MAX/2` width down to wrapped
                // Labels, whose bounds then blow up LCD-backbuffer
                // allocators to hundreds of GB.  Guard with a sane
                // threshold: anything ≥ `CAP_SENTINEL` means "no cap,
                // fall back to viewport-provided bounds".
                const CAP_SENTINEL: f64 = 1.0e18;
                // WIDTH is PINNED to the current bounds.width (seeded
                // by `with_bounds` and preserved across frames).
                // Why: wrapping Labels inside the content claim their
                // full available width — if we pass the viewport
                // width here, the window grows to the canvas on the
                // first frame and never shrinks back.  egui's
                // equivalent is `default_width`, which also pins.
                let cap_w  = self.bounds.width.max(MIN_W);
                let cap_h  = if max_sz.height.is_finite() && max_sz.height < CAP_SENTINEL {
                    max_sz.height
                } else {
                    available.height.max(MIN_H)
                };
                let pref   = child.layout(Size::new(cap_w, cap_h));
                // Auto-size follows content in BOTH directions — so
                // the window can also shrink back down when the
                // inner Resize (or any other sizing widget) narrows.
                // Lower bound: `MIN_W`.  Upper bound: the parent-
                // provided `available.width` (main_area / canvas).
                // Matches egui where auto_sized tracks content size
                // symmetrically.
                let new_w  = pref.width
                    .max(MIN_W)
                    .min(available.width.max(MIN_W));
                let new_h  = (pref.height + TITLE_H).min(cap_h + TITLE_H).max(MIN_H);
                let top    = self.bounds.y + self.bounds.height;
                self.bounds.width   = new_w;
                self.bounds.height  = new_h;
                self.bounds.y       = top - new_h;
                self.pre_collapse_h = new_h;
            }
        }

        // ── Tight-fit pre-pass ───────────────────────────────────
        //
        // When `with_tight_content_fit(true)` is set (and we're not
        // already in the auto_size block above, which handles both
        // axes), ask the content tree what minimum height it needs
        // at our current width and SNAP `bounds.height` to that.
        //
        // Uses `Widget::measure_min_height` rather than `layout` so
        // the result is independent of flex distribution — a
        // flex-fill widget like `TextArea` reports its true wrapped-
        // content height through `measure_min_height` even though
        // its `layout` returns the full slot.  This is what makes
        // egui's "no scroll, no clip, no whitespace" contract work
        // for windows whose content includes a flex-fill child.
        if self.tight_content_fit
            && !self.auto_size
            && !self.collapsed
            && !self.maximized
        {
            if let Some(child) = self.children.first() {
                let needed = child.measure_min_height(self.bounds.width);
                let new_h  = (needed + TITLE_H).max(MIN_H);
                let top    = self.bounds.y + self.bounds.height;
                self.bounds.height = new_h;
                self.bounds.y      = top - new_h;
                self.last_content_natural_h.set(needed);
            }
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

        // Cache the child's required height via `measure_min_height`
        // so `apply_resize` and the tight-fit floor see a current
        // value EVEN when the content's `layout` returns the slot
        // size (the flex-fill case).  `Widget::measure_min_height`
        // walks the content tree and returns the actual content
        // requirement at the supplied width.
        if (self.tight_content_fit || self.floor_content_height) && !self.collapsed {
            if let Some(child) = self.children.first() {
                self.last_content_natural_h.set(
                    child.measure_min_height(self.bounds.width)
                );
            }
        }

        // Position the title-bar strip at the top of the window and
        // give it a layout pass so the title label knows its size.
        let tb_y = self.bounds.height - TITLE_H;
        self.title_bar.set_bounds(Rect::new(0.0, tb_y, self.bounds.width, TITLE_H));
        self.title_bar.layout(Size::new(self.bounds.width, TITLE_H));

        // Record the canvas size — used by drag / resize / collapse clamp
        // paths that fire on USER ACTION.  We deliberately do NOT clamp
        // passively at layout time: platforms fire a Resized event with a
        // transient smaller size during fullscreen/maximize EXIT (Windows
        // notably), and if we clamped on shrink the auto-save would persist
        // those transient clamped bounds — the "all windows pushed down to
        // the same Y on next startup" bug.  Clamping only on user actions
        // (dragging a window, resize-handle, collapse toggle) keeps saved
        // state pinned to what the user actually chose.
        //
        // If a later OS shrink genuinely leaves a window's title bar out of
        // reach, the user can drag it back, use "Organize windows" to
        // retile, or a dedicated "reset positions" command.
        self.canvas_size = available;
        if let Some(ref cell) = self.position_cell {
            // When maximised, persist the UNDERLYING pre-maximise bounds,
            // not the stretched-to-canvas ones.  Maximise is an interaction
            // state, not a saved size: we want cold reloads to come up at
            // the user's last chosen "real" size, then let them re-maximise
            // if they want.  Matches native window-manager behaviour.
            let save_bounds = if self.maximized {
                self.pre_maximize_bounds
            } else {
                self.bounds
            };
            cell.set(save_bounds);
        }

        Size::new(self.bounds.width, self.bounds.height)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        if !self.is_visible() { return; }

        let v  = ctx.visuals();
        let w  = self.bounds.width;
        // bounds.height == TITLE_H when collapsed (adjusted on toggle).
        let h  = self.bounds.height;

        // Drop shadow — stacked rounded rects approximating a Gaussian blur.
        // Outer layers inflate outward and fade with a (1−t)² falloff; drawn
        // outside-in so the denser core overlays the softer halo.
        let base = v.window_shadow;
        for i in (0..SHADOW_STEPS).rev() {
            let t     = i as f64 / SHADOW_STEPS as f64;
            let infl  = t * SHADOW_BLUR;
            let falloff = (1.0 - t).powi(2) as f32;
            let alpha = base.a * falloff / SHADOW_STEPS as f32 * 6.0;
            ctx.set_fill_color(Color::rgba(base.r, base.g, base.b, alpha));
            ctx.begin_path();
            ctx.rounded_rect(
                SHADOW_DX - infl,
                -SHADOW_DY - infl,
                w + 2.0 * infl,
                h + 2.0 * infl,
                CORNER_R + infl,
            );
            ctx.fill();
        }

        // Window body.
        ctx.set_fill_color(v.window_fill);
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, w, h, CORNER_R);
        ctx.fill();

        // Sync the title-bar sub-widget's display state for this frame
        // and paint it.  Positioning was done in `layout`; we just need
        // to hand it the per-frame interaction snapshot and dispatch
        // through `paint_subtree` so the ancestor-chain stack gets the
        // WindowTitleBar entry (background_color = window_title_fill).
        {
            let mut st = self.title_state.borrow_mut();
            st.bar_color = if self.drag_mode == DragMode::Move {
                v.window_title_fill_drag
            } else {
                v.window_title_fill
            };
            st.title_color      = v.window_title_text;
            st.collapsed        = self.collapsed;
            st.maximized        = self.maximized;
            st.close_hovered    = self.close_hovered;
            st.maximize_hovered = self.maximize_hovered;
        }
        let tb_bounds = self.title_bar.bounds();
        ctx.save();
        ctx.translate(tb_bounds.x, tb_bounds.y);
        paint_subtree(&mut self.title_bar, ctx);
        ctx.restore();

        // Outer border — on top of the title bar so the rounded corners
        // cleanly frame both body and title region.
        ctx.set_fill_color(v.window_fill); // restore default fill — stroke follows
        ctx.set_stroke_color(v.window_stroke);
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, w, h, CORNER_R);
        ctx.stroke();

    }

    // paint_overlay: draws the resize handle dots + edge highlights on top of content.
    fn paint_overlay(&mut self, ctx: &mut dyn DrawCtx) {
        if !self.is_visible() || self.collapsed { return; }
        // Skip all resize-related chrome when the window can't be resized,
        // so an auto-sized or `.resizable(false)` window doesn't look
        // deceptively interactive.
        if !self.resizable || self.auto_size { return; }
        let v = ctx.visuals();
        let w = self.bounds.width;
        let h = self.bounds.height;

        // ── SE corner drag grip (3 diagonal lines, egui-style) ───────────────
        // Only shown when both axes are resizable; for uni-axis resizable
        // windows the SE grip would suggest a capability that isn't there.
        if self.resizable_h && self.resizable_v {
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
                let was_close = self.close_hovered;
                let was_max   = self.maximize_hovered;
                let was_dir   = self.hover_dir;
                self.close_hovered    = self.in_close_button(*pos);
                self.maximize_hovered = self.in_maximize_button(*pos);

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
                        crate::animation::request_tick();
                        return EventResult::Consumed;
                    }
                    DragMode::Resize(dir) => {
                        let world = Point::new(pos.x + self.bounds.x, pos.y + self.bounds.y);
                        self.apply_resize(world);
                        set_cursor_icon(resize_cursor(dir));
                        crate::animation::request_tick();
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
                if was_close != self.close_hovered
                    || was_max != self.maximize_hovered
                    || was_dir != self.hover_dir
                {
                    crate::animation::request_tick();
                }
                EventResult::Ignored
            }

            Event::MouseDown { button: MouseButton::Left, pos, .. } => {
                // Click-to-raise — any left click that reaches this Window
                // (hit-test routed it here in reverse paint order, so we
                // ARE the topmost widget under the cursor in the stack
                // sense) requests a raise.  Classic window-manager
                // behaviour: clicking anywhere on a window pops it to the
                // top of the z-order.  Consumed by `Stack::layout` on the
                // next frame via `take_raise_request`; one-frame visual
                // delay is invisible in practice.
                self.raise_request.set(true);
                // Z-order changes are visible; repaint.
                crate::animation::request_tick();
                if let Some(cb) = self.on_raised.as_mut() { cb(&self.title); }

                // Close button — highest priority.
                if self.in_close_button(*pos) {
                    self.visible = false;
                    if let Some(ref cell) = self.visible_cell { cell.set(false); }
                    if let Some(cb) = self.on_close.as_mut() { cb(); }
                    crate::animation::request_tick();
                    return EventResult::Consumed;
                }

                // Maximize / Restore button.
                if self.in_maximize_button(*pos) {
                    self.toggle_maximize();
                    crate::animation::request_tick();
                    return EventResult::Consumed;
                }

                // Collapse / expand chevron.
                if self.in_chevron_button(*pos) {
                    self.toggle_collapse();
                    // Null out the double-click timer so clicking the
                    // chevron then quickly clicking the bar doesn't
                    // trigger a maximize toggle.
                    self.last_title_click = None;
                    crate::animation::request_tick();
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

                // Title bar drag + double-click maximize.
                if self.in_title_bar(*pos) {
                    // Double-click detection.
                    let now = Instant::now();
                    let is_double = self.last_title_click
                        .map(|t| now.duration_since(t).as_millis() < DBL_CLICK_MS)
                        .unwrap_or(false);

                    if is_double {
                        // Windows convention: double-click title bar toggles
                        // maximize / restore.  Collapse/expand lives on the
                        // chevron button to the left.
                        self.toggle_maximize();
                        self.last_title_click = None;
                        crate::animation::request_tick();
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
                if was_dragging {
                    crate::animation::request_tick();
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }

            _ => EventResult::Ignored,
        }
    }
}
