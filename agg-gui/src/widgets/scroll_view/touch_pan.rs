//! Which finger drags a [`ScrollView`] pans.
//!
//! agg-gui's touch emulation (`crate::touch_emulation`) turns a finger drag
//! past the tap tolerance into a middle press at the touch-down point and
//! publishes the finger's travel while it is dispatched. A `ScrollView` pans
//! for it only when it has content hidden along the drag's main axis, as
//! agg-sharp's `ScrollableWidget.CanTouchPan` decides; otherwise it leaves the
//! press to its ancestors and, when none takes it, to the widget under the
//! finger as that widget's own left drag. A press on a bar the user can see
//! is the bar's drag, not a pan; a floating bar that has faded out is passed
//! over, since a finger cannot see it (agg-sharp
//! `TouchPressDeferral.PanTarget`).

use crate::geometry::Point;
use crate::widgets::scrollbar::ScrollbarAxis;

use super::{ScrollBarKind, ScrollBarStyle, ScrollBarVisibility, ScrollView};

impl ScrollView {
    /// Whether a finger drag pressed at `pos` (local) that has travelled
    /// `travel` (logical, Y-up) pans this view.
    pub(super) fn touch_can_pan(
        &self,
        pos: Point,
        travel: (f64, f64),
        v_scroll: bool,
        h_scroll: bool,
    ) -> bool {
        let (vw, vh) = self.viewport();
        let on_seen_bar = (v_scroll
            && self.pos_in_v_hover(pos)
            && bar_is_seen(&self.v, vh, self.style, self.bar_visibility))
            || (h_scroll
                && self.pos_in_h_hover(pos)
                && bar_is_seen(&self.h, vw, self.style, self.bar_visibility));
        if on_seen_bar {
            return false;
        }
        if travel.1.abs() >= travel.0.abs() {
            v_scroll
        } else {
            h_scroll
        }
    }
}

/// Whether a finger can see `axis`'s bar. A floating bar's showing depends on
/// hover, which the finger's own touch-down hover would fake, so it counts
/// only while it is actually painted (or dragged).
fn bar_is_seen(
    axis: &ScrollbarAxis,
    viewport: f64,
    style: ScrollBarStyle,
    visibility: ScrollBarVisibility,
) -> bool {
    if style.kind == ScrollBarKind::Floating && visibility == ScrollBarVisibility::VisibleWhenNeeded
    {
        axis.dragging || axis.visibility_anim.value() > 0.001
    } else {
        axis.should_paint(viewport, style, visibility)
    }
}
