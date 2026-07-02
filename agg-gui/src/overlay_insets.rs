//! Frame-scoped "reserved screen edges" for overlay placement.
//!
//! Mobile-style layouts pin chrome to the viewport edges — a button rail
//! on the left, a control tray or the on-screen keyboard at the bottom.
//! Anything that floats content near an anchor (tap-info cards, tooltips,
//! popovers) must avoid those strips or it renders underneath them,
//! looking cut off. Historically each painter hand-rolled its own clamp
//! against its own widget bounds and knew nothing about sibling overlays
//! — the exact bug class this module removes.
//!
//! Life cycle per frame:
//!
//! 1. [`crate::App::layout`] calls [`begin_frame`], zeroing the insets,
//!    then auto-reserves the on-screen keyboard's strip when visible (the
//!    one library-owned edge obstruction).
//! 2. Edge-hugging overlay widgets reserve their strip during layout —
//!    wrap them in [`crate::widgets::ReserveInset`] instead of calling
//!    [`reserve`] by hand.
//! 3. Paint-time placement code reads [`current`] — directly or through
//!    [`crate::card::anchored_rect`] — and keeps floating content inside
//!    the remaining safe area.
//!
//! All values are **logical units**, matching layout coordinates.
//! `current()` is complete only after the whole tree has laid out, so
//! consume it at paint time (which is when anchored overlays are placed).

use std::cell::Cell;

use crate::draw_ctx::DrawCtx;
use crate::geometry::{Rect, Size};
use crate::layout_props::Insets;

thread_local! {
    static RESERVED: Cell<Insets> = const {
        Cell::new(Insets {
            top: 0.0,
            bottom: 0.0,
            left: 0.0,
            right: 0.0,
        })
    };
}

/// Zero the reserved insets for a new layout pass. Called by
/// [`crate::App::layout`]; hosts driving `Widget::layout` directly (tests)
/// call it themselves when they use reservations.
pub fn begin_frame() {
    RESERVED.with(|c| {
        c.set(Insets {
            top: 0.0,
            bottom: 0.0,
            left: 0.0,
            right: 0.0,
        })
    });
}

/// Reserve edge strips for this frame. Each edge keeps the **max** of all
/// reservations — two left-edge rails don't stack, the wider one wins.
pub fn reserve(insets: Insets) {
    RESERVED.with(|c| {
        let cur = c.get();
        c.set(Insets {
            top: cur.top.max(insets.top),
            bottom: cur.bottom.max(insets.bottom),
            left: cur.left.max(insets.left),
            right: cur.right.max(insets.right),
        });
    });
}

/// The edge strips reserved so far this frame (logical units, viewport
/// relative). Complete once layout has finished.
pub fn current() -> Insets {
    RESERVED.with(|c| c.get())
}

/// Convert viewport-space insets into the local space of `container`, a
/// rect given in absolute (viewport) logical coordinates. Each local
/// edge is however far the corresponding reserved strip intrudes into
/// the container — zero when the strip doesn't reach it. Pure; see
/// [`for_paint_ctx`] for the paint-time entry point.
pub fn clip_to(container: Rect, insets: Insets, viewport: Size) -> Insets {
    Insets {
        left: (insets.left - container.x).max(0.0),
        bottom: (insets.bottom - container.y).max(0.0),
        right: ((container.x + container.width) - (viewport.width - insets.right)).max(0.0),
        top: ((container.y + container.height) - (viewport.height - insets.top)).max(0.0),
    }
}

/// The frame's reserved insets expressed in the local coordinates of the
/// widget currently painting through `ctx` (whose local size is
/// `local_size`). Uses the context's accumulated transform, so it is
/// exact regardless of where the widget sits in the tree — this is what
/// paint-time overlay placement ([`crate::card`]) should feed into
/// `anchored_rect_with_insets` when the painting widget doesn't fill the
/// viewport.
pub fn for_paint_ctx(ctx: &dyn DrawCtx, local_size: Size) -> Insets {
    // root_transform maps local → root *physical* pixels (it includes the
    // App-level DPI/UX scale); divide back to logical viewport space.
    let t = ctx.root_transform();
    let (mut x0, mut y0) = (0.0, 0.0);
    let (mut x1, mut y1) = (local_size.width, local_size.height);
    t.transform(&mut x0, &mut y0);
    t.transform(&mut x1, &mut y1);
    let s = crate::ux_scale::effective_scale().max(1e-6);
    let abs = Rect::new(
        x0.min(x1) / s,
        y0.min(y1) / s,
        (x1 - x0).abs() / s,
        (y1 - y0).abs() / s,
    );
    clip_to(abs, current(), crate::widget::current_viewport())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_to_converts_viewport_insets_into_container_space() {
        let viewport = Size::new(400.0, 800.0);
        // Sky-style container: full width, top 600 of the viewport.
        let container = Rect::new(0.0, 200.0, 400.0, 600.0);
        let ins = Insets {
            left: 56.0,
            bottom: 300.0, // keyboard: intrudes 100 into the container
            right: 0.0,
            top: 0.0,
        };
        let local = clip_to(container, ins, viewport);
        assert_eq!(local.left, 56.0);
        assert_eq!(local.bottom, 100.0, "only the intruding part remains");
        assert_eq!(local.right, 0.0);
        assert_eq!(local.top, 0.0);

        // A strip that stops short of the container clips to zero.
        let shallow = Insets {
            bottom: 150.0,
            ..Insets::default()
        };
        assert_eq!(clip_to(container, shallow, viewport).bottom, 0.0);
    }

    #[test]
    fn reservations_max_merge_and_reset() {
        begin_frame();
        reserve(Insets {
            left: 56.0,
            bottom: 40.0,
            ..Insets::default()
        });
        reserve(Insets {
            left: 30.0,
            bottom: 120.0,
            ..Insets::default()
        });
        let r = current();
        assert_eq!(r.left, 56.0, "wider left rail wins");
        assert_eq!(r.bottom, 120.0, "taller bottom strip wins");
        assert_eq!(r.right, 0.0);

        begin_frame();
        assert_eq!(current(), Insets::default(), "new frame starts clean");
    }
}
