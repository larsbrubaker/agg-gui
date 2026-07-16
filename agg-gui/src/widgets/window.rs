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
//! # ⚠ Backbuffer caching gotcha — read before adding a custom Window
//!
//! `Window` retains its painted pixels in a GL FBO (or CPU bitmap) and only
//! re-rasterises on widget setter mutations (`Label::set_text`, hover changes,
//! etc.).  Custom paint code that reads from an `Rc<RefCell<…>>` model the
//! framework can't observe — telemetry graphs, sensor streams, simulation
//! views — will blit stale pixels forever unless you tell the window to
//! invalidate.  Two ways:
//!
//! - `.with_live_content(true)` — Window self-invalidates every frame
//!   (auto-skipped when collapsed or hidden).  Use for streaming data.
//! - [`Window::invalidate_backbuffer`] — manual flag from the data-arrival
//!   path.  Use when invalidation is sparse and you want frame-skip when
//!   nothing changed.
//!
//! See [`Window::new`] for the full discussion.
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

use crate::cursor::{set_cursor_icon, CursorIcon};
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult, Key, MouseButton};
use crate::geometry::{Point, Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::text::Font;
use crate::widget::{BackbufferKind, BackbufferSpec, BackbufferState, Widget};
use crate::widgets::window_title_bar::{TitleBarView, WindowTitleBar};

/// Round all four components of a Rect to the nearest integer so widgets
/// are always placed on exact pixel boundaries (crisp bitmap blits, no blur).
fn snap(r: Rect) -> Rect {
    Rect::new(r.x.round(), r.y.round(), r.width.round(), r.height.round())
}

const TITLE_H: f64 = 28.0;
const CORNER_R: f64 = 8.0;
const SHADOW_BLUR: f64 = 14.0;
const SHADOW_DX: f64 = 2.0;
const SHADOW_DY: f64 = 6.0;
const VISIBILITY_FADE_SECS: f64 = 0.18;
const CLOSE_R: f64 = 6.0;
const CLOSE_PAD: f64 = 10.0;
const MAX_PAD: f64 = CLOSE_PAD + CLOSE_R * 2.0 + 4.0; // 26 px
const RESIZE_EDGE: f64 = 6.0; // px from the edge that counts as a resize zone
const MIN_W: f64 = 120.0;
const MIN_H: f64 = 80.0;
const DBL_CLICK_MS: u128 = 500; // double-click detection window

/// Which edge(s) are being dragged during a resize operation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ResizeDir {
    N,
    NE,
    E,
    SE,
    S,
    SW,
    W,
    NW,
}

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
    /// Runtime-driven flag cells (egui `Window::resizable/collapsible`,
    /// plus our `auto_size` addition). Read each `layout()` so a demo's
    /// checkboxes can steer the real host window. `None` means the flag is
    /// fixed at its builder value.
    resizable_cell: Option<Rc<Cell<bool>>>,
    auto_size_cell: Option<Rc<Cell<bool>>>,
    collapsible_cell: Option<Rc<Cell<bool>>>,
    /// Live title text. Applied to the title-bar label only — the window's
    /// identity (`self.title`, used as the persistence / z-order key) is
    /// deliberately left untouched so retitling can't corrupt saved state.
    title_cell: Option<Rc<RefCell<String>>>,
    /// Last title string pushed into the title bar, so we only re-set (and
    /// invalidate the label's glyph cache) when the text actually changes.
    last_applied_title: RefCell<String>,

    /// Snapshot of `is_visible()` from the previous `layout()` call.  Used
    /// to detect the false→true transition (demo toggled on in the
    /// sidebar) so we can request the parent `Stack` raise us to the top.
    last_visible: Cell<bool>,
    /// `true` until the first `layout()` runs.  A window restored as
    /// already-visible (e.g. saved-state inspector open) misses the
    /// rising-edge fit-to-canvas pass, so without this one-shot trigger
    /// its persisted bounds can land outside a smaller live viewport
    /// (mobile portrait, resized window, etc.) and the user sees the
    /// sidebar toggle highlighted but no window.  Cleared after the
    /// first layout completes.
    needs_initial_fit: Cell<bool>,
    /// Set to `true` on a visibility rising edge; read + cleared by
    /// `take_raise_request` on the next parent-layout pass.
    raise_request: Cell<bool>,

    collapsed: bool,
    /// Whether the collapse affordance is offered. Driven at runtime by
    /// `collapsible_cell` when wired. Matches egui `Window::collapsible`.
    collapsible: bool,
    /// Height before collapsing, so we can restore it.
    pre_collapse_h: f64,

    drag_mode: DragMode,
    /// Cursor world position when drag started.
    drag_start_world: Point,
    /// Window bounds when drag started.
    drag_start_bounds: Rect,

    close_hovered: bool,
    on_close: Option<Box<dyn FnMut(CloseReason)>>,

    /// What an outside pointer press does to this (modal) window. `None` swallows
    /// it and stays open; `Close` dismisses via [`Window::close`] with
    /// [`CloseReason::ClickAway`]. Only consulted while `modal` is set.
    click_away: ClickAwayAction,

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

    /// When true, the window declares itself an app-modal layer while visible
    /// (see [`Widget::has_active_modal`]).  The `App` then routes *all* pointer
    /// and keyboard events into this window's subtree, so a floating dialog
    /// (e.g. the colour-wheel picker) swallows every click over its bounds
    /// instead of leaking them to widgets painted underneath.  Opt-in — plain
    /// windows leave this `false` and hit-test normally.
    modal: bool,

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
    /// Fine-grained axis control, used when `resizable` is `true`.
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

    /// When `true`, the window's backbuffer is invalidated on every
    /// frame the window is visible-and-expanded, forcing the content
    /// widget's `paint()` to run fresh.  See [`with_live_content`] and
    /// the constructor doc-comment for when to set this.
    live_content: bool,

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

    /// Identity for the snap-layout system.  Minted once at
    /// construction from a process-wide counter and never changes —
    /// `Snappable` uses it to skip self-matches in the snap engine's
    /// target list.
    snap_id: crate::snap::SnapId,
}

impl Window {
    /// Create a new window with the given title, font, and content widget.
    ///
    /// Default position: `(60, 60)` with `size = (360, 280)`. Call
    /// [`with_bounds`] to override.
    ///
    /// Windows keep a retained backbuffer. Live content must either call
    /// [`Window::invalidate_backbuffer`] when external data changes or use
    /// [`Window::with_live_content`] to force repaint while visible.
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
            resizable_cell: None,
            auto_size_cell: None,
            collapsible_cell: None,
            title_cell: None,
            last_applied_title: RefCell::new(title_str.clone()),
            // Seed `last_visible` to `true` (matches `visible` above) so a
            // window that's open on first frame doesn't spuriously request
            // a raise before the user has interacted with it.
            last_visible: Cell::new(true),
            needs_initial_fit: Cell::new(true),
            raise_request: Cell::new(false),
            collapsed: false,
            collapsible: true,
            pre_collapse_h: 280.0,
            drag_mode: DragMode::None,
            drag_start_world: Point::ORIGIN,
            drag_start_bounds: Rect::default(),
            close_hovered: false,
            on_close: None,
            click_away: ClickAwayAction::None,
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
            modal: false,
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
            live_content: false,
            snap_id: crate::snap::next_snap_id(),
        }
    }

    /// Returns the window title as it was passed to [`Window::new`].
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Force the window's retained backbuffer to re-rasterise on the next
    /// paint pass.  Use this when the content widget reads from a live
    /// data source (network feed, animation curve, simulation state)
    /// that the framework can't observe.  Otherwise the cached pixels
    /// blit unchanged and your live data never reaches the screen.
    ///
    /// Pair with [`Window::with_live_content`] for streaming data that
    /// changes every frame: that flag self-invalidates here automatically
    /// (and skips when collapsed/hidden).
    ///
    /// See [`Window::new`] for the full discussion of when this matters
    /// and the alternative ("compose live UI out of widgets that
    /// invalidate on data change") that avoids needing to call this at
    /// all.
    pub fn invalidate_backbuffer(&mut self) {
        self.backbuffer.invalidate();
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
        // A modal window is constrained to the whole app VIEWPORT rather than the
        // (often tiny) overlay slot it nests in: `clamp_modal_into_viewport`
        // pulls it fully on-screen during the clip-free global-overlay paint and
        // folds any correction back into `bounds`, so a drag can travel across
        // the entire app and the paint clamp keeps it visible. Applying the
        // slot-based clamp here would re-cage it to the parent slot — the exact
        // "locked to a dimension less than the app" bug we're fixing.
        if self.modal {
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

    /// Close the window: hide it, sync the optional `visible_cell`, and fire
    /// `on_close`.  Shared by the title-bar × button, the Escape key (modal
    /// windows), and any programmatic close so every route runs the same
    /// teardown — critically the `on_close` hook, which a modal dialog uses to
    /// unwind in-flight state (e.g. cancelling a live colour preview). Before
    /// this was factored out, only the × button ran it, so an Escape/close of
    /// the colour dialog left its preview session dangling.
    ///
    /// `reason` distinguishes the dismissal route (× button, Escape, click-away)
    /// so `on_close` can react differently — the colour dialog commits a live
    /// change on click-away but cancels it on Escape / ×.
    fn close(&mut self, reason: CloseReason) {
        self.visible = false;
        self.visibility_anim.set_target(0.0);
        if let Some(ref cell) = self.visible_cell {
            cell.set(false);
        }
        if let Some(cb) = self.on_close.as_mut() {
            cb(reason);
        }
        crate::animation::request_draw();
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

mod builder;
pub mod chrome;
mod close;
mod paint;
mod snap_glue;
mod widget_impl;

pub use close::{ClickAwayAction, CloseReason};

pub use chrome::{
    paint_chevron, paint_chrome_body, paint_chrome_border, paint_chrome_shadow,
    paint_chrome_title_bar, ChromeStyle,
};
