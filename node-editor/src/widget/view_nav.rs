//! View navigation for [`NodeEditor`]: the interaction-mode switch and
//! the animated "fit every node in the view" operation.
//!
//! Both exist because hosts grow navigation chrome around the canvas —
//! a home button and a Select / Pan / Zoom mode group is the shape every
//! node editor in the wild ships (NodeDesigner, Blender, Unreal), and
//! none of it can live in the host: the pan / zoom transform is the
//! widget's private state, so only the widget can frame content or
//! redefine what a left-drag means.
//!
//! The animation is ticked from [`NodeEditor::layout`] rather than
//! driven by repeated host commands, for the same reason: `layout` is
//! the one call guaranteed to run on every frame a `request_draw`
//! schedules, and it already owns the pan / zoom fields the tween writes.
//!
//! Coordinates are agg-gui's **Y-up** canvas space (origin bottom-left,
//! `NodeLayoutInfo::top_left[1]` is the node's TOP edge, so a node's
//! bottom edge is `top_left[1] - size[1]`).

use web_time::Instant;

use super::{NodeEditor, ZOOM_MAX, ZOOM_MIN};

/// Blank space, in canvas units, left on each side of the framed content
/// by [`NodeEditor::fit_to_content`]. NodeDesigner's `fitToNodes` uses
/// 50 units per side, i.e. `+ 100` in the scale divisor.
pub const FIT_PADDING: f64 = 50.0;

/// Duration of the fit animation, in milliseconds.
pub const FIT_ANIM_MS: f64 = 500.0;

/// What a left-button drag on the canvas does.
///
/// Middle-drag always pans and the wheel always zooms, whatever the mode
/// — the mode only re-binds the left button, which is what makes it safe
/// to leave a host's mode group out of the picture entirely.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InteractionMode {
    /// Nodes are draggable, sockets connectable, properties editable —
    /// the editor's normal behaviour.
    #[default]
    Select,
    /// Left-drag pans the canvas. Node interaction is suppressed while
    /// this mode is active.
    Pan,
    /// Left-drag zooms about the press point. Node interaction is
    /// suppressed while this mode is active.
    Zoom,
}

impl InteractionMode {
    /// True for the modes that take the left button away from the nodes.
    pub fn suppresses_node_interaction(self) -> bool {
        !matches!(self, InteractionMode::Select)
    }
}

/// An in-flight pan/zoom tween. Both ends are absolute view states, so a
/// second `fit_to_content` mid-animation simply restarts from wherever
/// the view is at that instant.
#[derive(Clone, Debug)]
pub(super) struct ViewAnimation {
    started: Instant,
    duration_ms: f64,
    from_scale: f64,
    to_scale: f64,
    from_offset: [f64; 2],
    to_offset: [f64; 2],
}

/// NodeDesigner's easing for the fit animation: `easeInOutCubic`.
fn ease_in_out_cubic(t: f64) -> f64 {
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        let f = -2.0 * t + 2.0;
        1.0 - f * f * f / 2.0
    }
}

/// Axis-aligned bounds of every node in `layouts`, as
/// `(min_x, min_y, max_x, max_y)` in Y-up canvas space. `None` when there
/// are no nodes to frame.
///
/// Title bars are included because `NodeLayoutInfo::size` already spans
/// the whole node body, title strip included.
pub(super) fn content_bounds(
    layouts: &[crate::draw::NodeLayoutInfo],
) -> Option<(f64, f64, f64, f64)> {
    let mut it = layouts.iter();
    let first = it.next()?;
    let mut min_x = first.top_left[0];
    let mut max_x = first.top_left[0] + first.size[0];
    let mut max_y = first.top_left[1];
    let mut min_y = first.top_left[1] - first.size[1];
    for l in it {
        min_x = min_x.min(l.top_left[0]);
        max_x = max_x.max(l.top_left[0] + l.size[0]);
        max_y = max_y.max(l.top_left[1]);
        min_y = min_y.min(l.top_left[1] - l.size[1]);
    }
    Some((min_x, min_y, max_x, max_y))
}

/// The view (`scale`, `offset`) that frames `bounds` inside a
/// `viewport_w × viewport_h` pane with [`FIT_PADDING`] canvas units of
/// slack on every side.
///
/// Port of NodeDesigner's `graph-manager.js` `resetView`:
/// `scale = min(w / (bw + 100), h / (bh + 100))`, clamped to the
/// editor's zoom limits, with the offset centring the bounds. The Y term
/// is identical under our Y-up convention because `local = canvas *
/// scale + offset` on both axes — only the *meaning* of the bounds' y
/// components flips, and `content_bounds` already produced them Y-up.
pub(super) fn fit_view(
    bounds: (f64, f64, f64, f64),
    viewport_w: f64,
    viewport_h: f64,
) -> (f64, [f64; 2]) {
    let (min_x, min_y, max_x, max_y) = bounds;
    let bw = (max_x - min_x).max(0.0);
    let bh = (max_y - min_y).max(0.0);
    let pad = FIT_PADDING * 2.0;
    let scale = (viewport_w / (bw + pad))
        .min(viewport_h / (bh + pad))
        .clamp(ZOOM_MIN, ZOOM_MAX);
    let cx = (min_x + max_x) * 0.5;
    let cy = (min_y + max_y) * 0.5;
    let offset = [viewport_w * 0.5 - cx * scale, viewport_h * 0.5 - cy * scale];
    (scale, offset)
}

impl NodeEditor {
    /// The active left-drag binding.
    pub fn interaction_mode(&self) -> InteractionMode {
        self.mode
    }

    /// Re-bind the left button. Cancels any drag already in flight so a
    /// mode change mid-gesture can't leave a node following the pointer.
    ///
    /// Dropping the interaction state is exactly the "the drag ended"
    /// path in `on_mouse_up`, so it has to do the same cleanup: a node
    /// drag may have written snap guides into the framework's registry,
    /// and neither those nor an in-flight noodle are part of the cached
    /// backbuffer's fingerprint — without the invalidate the next paint
    /// blits an image still showing them.
    pub fn set_interaction_mode(&mut self, mode: InteractionMode) {
        if self.mode == mode {
            return;
        }
        self.mode = mode;
        if !matches!(self.interaction, super::CanvasState::Idle) {
            self.interaction = super::CanvasState::Idle;
            agg_gui::snap::clear_guides();
            self.backbuffer.invalidate();
        }
        agg_gui::animation::request_draw();
    }

    /// Abandon the fit tween where it stands.
    ///
    /// Called from every path where the *user* moves the view — a press
    /// that starts a pan or a zoom, the pan / zoom drag itself, the
    /// wheel. Without it the tween keeps writing the view from `layout()`
    /// underneath the drag and then snaps to its target when its 500 ms
    /// are up, which reads as the editor fighting the pointer.
    ///
    /// Returns `true` when an animation was actually running.
    pub fn cancel_view_animation(&mut self) -> bool {
        self.view_anim.take().is_some()
    }

    /// True while the fit animation is running — hosts paint their home
    /// button's "busy" state from this, and tests wait on it.
    pub fn is_view_animating(&self) -> bool {
        self.view_anim.is_some()
    }

    /// Adopt an exact view. Instant, clamped, and reported to the model's
    /// pan / zoom hooks so a host mirroring the view sees the new values.
    ///
    /// A non-finite component is **ignored outright** rather than
    /// clamped: `f64::clamp` passes `NaN` through unchanged, so a
    /// corrupt project file (or a host arithmetic slip) would otherwise
    /// poison the canvas transform permanently — every subsequent
    /// `local_to_canvas` returns `NaN`, nothing hit-tests, and no
    /// gesture can recover it. Returns `true` when the view was applied.
    pub fn set_view(&mut self, scale: f64, offset: [f64; 2]) -> bool {
        if !scale.is_finite() || !offset[0].is_finite() || !offset[1].is_finite() {
            return false;
        }
        self.view_anim = None;
        self.apply_view(scale.clamp(ZOOM_MIN, ZOOM_MAX), offset);
        true
    }

    /// Frame every node in the pane, animated over [`FIT_ANIM_MS`].
    ///
    /// Returns `false` (and does nothing) for an empty graph or a pane
    /// that has not been laid out yet — NodeDesigner's home button is
    /// likewise inert on an empty canvas.
    pub fn fit_to_content(&mut self) -> bool {
        let Some((scale, offset)) = self.target_fit_view() else {
            return false;
        };
        self.view_anim = Some(ViewAnimation {
            started: Instant::now(),
            duration_ms: FIT_ANIM_MS,
            from_scale: self.canvas_scale,
            to_scale: scale,
            from_offset: self.canvas_offset,
            to_offset: offset,
        });
        agg_gui::animation::request_draw();
        true
    }

    /// The view [`Self::fit_to_content`] is heading for, without starting
    /// the animation. Public so a host (and our own tests) can assert the
    /// framing maths without waiting out 500 ms.
    pub fn target_fit_view(&self) -> Option<(f64, [f64; 2])> {
        let w = self.bounds.width;
        let h = self.bounds.height;
        if w <= 0.0 || h <= 0.0 {
            return None;
        }
        let bounds = content_bounds(&self.snapshot_layouts())?;
        Some(fit_view(bounds, w, h))
    }

    /// Advance the fit animation by one frame. Called from `layout()`;
    /// returns `true` while the tween is still running so the caller can
    /// keep the frame loop awake.
    pub(super) fn tick_view_animation(&mut self) -> bool {
        let Some(anim) = self.view_anim.clone() else {
            return false;
        };
        let elapsed = anim.started.elapsed().as_secs_f64() * 1000.0;
        let raw = (elapsed / anim.duration_ms).clamp(0.0, 1.0);
        let t = ease_in_out_cubic(raw);
        let scale = anim.from_scale + (anim.to_scale - anim.from_scale) * t;
        let offset = [
            anim.from_offset[0] + (anim.to_offset[0] - anim.from_offset[0]) * t,
            anim.from_offset[1] + (anim.to_offset[1] - anim.from_offset[1]) * t,
        ];
        self.apply_view(scale, offset);
        if raw >= 1.0 {
            self.view_anim = None;
            false
        } else {
            agg_gui::animation::request_draw();
            true
        }
    }

    /// Write a view straight onto the canvas fields and fire both model
    /// hooks under one lock, so a host recomputing from the pair never
    /// sees a half-updated view (same contract as the wheel handler).
    fn apply_view(&mut self, scale: f64, offset: [f64; 2]) {
        self.canvas_scale = scale;
        self.canvas_offset = offset;
        {
            let mut model = self.model.lock().unwrap();
            model.on_canvas_pan_changed(offset);
            model.on_canvas_zoom_changed(scale);
        }
        self.backbuffer.invalidate();
        agg_gui::animation::request_draw();
    }
}
