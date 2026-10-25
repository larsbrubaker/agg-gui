//! The selection handles of [`super::TouchSelection`]: where each one sits,
//! the finger-sized area that grabs it, and how it is drawn.
//!
//! Two looks, chosen by the input profile ([`HandleStyle::current`]):
//!
//!   - **Teardrop** (Android, and every touch device that is not iOS): a disc
//!     hanging below the text line with one square corner pointing at the
//!     caret position. The start handle hangs to the left of its end of the
//!     selection, the end handle to the right, and the caret handle (Android's
//!     insertion handle) straight down, its point on the caret.
//!   - **Lollipop** (iOS): a bar the height of the text line at the selection
//!     edge with a round knob, above the line at the start and below it at the
//!     end. iOS has no caret handle.
//!
//! Geometry is in the host widget's local, Y-up coordinates. Each handle is
//! anchored at a caret line: its `x`, and the line's `bottom` and `top`.

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::geometry::{Point, Rect};
use crate::input_profile::{current_input_profile, InputProfile};

/// Radius (logical points) of a teardrop handle's disc: 22 points across,
/// Android's 22 dp text-select handle.
pub(crate) const TEARDROP_RADIUS: f64 = 11.0;

/// Radius of the lollipop's knob (logical points).
pub(crate) const KNOB_RADIUS: f64 = 5.0;

/// Width of the lollipop's bar (logical points).
const BAR_WIDTH: f64 = 2.0;

/// Which end a handle moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HandleKind {
    /// The start (lower byte offset) of a selection.
    Start,
    /// The end (higher byte offset) of a selection.
    End,
    /// The caret, when nothing is selected (Android only).
    Caret,
}

/// How handles look, after the platform being imitated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HandleStyle {
    Teardrop,
    Lollipop,
}

impl HandleStyle {
    /// The style of the current input profile: iOS draws lollipops, every
    /// other profile Android's teardrops.
    pub(crate) fn current() -> Self {
        if current_input_profile() == InputProfile::MobileIOS {
            HandleStyle::Lollipop
        } else {
            HandleStyle::Teardrop
        }
    }

    /// Whether a placed caret gets a draggable handle (Android does, iOS
    /// moves the caret with a loupe instead).
    pub(crate) fn has_caret_handle(self) -> bool {
        self == HandleStyle::Teardrop
    }
}

/// A caret line a handle hangs from (local, Y-up).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CaretLine {
    pub x: f64,
    pub bottom: f64,
    pub top: f64,
}

impl CaretLine {
    /// The middle of the line: where a handle drag aims the caret.
    pub(crate) fn middle(self) -> Point {
        Point::new(self.x, (self.bottom + self.top) * 0.5)
    }
}

/// Side of the square area that grabs a handle (logical units): the 44 CSS
/// px touch target whatever the UX zoom, larger than the drawn handle.
pub(crate) fn hit_size() -> f64 {
    crate::input_profile::TOUCH_TARGET_CSS_PX / crate::ux_scale::ux_scale().max(1e-6)
}

/// Centre of the drawn handle: the teardrop's disc or the lollipop's knob.
pub(crate) fn body_center(style: HandleStyle, kind: HandleKind, line: CaretLine) -> Point {
    let r = TEARDROP_RADIUS;
    match (style, kind) {
        (HandleStyle::Teardrop, HandleKind::Start) => Point::new(line.x - r, line.bottom - r),
        (HandleStyle::Teardrop, HandleKind::End) => Point::new(line.x + r, line.bottom - r),
        (HandleStyle::Teardrop, HandleKind::Caret) => {
            Point::new(line.x, line.bottom - r * std::f64::consts::SQRT_2)
        }
        (HandleStyle::Lollipop, HandleKind::Start) => Point::new(line.x, line.top + KNOB_RADIUS),
        (HandleStyle::Lollipop, _) => Point::new(line.x, line.bottom - KNOB_RADIUS),
    }
}

/// The finger-sized area that grabs the handle: a [`hit_size`] square on
/// the drawn body (on the bar's middle for a lollipop, whose knob alone is
/// too small to aim at).
pub(crate) fn hit_rect(style: HandleStyle, kind: HandleKind, line: CaretLine) -> Rect {
    let c = match style {
        HandleStyle::Teardrop => body_center(style, kind, line),
        HandleStyle::Lollipop => {
            let knob = body_center(style, kind, line);
            let mid = line.middle();
            Point::new(mid.x, (knob.y + mid.y) * 0.5)
        }
    };
    let s = hit_size();
    Rect::new(c.x - s * 0.5, c.y - s * 0.5, s, s)
}

/// How far below the text line the handle reaches (logical points), so a
/// toolbar placed under the selection clears it.
pub(crate) fn drop_below_line(style: HandleStyle) -> f64 {
    match style {
        HandleStyle::Teardrop => TEARDROP_RADIUS * (1.0 + std::f64::consts::SQRT_2),
        HandleStyle::Lollipop => KNOB_RADIUS * 2.0,
    }
}

/// Draw one handle in `color`.
pub(crate) fn paint(
    ctx: &mut dyn DrawCtx,
    style: HandleStyle,
    kind: HandleKind,
    line: CaretLine,
    color: Color,
) {
    ctx.set_fill_color(color);
    match style {
        HandleStyle::Teardrop => paint_teardrop(ctx, kind, line),
        HandleStyle::Lollipop => paint_lollipop(ctx, kind, line),
    }
}

/// A disc with one square corner on the caret point. Angles are Y-up and a
/// clockwise sweep runs towards decreasing angle.
fn paint_teardrop(ctx: &mut dyn DrawCtx, kind: HandleKind, line: CaretLine) {
    use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI, TAU};
    let r = TEARDROP_RADIUS;
    let c = body_center(HandleStyle::Teardrop, kind, line);
    let tip = (line.x, line.bottom);
    ctx.begin_path();
    match kind {
        HandleKind::Start => {
            // Corner at the disc's top right: top → corner → right, then
            // clockwise round the bottom and left back to the top.
            ctx.move_to(c.x, c.y + r);
            ctx.line_to(tip.0, tip.1);
            ctx.line_to(c.x + r, c.y);
            ctx.arc_to(c.x, c.y, r, 0.0, FRAC_PI_2 - TAU, false);
        }
        HandleKind::End => {
            // Corner at the disc's top left: left → corner → top, then
            // clockwise round the right and bottom back to the left.
            ctx.move_to(c.x - r, c.y);
            ctx.line_to(tip.0, tip.1);
            ctx.line_to(c.x, c.y + r);
            ctx.arc_to(c.x, c.y, r, FRAC_PI_2, PI - TAU, false);
        }
        HandleKind::Caret => {
            // The corner points straight up: the two tangents from the tip
            // touch the disc at 45 and 135 degrees.
            ctx.move_to(tip.0, tip.1);
            ctx.line_to(c.x + r * FRAC_PI_4.cos(), c.y + r * FRAC_PI_4.sin());
            ctx.arc_to(c.x, c.y, r, FRAC_PI_4, 3.0 * FRAC_PI_4 - TAU, false);
        }
    }
    ctx.close_path();
    ctx.fill();
}

/// A bar the height of the line with a knob above (start) or below (end).
fn paint_lollipop(ctx: &mut dyn DrawCtx, kind: HandleKind, line: CaretLine) {
    ctx.begin_path();
    ctx.rect(
        line.x - BAR_WIDTH * 0.5,
        line.bottom,
        BAR_WIDTH,
        (line.top - line.bottom).max(0.0),
    );
    ctx.fill();
    let knob = body_center(HandleStyle::Lollipop, kind, line);
    ctx.begin_path();
    ctx.circle(knob.x, knob.y, KNOB_RADIUS);
    ctx.fill();
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINE: CaretLine = CaretLine {
        x: 100.0,
        bottom: 50.0,
        top: 66.0,
    };

    #[test]
    fn teardrops_hang_below_the_line_beside_their_end_of_the_selection() {
        let s = body_center(HandleStyle::Teardrop, HandleKind::Start, LINE);
        let e = body_center(HandleStyle::Teardrop, HandleKind::End, LINE);
        let c = body_center(HandleStyle::Teardrop, HandleKind::Caret, LINE);
        assert!(s.x < LINE.x && e.x > LINE.x && c.x == LINE.x);
        for p in [s, e, c] {
            assert!(p.y < LINE.bottom, "below the text line: {p:?}");
        }
    }

    #[test]
    fn the_grab_area_is_finger_sized_and_larger_than_the_drawing() {
        let _profile = crate::input_profile::profile_test_lock();
        crate::ux_scale::set_ux_scale(1.0);
        for style in [HandleStyle::Teardrop, HandleStyle::Lollipop] {
            let r = hit_rect(style, HandleKind::End, LINE);
            assert_eq!((r.width, r.height), (44.0, 44.0));
            assert!(r.width > TEARDROP_RADIUS * 2.0);
            let body = body_center(style, HandleKind::End, LINE);
            assert!(r.x <= body.x && body.x <= r.x + r.width);
            assert!(r.y <= body.y && body.y <= r.y + r.height);
        }
    }

    #[test]
    fn lollipop_knobs_sit_above_the_start_and_below_the_end() {
        let s = body_center(HandleStyle::Lollipop, HandleKind::Start, LINE);
        let e = body_center(HandleStyle::Lollipop, HandleKind::End, LINE);
        assert!(s.y > LINE.top && e.y < LINE.bottom);
        assert!(!HandleStyle::Lollipop.has_caret_handle());
        assert!(HandleStyle::Teardrop.has_caret_handle());
    }
}
