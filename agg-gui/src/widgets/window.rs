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
use crate::cursor::{set_cursor_icon, CursorIcon};
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult, MouseButton};
use crate::geometry::{Point, Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::text::Font;
use crate::widget::{paint_subtree, BackbufferKind, BackbufferSpec, BackbufferState, Widget};
use crate::widgets::window_title_bar::{TitleBarView, WindowTitleBar};

/// Round all four components of a Rect to the nearest integer so widgets
/// are always placed on exact pixel boundaries (crisp bitmap blits, no blur).
fn snap(r: Rect) -> Rect {
    Rect::new(r.x.round(), r.y.round(), r.width.round(), r.height.round())
}

const TITLE_H: f64 = 28.0;
const CORNER_R: f64 = 8.0;
/// Shadow blur radius in pixels (egui default Shadow::blur is ≈16; we use 14
/// for a slightly tighter falloff since windows live on a panel background).
const SHADOW_BLUR: f64 = 14.0;
/// Shadow offset from the window (Y-down visually → −y in Y-up space).
const SHADOW_DX: f64 = 2.0;
const SHADOW_DY: f64 = 6.0;
/// Number of stacked layers approximating a Gaussian blur falloff.
const SHADOW_STEPS: usize = 10;
const VISIBILITY_FADE_SECS: f64 = 0.18;
const CLOSE_R: f64 = 6.0;
const CLOSE_PAD: f64 = 10.0;
/// Horizontal distance from the right edge to the maximize button centre.
/// = CLOSE_PAD + CLOSE_R*2 + 4 px gap
const MAX_PAD: f64 = CLOSE_PAD + CLOSE_R * 2.0 + 4.0; // 26 px
const RESIZE_EDGE: f64 = 6.0; // px from the edge that counts as a resize zone
const MIN_W: f64 = 120.0;
const MIN_H: f64 = 80.0;
const DBL_CLICK_MS: u128 = 500; // double-click detection window

// ── Resize direction ───────────────────────────────────────────────────────────

/// Which edge(s) are being dragged during a resize operation.
#[derive(Clone, Copy, Debug, PartialEq)]
enum ResizeDir {
    N,
    NE,
    E,
    SE,
    S,
    SW,
    W,
    NW,
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
    visibility_anim: crate::animation::Tween,
    fade_out_active: Cell<bool>,
    backbuffer: BackbufferState,
    use_gl_backbuffer: bool,
    reset_to: Option<Rc<Cell<Option<Rect>>>>,
    position_cell: Option<Rc<Cell<Rect>>>,
    maximized_cell: Option<Rc<Cell<bool>>>,

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
    title_bar: WindowTitleBar,
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
    /// True between `paint()` and `finish_paint()` when GL compositing opened
    /// a foreground layer for body/title/children. The shadow stays outside.
    foreground_layer_active: Cell<bool>,

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
            visibility_anim: crate::animation::Tween::new(1.0, VISIBILITY_FADE_SECS),
            fade_out_active: Cell::new(false),
            backbuffer: BackbufferState::new(),
            use_gl_backbuffer: true,
            reset_to: None,
            position_cell: None,
            maximized_cell: None,
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
            foreground_layer_active: Cell::new(false),
            title: title_str,
            on_raised: None,
        }
    }

    /// Returns the window title as it was passed to [`Window::new`].
    pub fn title(&self) -> &str {
        &self.title
    }

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
        if self.maximized {
            self.pre_maximize_bounds = b;
        }
        self
    }
    pub fn with_font_size(mut self, size: f64) -> Self {
        self.font_size = size;
        self
    }

    pub fn with_visible_cell(mut self, cell: Rc<Cell<bool>>) -> Self {
        let visible = cell.get();
        self.last_visible.set(visible);
        self.fade_out_active.set(false);
        self.visibility_anim =
            crate::animation::Tween::new(if visible { 1.0 } else { 0.0 }, VISIBILITY_FADE_SECS);
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

    /// Wire the window's canvas-maximized state into external persistence.
    ///
    /// Call after [`with_bounds`] when restoring saved state so the current
    /// bounds become the pre-maximize bounds used by the first layout pass.
    pub fn with_maximized_cell(mut self, cell: Rc<Cell<bool>>) -> Self {
        self.maximized = cell.get();
        if self.maximized {
            self.pre_maximize_bounds = self.bounds;
        }
        self.maximized_cell = Some(cell);
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

    pub fn with_constrain(mut self, constrain: bool) -> Self {
        self.constrain = constrain;
        self
    }

    /// Opt this window in/out of the generic retained GL-FBO backbuffer.
    /// Disabling renders directly into the inherited parent target.
    pub fn with_gl_backbuffer(mut self, enabled: bool) -> Self {
        self.use_gl_backbuffer = enabled;
        self.backbuffer.invalidate();
        self
    }

    /// Make the window size itself to the content's preferred size every frame.
    /// Top-left pin: as content grows/shrinks, the title bar stays where it is.
    pub fn with_auto_size(mut self, auto: bool) -> Self {
        self.auto_size = auto;
        self
    }

    /// Toggle user-dragged resize.  `false` hides every edge/corner handle
    /// and disables resize hit-tests.  Default: `true`.  Matches egui's
    /// `Window::resizable(bool)`.
    pub fn with_resizable(mut self, on: bool) -> Self {
        self.resizable = on;
        self
    }

    /// Fine-grained axis-locking of the resize handles — pass `(true, false)`
    /// for a horizontally-only resizable window, etc.  Implies
    /// `with_resizable(true)`.  Matches egui's `Window::resizable([h, v])`.
    pub fn with_resizable_axes(mut self, h: bool, v: bool) -> Self {
        self.resizable = h || v;
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

    fn requested_visible(&self) -> bool {
        if let Some(ref cell) = self.visible_cell {
            cell.get()
        } else {
            self.visible
        }
    }

    fn layer_outsets() -> (f64, f64, f64, f64) {
        let left = (SHADOW_BLUR - SHADOW_DX).max(0.0).ceil();
        let bottom = (SHADOW_BLUR + SHADOW_DY).ceil();
        let right = (SHADOW_BLUR + SHADOW_DX).ceil();
        let top = (SHADOW_BLUR - SHADOW_DY).max(0.0).ceil();
        (left, bottom, right, top)
    }

    fn clamp_to_canvas(&mut self) {
        if !self.constrain {
            return;
        }
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

    fn fit_fully_to_canvas(&mut self, available: Size) {
        if !self.constrain || available.width <= 1.0 || available.height <= 1.0 {
            return;
        }
        let max_w = available.width.max(MIN_W);
        let max_h = available.height.max(TITLE_H);
        self.bounds.width = self.bounds.width.clamp(MIN_W.min(max_w), max_w).round();
        self.bounds.height = self.bounds.height.clamp(TITLE_H, max_h).round();
        self.bounds.x = self
            .bounds
            .x
            .clamp(0.0, (available.width - self.bounds.width).max(0.0))
            .round();
        self.bounds.y = self
            .bounds
            .y
            .clamp(0.0, (available.height - self.bounds.height).max(0.0))
            .round();
        self.pre_collapse_h = self.bounds.height;
        if self.maximized {
            self.pre_maximize_bounds = self.bounds;
        }
    }

    pub fn show(&mut self) {
        self.visible = true;
        self.fade_out_active.set(false);
        self.visibility_anim.set_target(1.0);
        crate::animation::request_draw();
    }
    pub fn hide(&mut self) {
        self.visible = false;
        self.visibility_anim.set_target(0.0);
        crate::animation::request_draw();
    }
    pub fn toggle(&mut self) {
        if self.visible {
            self.hide();
        } else {
            self.show();
        }
    }
    /// Current visibility — honours an optional shared `visible_cell` when
    /// wired (sidebar toggles, programmatic show/hide).  The inherent
    /// `self.visible` field is a fallback for windows that aren't wired to
    /// a cell.  Must match the Widget-trait impl below so rising-edge
    /// detection in `layout()` observes sidebar toggles.
    pub fn is_visible(&self) -> bool {
        self.requested_visible() || self.fade_out_active.get()
    }

    fn title_bar_bottom(&self) -> f64 {
        self.bounds.height - TITLE_H
    }

    fn in_title_bar(&self, local: Point) -> bool {
        local.y >= self.title_bar_bottom()
            && local.y <= self.bounds.height
            && local.x >= 0.0
            && local.x <= self.bounds.width
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
        local.x >= cx - half && local.x <= cx + half && local.y >= cy - half && local.y <= cy + half
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
                0.0,
                0.0,
                self.canvas_size.width,
                self.canvas_size.height,
            ));
            self.maximized = true;
        }
        if let Some(ref cell) = self.maximized_cell {
            cell.set(self.maximized);
        }
    }

    // ── Resize zone detection ──────────────────────────────────────────────────

    /// Return the resize direction for `local`, or `None` if the point is in
    /// the interior (or the window is collapsed).
    fn resize_dir(&self, local: Point) -> Option<ResizeDir> {
        if self.collapsed || self.auto_size {
            return None;
        }
        if !self.resizable {
            return None;
        }
        let w = self.bounds.width;
        let h = self.bounds.height;
        let x = local.x;
        let y = local.y;

        // Outside the window altogether.
        if x < 0.0 || x > w || y < 0.0 || y > h {
            return None;
        }

        // Mask each edge to the axes the window is allowed to resize on.
        let on_n = self.resizable_v && y > h - RESIZE_EDGE;
        let on_s = self.resizable_v && y < RESIZE_EDGE;
        let on_w = self.resizable_h && x < RESIZE_EDGE;
        let on_e = self.resizable_h && x > w - RESIZE_EDGE;

        match (on_n, on_e, on_s, on_w) {
            (true, true, _, _) => Some(ResizeDir::NE),
            (true, _, _, true) => Some(ResizeDir::NW),
            (_, _, true, true) => Some(ResizeDir::SW),
            (_, true, true, _) => Some(ResizeDir::SE),
            (true, _, _, _) => Some(ResizeDir::N),
            (_, true, _, _) => Some(ResizeDir::E),
            (_, _, true, _) => Some(ResizeDir::S),
            (_, _, _, true) => Some(ResizeDir::W),
            _ => None,
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
                ResizeDir::N => {
                    h = (sb.height + dy).max(min_h);
                }
                ResizeDir::S => {
                    y = sb.y + dy;
                    h = (sb.height - dy).max(min_h);
                    if h == min_h {
                        y = sb.y + sb.height - min_h;
                    }
                }
                ResizeDir::E => {
                    w = (sb.width + dx).max(MIN_W);
                }
                ResizeDir::W => {
                    x = sb.x + dx;
                    w = (sb.width - dx).max(MIN_W);
                    if w == MIN_W {
                        x = sb.x + sb.width - MIN_W;
                    }
                }
                ResizeDir::NE => {
                    w = (sb.width + dx).max(MIN_W);
                    h = (sb.height + dy).max(min_h);
                }
                ResizeDir::NW => {
                    x = sb.x + dx;
                    w = (sb.width - dx).max(MIN_W);
                    if w == MIN_W {
                        x = sb.x + sb.width - MIN_W;
                    }
                    h = (sb.height + dy).max(min_h);
                }
                ResizeDir::SE => {
                    w = (sb.width + dx).max(MIN_W);
                    y = sb.y + dy;
                    h = (sb.height - dy).max(min_h);
                    if h == min_h {
                        y = sb.y + sb.height - min_h;
                    }
                }
                ResizeDir::SW => {
                    x = sb.x + dx;
                    w = (sb.width - dx).max(MIN_W);
                    if w == MIN_W {
                        x = sb.x + sb.width - MIN_W;
                    }
                    y = sb.y + dy;
                    h = (sb.height - dy).max(min_h);
                    if h == min_h {
                        y = sb.y + sb.height - min_h;
                    }
                }
            }
        }

        self.bounds = snap(Rect::new(x, y, w, h));
        self.clamp_to_canvas();
    }
}

/// Map a resize direction to the appropriate OS cursor icon.
fn resize_cursor(dir: ResizeDir) -> CursorIcon {
    match dir {
        ResizeDir::N => CursorIcon::ResizeNorth,
        ResizeDir::S => CursorIcon::ResizeSouth,
        ResizeDir::E => CursorIcon::ResizeEast,
        ResizeDir::W => CursorIcon::ResizeWest,
        ResizeDir::NE => CursorIcon::ResizeNorthEast,
        ResizeDir::NW => CursorIcon::ResizeNorthWest,
        ResizeDir::SE => CursorIcon::ResizeSouthEast,
        ResizeDir::SW => CursorIcon::ResizeSouthWest,
    }
}

mod widget_impl;
