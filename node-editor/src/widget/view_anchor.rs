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
//! A big graph fitted whole can shrink its cards' text past reading, so a
//! host can open a graph with [`NodeEditor::show_nodes_readable`] instead
//! ([`NodeEditor::request_readable_on_draw`] for the first layout): it fits
//! as the home button does while that zoom stays at or above the host's
//! minimum readable zoom, and otherwise opens at that minimum from the
//! graph's top-left, where reading starts. The host chooses the minimum,
//! because only it knows the fonts its cards draw.
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
    /// The minimum readable zoom of a pending centring that opens the
    /// graph readable ([`NodeEditor::request_readable_on_draw`]); `None`
    /// fits the whole graph.
    pub pending_min_scale: Option<f64>,
    /// The size of the last layout, to shift a centre-anchored view by
    /// half of a resize.
    pub last_size: Size,
}

/// NodeDesigner's `CenterNodesInView` margins, in device pixels (its
/// widgets measure in them): the graph keeps `MARGIN` in total across each
/// axis, plus `BOTTOM_PIXELS` above and below. Divided by the device scale
/// for the editor's logical units, so a 2x display fits as NodeDesigner does.
const MARGIN: f64 = 30.0;
const BOTTOM_PIXELS: f64 = 20.0;

/// What fitting the graph measured: its canvas bounds `(min_x, min_y,
/// max_x, max_y)`, the margins in logical units, and the zoom that fits it.
struct FitFrame {
    bounds: (f64, f64, f64, f64),
    margin: f64,
    bottom_pixels: f64,
    fit: f64,
}

/// NodeDesigner's `LayerScale` setter snaps a near-100 % zoom to 100 %.
fn snap_near_one(scale: f64) -> f64 {
    if scale > 0.95 && scale < 1.05 {
        1.0
    } else {
        scale
    }
}

/// The raw offset that centres the graph at `scale` in a `w` x `h` editor.
/// NodeDesigner: `UnscaledRenderOffset = -center + (0, 20) * scale`, then
/// `local = (canvas + U) * scale + size / 2`.
fn centred_offset(frame: &FitFrame, scale: f64, w: f64, h: f64) -> [f64; 2] {
    let (min_x, min_y, max_x, max_y) = frame.bounds;
    let u = [
        -(min_x + max_x) * 0.5,
        -(min_y + max_y) * 0.5 + frame.bottom_pixels * scale,
    ];
    [u[0] * scale + w * 0.5, u[1] * scale + h * 0.5]
}

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
        let Some(frame) = self.fit_frame() else {
            return false;
        };
        let scale = snap_near_one(frame.fit.min(1.0).clamp(ZOOM_MIN, ZOOM_MAX));
        self.apply_centred(&frame, scale);
        true
    }

    /// Show the graph so its cards can be read: fit it whole, as
    /// [`Self::center_nodes_in_view`] does, when that zoom is at least
    /// `min_scale`; otherwise zoom to `min_scale` (snapped to 100 % when
    /// within 5 % of it, as the fit is) and start at the graph's top-left.
    /// There the graph's left edge sits at the margin and, when the graph
    /// is taller than the view, its top at the top margin; an axis the
    /// graph fits along at that zoom is centred instead. Instant. Returns
    /// `false`, leaving the view alone, in the same cases as
    /// [`Self::center_nodes_in_view`].
    pub fn show_nodes_readable(&mut self, min_scale: f64) -> bool {
        let Some(frame) = self.fit_frame() else {
            return false;
        };
        let fitted = snap_near_one(frame.fit.min(1.0).clamp(ZOOM_MIN, ZOOM_MAX));
        // A NaN minimum fits as the home button does.
        if fitted >= min_scale || min_scale.is_nan() {
            self.apply_centred(&frame, fitted);
            return true;
        }
        let scale = snap_near_one(min_scale.clamp(ZOOM_MIN, ZOOM_MAX));
        let (w, h) = (self.bounds.width, self.bounds.height);
        let (min_x, min_y, max_x, max_y) = frame.bounds;
        let centred = centred_offset(&frame, scale, w, h);
        // The margins split evenly across each axis: half of `margin` on
        // either side, and `bottom_pixels` more above and below.
        let fits_x = (max_x - min_x) * scale <= w - frame.margin;
        let fits_y = (max_y - min_y) * scale <= h - frame.margin - frame.bottom_pixels * 2.0;
        let x = if fits_x {
            centred[0]
        } else {
            frame.margin * 0.5 - min_x * scale
        };
        let y = if fits_y {
            centred[1]
        } else {
            h - frame.margin * 0.5 - frame.bottom_pixels - max_y * scale
        };
        self.view_anim = None;
        self.apply_view(scale, [x, y]);
        true
    }

    /// The graph's bounds, the margins in logical units and the zoom that
    /// fits the graph in the editor, or `None` when nothing fits (an empty
    /// graph, or a panel squeezed below the margins).
    fn fit_frame(&self) -> Option<FitFrame> {
        let w = self.bounds.width;
        let h = self.bounds.height;
        let (min_x, min_y, max_x, max_y) = content_bounds(&self.snapshot_layouts())?;
        let device = agg_gui::device_scale();
        let (margin, bottom_pixels) = (MARGIN / device, BOTTOM_PIXELS / device);
        let by_height = (h - margin - bottom_pixels * 2.0) / (max_y - min_y);
        let by_width = (w - margin) / (max_x - min_x);
        // C#'s `Math.Min` keeps a NaN where `f64::min` would drop it.
        let fit = if by_height.is_nan() || by_width.is_nan() {
            f64::NAN
        } else {
            by_height.min(by_width)
        };
        if fit.is_nan() || fit <= 0.0 || fit.is_infinite() {
            return None;
        }
        Some(FitFrame {
            bounds: (min_x, min_y, max_x, max_y),
            margin,
            bottom_pixels,
            fit,
        })
    }

    /// Centre the graph at `scale`, NodeDesigner's way.
    fn apply_centred(&mut self, frame: &FitFrame, scale: f64) {
        let offset = centred_offset(frame, scale, self.bounds.width, self.bounds.height);
        self.view_anim = None;
        self.apply_view(scale, offset);
    }

    /// Centre the graph ([`Self::center_nodes_in_view`]) on the next layout
    /// that can — retried every layout while the panel is too short or the
    /// graph empty — unless the user moves the view first (a pan, a zoom or
    /// the wheel cancels it). NodeDesigner sets this when it shows a graph
    /// that has no saved view.
    pub fn request_center_on_draw(&mut self) {
        self.view.center_pending = true;
        self.view.pending_min_scale = None;
        agg_gui::animation::request_draw();
    }

    /// Show the graph readable ([`Self::show_nodes_readable`] with
    /// `min_scale`) on the next layout that can, retried and cancelled as
    /// [`Self::request_center_on_draw`] is. For a graph shown for the
    /// first time, whose whole might be too small to read.
    pub fn request_readable_on_draw(&mut self, min_scale: f64) {
        self.view.center_pending = true;
        self.view.pending_min_scale = Some(min_scale);
        agg_gui::animation::request_draw();
    }

    /// Drop a pending [`Self::request_center_on_draw`].
    pub fn cancel_center_on_draw(&mut self) {
        self.view.center_pending = false;
    }

    /// Whether a [`Self::request_center_on_draw`] or
    /// [`Self::request_readable_on_draw`] is still waiting.
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
        if !self.view.center_pending {
            return;
        }
        let done = match self.view.pending_min_scale {
            Some(min_scale) => self.show_nodes_readable(min_scale),
            None => self.center_nodes_in_view(),
        };
        if done {
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
