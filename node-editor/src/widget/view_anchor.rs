//! The view's anchor, coordinate mapping, drop feedback and the pending
//! "centre the graph" of a freshly shown graph, for [`NodeEditor`].
//!
//! NodeDesigner measures the pan from the widget's centre
//! (`TotalTransform = Translate(UnscaledRenderOffset) * Scale *
//! Translate(Width / 2, Height / 2)`), so a panel resize keeps the graph
//! where it was relative to the middle; [`ViewAnchor::Center`] gives a
//! host that convention. The editor itself keeps one raw transform,
//! `local = canvas * scale + offset`, whatever the anchor: the anchor
//! changes only what [`NodeEditor::pan`], [`NodeEditor::set_view`] and
//! `NodeGraphModel::on_canvas_pan_changed` mean, and shifts the raw
//! offset by half of a resize.
//!
//! [`NodeEditor::center_nodes_in_view`] ports NodeDesigner's
//! `CenterNodesInView` (the home button, and the first draw of a graph);
//! [`NodeEditor::request_center_on_draw`] is its `centerPending`.
//!
//! Split out of `mod.rs` for the 800-line guardrail; the fit tween lives
//! in `view_nav`.

use agg_gui::{DrawCtx, Point, Size};

use super::view_nav::content_bounds;
use super::{NodeEditor, ZOOM_MAX, ZOOM_MIN};

/// Where [`NodeEditor::pan`] is measured from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ViewAnchor {
    /// From the editor's bottom-left corner: `local = canvas * scale +
    /// pan`. A resize keeps the bottom-left fixed.
    #[default]
    Origin,
    /// From the editor's centre: `local = canvas * scale + pan + size / 2`,
    /// as NodeDesigner does. A resize keeps the centre fixed. NodeDesigner's
    /// `UnscaledRenderOffset` is `pan / scale`.
    Center,
}

/// View state on [`NodeEditor`] beyond the raw pan and zoom.
#[derive(Default)]
pub(crate) struct ViewState {
    pub anchor: ViewAnchor,
    /// Something dragged over the editor will drop: ring it.
    pub drop_feedback: bool,
    /// Centre the graph on the next layout that can (NodeDesigner's
    /// `centerPending`).
    pub center_pending: bool,
    /// The size of the last layout, to shift a centre-anchored view by
    /// half of a resize.
    pub last_size: Size,
}

/// NodeDesigner's `CenterNodesInView` margins, in logical pixels: the
/// graph keeps `MARGIN` in total across each axis, plus `BOTTOM_PIXELS`
/// above and below.
const MARGIN: f64 = 30.0;
const BOTTOM_PIXELS: f64 = 20.0;

impl NodeEditor {
    /// Measure the pan from `anchor` (see [`ViewAnchor`]). The view on
    /// screen does not move.
    pub fn with_view_anchor(mut self, anchor: ViewAnchor) -> Self {
        self.view.anchor = anchor;
        self
    }

    /// Where the pan is measured from.
    pub fn view_anchor(&self) -> ViewAnchor {
        self.view.anchor
    }

    /// Map an editor-local point (Y up, origin at the editor's bottom-left)
    /// to canvas space at the current pan and zoom.
    pub fn screen_to_canvas(&self, local: Point) -> [f64; 2] {
        self.local_to_canvas(local)
    }

    /// Map a canvas-space point to editor-local coordinates; the inverse
    /// of [`Self::screen_to_canvas`].
    pub fn canvas_to_screen(&self, canvas: [f64; 2]) -> Point {
        Point::new(
            canvas[0] * self.canvas_scale + self.canvas_offset[0],
            canvas[1] * self.canvas_scale + self.canvas_offset[1],
        )
    }

    /// The editor's bottom-left in app-absolute logical coordinates, as of
    /// its last paint (so at worst one frame stale).
    pub fn app_origin(&self) -> Point {
        let (x, y) = self.last_abs_origin.get();
        Point::new(x, y)
    }

    /// Map an app-absolute logical point (a drag from another panel) to
    /// canvas space, through [`Self::app_origin`].
    pub fn app_to_canvas(&self, app: Point) -> [f64; 2] {
        let o = self.app_origin();
        self.local_to_canvas(Point::new(app.x - o.x, app.y - o.y))
    }

    /// Ring the editor in the theme accent (2 px) while something dragged
    /// over it will drop there; `false` draws nothing (the drag's cursor
    /// says a refusal). NodeDesigner's `DropFeedback`.
    pub fn set_drop_feedback(&mut self, on: bool) {
        if self.view.drop_feedback != on {
            self.view.drop_feedback = on;
            self.backbuffer.invalidate();
            agg_gui::animation::request_draw();
        }
    }

    /// Whether the drop ring is showing.
    pub fn drop_feedback(&self) -> bool {
        self.view.drop_feedback
    }

    /// Fit the whole graph in the editor, at most at 100 %, as
    /// NodeDesigner's home button does. Instant. Returns `false`, leaving
    /// the view alone, when there is nothing to fit or no room to fit it in
    /// (an empty graph, or a panel squeezed below the margins).
    pub fn center_nodes_in_view(&mut self) -> bool {
        let w = self.bounds.width;
        let h = self.bounds.height;
        let Some((min_x, min_y, max_x, max_y)) = content_bounds(&self.snapshot_layouts()) else {
            return false;
        };
        let by_height = (h - MARGIN - BOTTOM_PIXELS * 2.0) / (max_y - min_y);
        let by_width = (w - MARGIN) / (max_x - min_x);
        // C#'s `Math.Min` keeps a NaN where `f64::min` would drop it.
        let fit = if by_height.is_nan() || by_width.is_nan() {
            f64::NAN
        } else {
            by_height.min(by_width)
        };
        if fit.is_nan() || fit <= 0.0 || fit.is_infinite() {
            return false;
        }
        let mut scale = fit.min(1.0).clamp(ZOOM_MIN, ZOOM_MAX);
        // NodeDesigner's `LayerScale` setter snaps a near-100 % zoom to 100 %.
        if scale > 0.95 && scale < 1.05 {
            scale = 1.0;
        }
        // NodeDesigner: `UnscaledRenderOffset = -center + (0, 20) * scale`,
        // then `local = (canvas + U) * scale + size / 2`.
        let u = [
            -(min_x + max_x) * 0.5,
            -(min_y + max_y) * 0.5 + BOTTOM_PIXELS * scale,
        ];
        self.view_anim = None;
        self.apply_view(scale, [u[0] * scale + w * 0.5, u[1] * scale + h * 0.5]);
        true
    }

    /// Centre the graph ([`Self::center_nodes_in_view`]) on the next layout
    /// that can — retried every layout while the panel is too short or the
    /// graph empty — unless the user moves the view first (a pan, a zoom or
    /// the wheel cancels it). NodeDesigner sets this when it shows a graph
    /// that has no saved view.
    pub fn request_center_on_draw(&mut self) {
        self.view.center_pending = true;
        agg_gui::animation::request_draw();
    }

    /// Drop a pending [`Self::request_center_on_draw`].
    pub fn cancel_center_on_draw(&mut self) {
        self.view.center_pending = false;
    }

    /// Whether a [`Self::request_center_on_draw`] is still waiting.
    pub fn is_center_pending(&self) -> bool {
        self.view.center_pending
    }

    /// The pan as the anchor measures it — what [`Self::pan`] returns and
    /// `on_canvas_pan_changed` reports.
    pub(super) fn reported_pan(&self) -> [f64; 2] {
        match self.view.anchor {
            ViewAnchor::Origin => self.canvas_offset,
            ViewAnchor::Center => [
                self.canvas_offset[0] - self.bounds.width * 0.5,
                self.canvas_offset[1] - self.bounds.height * 0.5,
            ],
        }
    }

    /// The raw offset of a pan measured from the anchor.
    pub(super) fn raw_offset(&self, pan: [f64; 2]) -> [f64; 2] {
        match self.view.anchor {
            ViewAnchor::Origin => pan,
            ViewAnchor::Center => [
                pan[0] + self.bounds.width * 0.5,
                pan[1] + self.bounds.height * 0.5,
            ],
        }
    }

    /// Called at the top of `layout()` with the new size: a centre-anchored
    /// view moves by half the resize so the centre stays put.
    pub(super) fn view_resized(&mut self, available: Size) {
        let old = self.view.last_size;
        self.view.last_size = available;
        if self.view.anchor == ViewAnchor::Center
            && (old.width != available.width || old.height != available.height)
        {
            self.canvas_offset[0] += (available.width - old.width) * 0.5;
            self.canvas_offset[1] += (available.height - old.height) * 0.5;
            self.backbuffer.invalidate();
        }
    }

    /// Called from `layout()` once the cards are measured: run a pending
    /// centring, cleared only once it worked.
    pub(super) fn run_pending_center(&mut self) {
        if self.view.center_pending && self.center_nodes_in_view() {
            self.view.center_pending = false;
        }
    }

    /// The drop ring, painted over everything in editor-local space.
    pub(super) fn paint_drop_feedback(&self, ctx: &mut dyn DrawCtx) {
        if !self.view.drop_feedback {
            return;
        }
        let accent = ctx.visuals().accent;
        ctx.set_stroke_color(accent);
        ctx.set_line_width(2.0);
        ctx.begin_path();
        ctx.rect(1.0, 1.0, self.bounds.width - 2.0, self.bounds.height - 2.0);
        ctx.stroke();
    }
}
