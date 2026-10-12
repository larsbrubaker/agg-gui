//! Read-only query of a [`ScrollView`]'s scroll bars: where each bar's strip,
//! track and thumb lie, whether it is showing and how opaque, how much room
//! it takes from the content, which edges the fade covers, and the content
//! viewport they leave.
//!
//! agg-sharp's `ScrollableWidget` has its bars as child widgets (`ScrollBar`
//! with `Track` and `Thumb` children), so its tests read
//! `VerticalScrollBar.Visible`, the bars' bounds and
//! `ScrollArea.DeviceMargin` directly; agg-gui paints its bars, so the same
//! numbers come from [`ScrollView::scroll_bar_properties`]. The geometry is
//! exactly what `paint_overlay` draws (both go through
//! `ScrollbarAxis::bar_rects`), taken at the tweens' current values without
//! advancing them. Everything is computed from the view's current state, so
//! a visibility change is reflected at once, with no layout.

use super::*;

/// One scroll bar's geometry, in the view's local Y-up coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollBarAxisProperties {
    /// The axis scrolls, its content overflows and the visibility policy is
    /// not `AlwaysHidden` (agg-sharp's `ScrollBar.Visible`). A floating bar
    /// can be showing at opacity 0 while it waits for hover.
    pub showing: bool,
    /// The visibility tween's current value (agg-sharp's `ScrollBar.Opacity`).
    pub opacity: f64,
    /// Where the visibility is heading: 1 to paint, 0 to hide (agg-sharp's
    /// `ScrollBar.FadeTarget`).
    pub fade_target: f64,
    /// A thumb drag is in progress.
    pub dragging: bool,
    /// The room the bar owns, margins included (agg-sharp's `ScrollBar`
    /// bounds): `outer_margin + bar_width + inner_margin` across, and the
    /// track plus its inner margin at each end along. Its outer edge sits
    /// inside the parent window's resize-grip guard.
    pub strip: Rect,
    /// The painted track (agg-sharp's `ScrollBar.Track`); zero-sized when
    /// the content fits.
    pub track: Rect,
    /// The painted thumb (agg-sharp's `ScrollBar.Thumb`); zero-sized when
    /// the content fits.
    pub thumb: Rect,
    /// Layout room the bar takes from the content across its axis (the
    /// height a horizontal bar reserves at the bottom, the width a vertical
    /// one reserves at the right; agg-sharp's `ScrollArea.DeviceMargin`):
    /// from the view's edge to the strip's inner edge, so the strip plus the
    /// resize-grip guard outside it. Non-zero only for a
    /// [`ScrollBarKind::Solid`] bar over overflowing content that is not
    /// [`ScrollBarVisibility::AlwaysHidden`].
    pub reserved: f64,
    /// The content's extent along the axis.
    pub content: f64,
    /// The viewport's extent along the axis.
    pub viewport: f64,
}

/// Which edges of the viewport the scroll fade currently covers: those with
/// content clipped beyond them (agg-sharp's `ScrollEdgeFade.FadedEdges`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScrollFadedEdges {
    pub top: bool,
    pub bottom: bool,
    pub left: bool,
    pub right: bool,
}

/// Everything [`ScrollView::scroll_bar_properties`] reports.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollBarProperties {
    pub vertical: ScrollBarAxisProperties,
    pub horizontal: ScrollBarAxisProperties,
    /// The content viewport (the children's clip), local Y-up: the view
    /// less the room Solid bars reserve.
    pub viewport: Rect,
    /// The edges the fade paints over; none while `fade_strength` or
    /// `fade_size` is too small to paint.
    pub faded_edges: ScrollFadedEdges,
    /// The colour set by [`ScrollView::with_fade_color`]; `None` fades to
    /// `Visuals::window_fill`.
    pub fade_color: Option<Color>,
}

impl ScrollView {
    /// The scroll bars' current geometry and state, as of the last layout.
    pub fn scroll_bar_properties(&self) -> ScrollBarProperties {
        let (vw, vh) = self.viewport();
        let (reserve_x, reserve_y) = self.bar_reserve();
        let thickness = self.v_bar_thickness();
        let inner = self.style.inner_margin;

        let v_geom = self.v_scrollbar_geometry();
        let v_strip = Rect::new(
            v_geom.cross_end + self.style.outer_margin - thickness,
            v_geom.track_start - inner,
            thickness,
            v_geom.track_end - v_geom.track_start + 2.0 * inner,
        );
        let h_geom = self.h_scrollbar_geometry();
        let h_strip = Rect::new(
            h_geom.track_start - inner,
            h_geom.cross_end - self.style.outer_margin,
            h_geom.track_end - h_geom.track_start + 2.0 * inner,
            thickness,
        );

        ScrollBarProperties {
            vertical: self.axis_properties(&self.v, vh, v_geom, v_strip, reserve_x),
            horizontal: self.axis_properties(&self.h, vw, h_geom, h_strip, reserve_y),
            viewport: Rect::new(0.0, self.bounds.height - vh, vw, vh),
            faded_edges: if self.style.fade_strength > 0.001 && self.style.fade_size > 0.5 {
                self.faded_edges()
            } else {
                ScrollFadedEdges::default()
            },
            fade_color: self.fade_color,
        }
    }

    fn axis_properties(
        &self,
        axis: &ScrollbarAxis,
        viewport: f64,
        geom: ScrollbarGeometry,
        strip: Rect,
        reserved: f64,
    ) -> ScrollBarAxisProperties {
        let (track, thumb) = axis
            .bar_rects(viewport, self.style, geom, axis.hover_anim.value())
            .unwrap_or_default();
        ScrollBarAxisProperties {
            showing: axis.can_scroll(viewport)
                && self.bar_visibility != ScrollBarVisibility::AlwaysHidden,
            opacity: axis.visibility_anim.value(),
            fade_target: axis.fade_target(viewport, self.style, self.bar_visibility),
            dragging: axis.dragging,
            strip,
            track,
            thumb,
            reserved,
            content: axis.content,
            viewport,
        }
    }

    /// The viewport edges with content clipped beyond them, on the axes that
    /// scroll; `paint_fade` paints a gradient over each.
    pub(super) fn faded_edges(&self) -> ScrollFadedEdges {
        let (vw, vh) = self.viewport();
        ScrollFadedEdges {
            top: self.v.enabled && self.v.offset > 0.5,
            bottom: self.v.enabled && (self.v.max_scroll(vh) - self.v.offset) > 0.5,
            left: self.h.enabled && self.h.offset > 0.5,
            right: self.h.enabled && (self.h.max_scroll(vw) - self.h.offset) > 0.5,
        }
    }
}
