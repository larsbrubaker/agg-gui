//! `Scene` — a pan/zoom container that hosts one child widget subtree in an
//! infinite scrollable/zoomable canvas.
//!
//! # What it does
//!
//! `Scene` applies a translation + uniform-scale transform (a
//! [`SceneTransform`]) to its content's painting **and** its pointer input, so
//! the hosted widgets stay fully interactive while the user pans (drag on empty
//! background, or middle-drag) and zooms (mouse wheel, anchored on the cursor).
//! Double-clicking the empty background resets the view, matching egui's
//! `Scene` container.  The current visible region is exposed as a scene-space
//! rect via [`Scene::scene_rect`] / [`Scene::with_scene_rect_cell`].
//!
//! # The content is a first-class framework child
//!
//! The hosted subtree lives in [`children`](crate::widget::Widget::children)
//! like any other container's content, laid out in **scene space** (pinned at
//! the origin).  The pan/zoom is injected through the framework's
//! [`child_transform`](crate::widget::Widget::child_transform) hook: painting,
//! hit-testing, event dispatch, focus, and the inspector all map coordinates
//! through that transform when they descend into the Scene, so the content is
//! fully interactive *and* reachable by:
//!
//! * keyboard focus — Tab traversal and click-to-focus reach widgets inside the
//!   Scene, so text fields hosted here accept typing;
//! * the inspector — hosted widgets appear in the tree at their on-screen
//!   (transformed) bounds.
//!
//! `Scene`'s own [`on_event`](crate::widget::Widget::on_event) handles only the
//! background gestures (pan / zoom / double-click reset).  It receives a pointer
//! event exactly when no hosted child consumed it — the framework's leaf→root
//! bubble delivers the press to the Scene only after the content declined it —
//! so a press on empty canvas pans while a press on a hosted button clicks the
//! button.  While a pan is in progress the Scene claims the pointer
//! exclusively (see [`claims_pointer_exclusively`]) so the drag keeps landing on
//! the Scene regardless of what sits under the moving cursor.
//!
//! Relationship to other modules: mirrors the builder style of
//! [`crate::widgets::ScrollView`]; the transform math lives in the
//! [`transform`] submodule (unit-tested independently); the background-gesture
//! handling lives in the [`events`] submodule.
//!
//! [`claims_pointer_exclusively`]: crate::widget::Widget::claims_pointer_exclusively
//! [`on_event`]: crate::widget::Widget::on_event

use std::cell::Cell;
use std::rc::Rc;

use web_time::Instant;

use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Point, Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::widget::Widget;

mod events;
mod transform;

pub use transform::SceneTransform;

/// Default zoom range, matching egui's `Scene` (`0.1..=2.0`).
pub const DEFAULT_ZOOM_RANGE: (f64, f64) = (0.1, 2.0);

/// Double-click window (ms) for background-reset detection.  300-400 ms is
/// the conventional range; egui's default double-click delay is 300 ms.
const DBL_CLICK_MS: u128 = 400;

/// Maximum pointer travel (logical px) for a press-release pair to still
/// count as a *click* rather than a drag — matches egui's click tolerance.
const MAX_CLICK_DIST: f64 = 6.0;

/// Wheel-notch → zoom sensitivity. `zoom *= exp(delta_y * k)`, so positive
/// `delta_y` (wheel forward / scroll up) zooms in, symmetric on the way out.
const ZOOM_SENSITIVITY: f64 = 0.1;

/// A pan/zoom canvas hosting a single content widget subtree.
///
/// See the [module docs](self) for the coordinate model and the keyboard-focus
/// limitation.
pub struct Scene {
    bounds: Rect,
    base: WidgetBase,

    /// The hosted subtree as the Scene's single framework child, laid out in
    /// scene space and pinned at the scene-space origin `(0, 0)`.  The pan/zoom
    /// is injected via [`Widget::child_transform`], so the framework maps
    /// coordinates through it for paint, hit-test, dispatch, focus, and the
    /// inspector.
    children: Vec<Box<dyn Widget>>,

    transform: SceneTransform,
    zoom_range: (f64, f64),

    /// Explicit size handed to `content.layout`. When `None`, the content is
    /// laid out against the Scene's own available size.
    content_size: Option<Size>,
    /// Scene-space rect that "reset view" (and the initial fit) targets. When
    /// `None`, the content's laid-out bounds are used.
    default_scene_rect: Option<Rect>,

    /// While `false`, every layout re-fits the view to the content so the
    /// initial view is correct as the container settles to its final size.
    /// The first pan/zoom sets it `true`, freezing the view under user control.
    user_interacted: bool,

    /// Optional cell that receives the visible scene rect each layout.
    scene_rect_cell: Option<Rc<Cell<Rect>>>,
    /// Optional cell polled each layout; when `true`, resets the view and
    /// clears the flag. Lets an external "Reset view" button drive the Scene.
    reset_cell: Option<Rc<Cell<bool>>>,

    // ── interaction state ──
    panning: bool,
    pan_last: Point,
    /// Where the current pan gesture's button went down — used to decide on
    /// release whether the gesture was a *click* (no significant motion) or a
    /// drag, for double-click-reset detection.
    pan_press: Point,
    /// `true` once the current pan gesture moved beyond the click tolerance.
    pan_moved: bool,
    /// `true` when the current pan gesture was started with the left button
    /// (only left-button clicks participate in double-click reset).
    pan_is_left: bool,
    /// Pointer-press epoch (see [`crate::animation::pointer_press_epoch`])
    /// captured when the current background press began — compared against
    /// [`last_bg_click_epoch`](Self::last_bg_click_epoch) on release so a
    /// double-click that straddles a hosted-child click (which the Scene never
    /// sees, because the child consumes it) is not mistaken for two
    /// consecutive background clicks.
    pan_press_epoch: u64,
    /// Completion time of the last genuine background left-click
    /// (press + release without significant motion), for double-click reset.
    last_bg_click: Option<Instant>,
    /// Pointer-press epoch of that last background click.  A second click only
    /// completes a double-click when its press epoch is exactly one greater
    /// (i.e. no other press happened in between).
    last_bg_click_epoch: Option<u64>,
}

impl Scene {
    /// Create a Scene hosting `content`, with the default egui zoom range.
    pub fn new(content: Box<dyn Widget>) -> Self {
        Self {
            bounds: Rect::default(),
            base: WidgetBase::new(),
            children: vec![content],
            transform: SceneTransform::identity(),
            zoom_range: DEFAULT_ZOOM_RANGE,
            content_size: None,
            default_scene_rect: None,
            user_interacted: false,
            scene_rect_cell: None,
            reset_cell: None,
            panning: false,
            pan_last: Point::ORIGIN,
            pan_press: Point::ORIGIN,
            pan_moved: false,
            pan_is_left: false,
            pan_press_epoch: 0,
            last_bg_click: None,
            last_bg_click_epoch: None,
        }
    }

    // Invariant: `children` always holds exactly one element — the hosted
    // content — set at construction and never drained.  `children_mut()` is
    // public trait surface, so a caller could in principle empty it; the
    // library itself never does, and indexing `[0]` documents (and asserts)
    // that contract.

    /// The hosted content subtree (the Scene's single child).
    pub(super) fn content(&self) -> &dyn Widget {
        self.children[0].as_ref()
    }

    /// Mutable access to the hosted content subtree.
    pub(super) fn content_mut(&mut self) -> &mut dyn Widget {
        self.children[0].as_mut()
    }

    /// Set the allowed zoom range (min, max). Values are screen-px per scene
    /// unit. Passing them reversed is tolerated.
    pub fn with_zoom_range(mut self, min: f64, max: f64) -> Self {
        self.zoom_range = (min, max);
        self
    }

    /// Lay the content out against this fixed size instead of the Scene's
    /// available size. Use when the content should occupy a definite scene-space
    /// extent regardless of the container size.
    pub fn with_content_size(mut self, size: Size) -> Self {
        self.content_size = Some(size);
        self
    }

    /// Scene-space rect that the initial fit and "reset view" target. When
    /// unset, the content's laid-out bounds are fitted instead.
    pub fn with_default_scene_rect(mut self, rect: Rect) -> Self {
        self.default_scene_rect = Some(rect);
        self
    }

    /// Publish the visible scene rect into `cell` on every layout.
    pub fn with_scene_rect_cell(mut self, cell: Rc<Cell<Rect>>) -> Self {
        self.scene_rect_cell = Some(cell);
        self
    }

    /// Bind a cell that, when set to `true`, resets the view on the next layout.
    pub fn with_reset_cell(mut self, cell: Rc<Cell<bool>>) -> Self {
        self.reset_cell = Some(cell);
        self
    }

    // ── layout-property forwarding (mirrors other container widgets) ──

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

    /// The region of scene space currently visible in the Scene's bounds.
    pub fn scene_rect(&self) -> Rect {
        self.transform
            .visible_scene_rect(Size::new(self.bounds.width, self.bounds.height))
    }

    /// Current pan/zoom transform (scene→screen).
    pub fn transform(&self) -> SceneTransform {
        self.transform
    }

    /// Reset the view to fit the default scene rect (or the content bounds)
    /// centered in the container. Re-enables the auto-fit-until-interaction
    /// behaviour so the view stays fitted as the container settles.
    pub fn reset_view(&mut self) {
        self.user_interacted = false;
        self.fit_to_default();
        self.publish_scene_rect();
        crate::animation::request_draw();
    }

    /// Compute and apply a fit transform for the current bounds. No-op until
    /// the container and content both have positive size.
    fn fit_to_default(&mut self) {
        let container = Size::new(self.bounds.width, self.bounds.height);
        let cb = self.content().bounds();
        let target = self
            .default_scene_rect
            .unwrap_or_else(|| Rect::new(0.0, 0.0, cb.width, cb.height));
        if container.width > 0.0
            && container.height > 0.0
            && target.width > 0.0
            && target.height > 0.0
        {
            self.transform = SceneTransform::fit(target, container, self.zoom_range);
        }
    }

    fn publish_scene_rect(&self) {
        if let Some(cell) = &self.scene_rect_cell {
            cell.set(self.scene_rect());
        }
    }
}

impl Widget for Scene {
    fn type_name(&self) -> &'static str {
        "Scene"
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

    /// Inject the pan/zoom as the framework-level child transform.  Every
    /// coordinate-mapping traversal (paint, hit-test, dispatch, inspector)
    /// composes this when descending into the content, so hosted widgets are
    /// painted, clicked, focused, and inspected at the right place.
    fn child_transform(&self) -> Option<crate::TransAffine> {
        Some(self.transform.to_affine())
    }

    /// While a background pan is in progress the Scene owns the pointer: the
    /// drag must keep landing on the Scene even as the cursor sweeps over
    /// hosted widgets, so hit-testing stops here until the gesture ends.
    fn claims_pointer_exclusively(&self, _local_pos: Point) -> bool {
        self.panning
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
        self.base.min_size
    }
    fn max_size(&self) -> Size {
        self.base.max_size
    }

    /// Smooth pan/zoom needs sub-pixel positioning; opt out of the crisp-UI
    /// integer-snap default (see [`Widget::enforce_integer_bounds`]).
    fn enforce_integer_bounds(&self) -> bool {
        false
    }

    fn layout(&mut self, available: Size) -> Size {
        // Poll the external reset trigger.
        if let Some(cell) = &self.reset_cell {
            if cell.get() {
                cell.set(false);
                self.user_interacted = false;
            }
        }

        self.bounds = Rect::new(0.0, 0.0, available.width, available.height);

        // Lay out the content at its natural (or configured) size and pin it to
        // the scene-space origin.
        let content_avail = self.content_size.unwrap_or(available);
        let sz = self.content_mut().layout(content_avail);
        self.content_mut()
            .set_bounds(Rect::new(0.0, 0.0, sz.width, sz.height));

        // Keep the view fitted until the user takes control, so the initial
        // framing is right even as the container settles to its final size.
        if !self.user_interacted {
            self.fit_to_default();
        }

        self.publish_scene_rect();
        available
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let w = self.bounds.width;
        let h = self.bounds.height;

        // Canvas background only. The hosted content is a framework child,
        // painted after this returns: the framework clips it to
        // `clip_children_rect` (the Scene's bounds, in screen space) and then
        // applies the pan/zoom via `child_transform`, so it lands exactly where
        // pointer/focus/inspector expect it.
        ctx.set_fill_color(v.bg_color);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, w, h);
        ctx.fill();
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        self.handle_event(event)
    }

    fn properties(&self) -> Vec<(&'static str, String)> {
        let r = self.scene_rect();
        vec![
            ("zoom", format!("{:.3}", self.transform.zoom)),
            (
                "scene_rect",
                format!("[{:.1}, {:.1}, {:.1}, {:.1}]", r.x, r.y, r.width, r.height),
            ),
            (
                "zoom_range",
                format!("{:.2}..={:.2}", self.zoom_range.0, self.zoom_range.1),
            ),
        ]
    }
}
