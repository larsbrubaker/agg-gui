//! Read-only report of what a [`ScrollView`]'s bar on one axis is doing:
//! whether it is shown, where its track and thumb sit, and how much layout
//! room it takes from the viewport.
//!
//! agg-sharp's `ScrollableWidget` has its bars as child widgets, so hosts and
//! tests read `VerticalScrollBar.Visible`, `.Height` and
//! `ScrollArea.DeviceMargin` directly. agg-gui paints its bars, so this
//! report is the equivalent read (ported from the queries agg-sharp's
//! `ScrollableWidgetTests`, `ScrollableWidgetOptionsTests` and
//! `ScrollingAreaDeviceScaleTests` make). It is computed from the view's
//! current state, so a visibility change is reflected at once, with no layout.

use super::*;

/// One axis of a [`ScrollView`], as of its last layout and current settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollBarReport {
    /// The bar is drawn: the axis scrolls, its content overflows the
    /// viewport, and the visibility allows it. A
    /// [`ScrollBarKind::Floating`] bar under
    /// [`ScrollBarVisibility::VisibleWhenNeeded`] is shown only while hovered
    /// or dragged; the fade animation is not counted.
    pub shown: bool,
    /// The full track at the bar's full width, in the view's local (Y-up)
    /// space.
    pub track: Rect,
    /// The thumb inside [`Self::track`], when the content overflows.
    pub thumb: Option<Rect>,
    /// Layout room the bar takes from the viewport across its axis (the
    /// height a horizontal bar reserves at the bottom, the width a vertical
    /// one reserves at the right): non-zero only for a
    /// [`ScrollBarKind::Solid`] bar over overflowing content.
    pub reserved: f64,
    /// The content's extent along the axis.
    pub content: f64,
    /// The viewport's extent along the axis.
    pub viewport: f64,
}

impl ScrollView {
    /// The vertical bar's [`ScrollBarReport`].
    pub fn vertical_bar_report(&self) -> ScrollBarReport {
        let (_, vh) = self.viewport();
        let (reserve_x, _) = self.bar_reserve();
        self.bar_report(&self.v, vh, reserve_x, self.v_scrollbar_geometry())
    }

    /// The horizontal bar's [`ScrollBarReport`].
    pub fn horizontal_bar_report(&self) -> ScrollBarReport {
        let (vw, _) = self.viewport();
        let (_, reserve_y) = self.bar_reserve();
        self.bar_report(&self.h, vw, reserve_y, self.h_scrollbar_geometry())
    }

    fn bar_report(
        &self,
        axis: &ScrollbarAxis,
        viewport: f64,
        reserved: f64,
        geom: ScrollbarGeometry,
    ) -> ScrollBarReport {
        let w = self.style.bar_width;
        let len = geom.track_end - geom.track_start;
        let (track, thumb) = match geom.orientation {
            ScrollbarOrientation::Vertical => {
                let x = geom.cross_end - w;
                (
                    Rect::new(x, geom.track_start, w, len),
                    axis.thumb_metrics(viewport, self.style, geom)
                        .map(|(start, l)| Rect::new(x, start, w, l)),
                )
            }
            ScrollbarOrientation::Horizontal => (
                Rect::new(geom.track_start, geom.cross_end, len, w),
                axis.thumb_metrics(viewport, self.style, geom)
                    .map(|(start, l)| Rect::new(start, geom.cross_end, l, w)),
            ),
        };
        ScrollBarReport {
            shown: axis.enabled && axis.should_paint(viewport, self.style, self.bar_visibility),
            track,
            thumb,
            reserved,
            content: axis.content,
            viewport,
        }
    }
}
