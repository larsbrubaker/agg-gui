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
//! # Why the content is not a framework child
//!
//! The framework's automatic hit-test and event dispatch
//! ([`crate::widget::hit_test_subtree`], [`crate::widget::dispatch_event`])
//! only translate pointer coordinates by each child's `bounds().x/y` — there is
//! no hook to inject a *scale*.  A zoomed child would therefore receive
//! mis-scaled coordinates through the standard machinery.  So `Scene` keeps its
//! content in a private field (not in `children()`), paints it manually under
//! the transform via [`crate::widget::paint_subtree`], and routes pointer
//! events into it manually after mapping screen→scene coordinates.  This gives
//! full pointer interactivity (hover, click, drag) for the hosted widgets.
//!
//! ## Known limitation — keyboard focus
//!
//! Because the content lives outside the widget tree the `App` walks for focus,
//! widgets inside a `Scene` cannot receive keyboard focus (Tab traversal and
//! click-to-focus stop at the `Scene`).  Pointer interaction works fully; text
//! entry into a focused field *inside* a `Scene` does not.  Host buttons,
//! labels, sliders, and painted shapes — not text fields — inside a `Scene`.
//!
//! Relationship to other modules: mirrors the builder style of
//! [`crate::widgets::ScrollView`]; the transform math lives in the
//! [`transform`] submodule (unit-tested independently); the event routing lives
//! in the [`events`] submodule.

use std::cell::Cell;
use std::rc::Rc;

use web_time::Instant;

use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Point, Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::widget::{paint_subtree, Widget};

mod events;
mod transform;

pub use transform::SceneTransform;

/// Default zoom range, matching egui's `Scene` (`0.1..=2.0`).
pub const DEFAULT_ZOOM_RANGE: (f64, f64) = (0.1, 2.0);

/// Double-click window (ms) for background-reset detection.
const DBL_CLICK_MS: u128 = 500;

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

    /// The hosted subtree. **Not** exposed through `children()` — see module
    /// docs. Positioned at scene-space origin `(0, 0)` after layout.
    content: Box<dyn Widget>,
    /// Always empty; returned by `children()` so the framework does not
    /// auto-traverse the transformed content with un-scaled coordinates.
    empty: Vec<Box<dyn Widget>>,

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
    /// Path (within the content subtree) that captured the pointer on a
    /// child-consumed `MouseDown`; receives subsequent moves/up.
    inner_captured: Option<Vec<usize>>,
    /// Path last hovered within the content subtree (for hover clearing).
    inner_hovered: Option<Vec<usize>>,
    /// Timestamp of the last background left-press, for double-click reset.
    last_bg_click: Option<Instant>,
}

impl Scene {
    /// Create a Scene hosting `content`, with the default egui zoom range.
    pub fn new(content: Box<dyn Widget>) -> Self {
        Self {
            bounds: Rect::default(),
            base: WidgetBase::new(),
            content,
            empty: Vec::new(),
            transform: SceneTransform::identity(),
            zoom_range: DEFAULT_ZOOM_RANGE,
            content_size: None,
            default_scene_rect: None,
            user_interacted: false,
            scene_rect_cell: None,
            reset_cell: None,
            panning: false,
            pan_last: Point::ORIGIN,
            inner_captured: None,
            inner_hovered: None,
            last_bg_click: None,
        }
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
        let cb = self.content.bounds();
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
        &self.empty
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.empty
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

    /// The content isn't a framework child, so the default children-walk can't
    /// see its ongoing draw needs — forward them explicitly.
    fn needs_draw(&self) -> bool {
        self.is_visible() && self.content.needs_draw()
    }
    fn next_draw_deadline(&self) -> Option<web_time::Instant> {
        if !self.is_visible() {
            return None;
        }
        self.content.next_draw_deadline()
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
        let sz = self.content.layout(content_avail);
        self.content
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

        // Canvas background.
        ctx.set_fill_color(v.bg_color);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, w, h);
        ctx.fill();

        // Paint the content under the pan/zoom transform, clipped to the
        // Scene's own bounds. The clip is established in screen-local space
        // *before* the transform so it stays axis-aligned to the widget.
        ctx.save();
        ctx.clip_rect(0.0, 0.0, w, h);
        ctx.translate(self.transform.offset.x, self.transform.offset.y);
        ctx.scale(self.transform.zoom, self.transform.zoom);
        let cb = self.content.bounds();
        ctx.translate(cb.x, cb.y);
        paint_subtree(self.content.as_mut(), ctx);
        ctx.restore();
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
                format!(
                    "[{:.1}, {:.1}, {:.1}, {:.1}]",
                    r.x, r.y, r.width, r.height
                ),
            ),
            (
                "zoom_range",
                format!("{:.2}..={:.2}", self.zoom_range.0, self.zoom_range.1),
            ),
        ]
    }
}
