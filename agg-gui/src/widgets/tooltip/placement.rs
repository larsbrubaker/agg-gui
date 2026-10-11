//! Where the central tooltip controller puts a widget's tip — the
//! placement policy, and agg-sharp's `ToolTipManager.DoShowToolTip`
//! placement as a pure function.
//!
//! agg-gui's own placement ([`TooltipPlacement::Pointer`], the default, used
//! by AtomArtist and the demos) hangs the tip below and right of the pointer
//! ([`super::render::place_panel`]). agg-sharp's `ToolTipManager` instead
//! centres the tip on the widget it describes, because a tip anchored at the
//! cursor on a toolbar lands under the next icon over and reads as if it
//! belonged to that one ([`TooltipPlacement::Widget`]). A host that wants
//! agg-sharp's look (MatterCAD) opts in with [`set_tooltip_placement`].
//!
//! Distances here are logical units. agg-sharp's are design pixels scaled by
//! `GuiWidget.DeviceScale`, which is exactly what a logical unit is, so the
//! 23-unit cursor clearance is 46 device pixels on a 2x display. agg-sharp
//! rounds (and, for the mouse's Y, truncates) in device pixels; [`device_round`]
//! and [`device_trunc`] do the same at the effective scale so the tip lands
//! on the same device pixel.
//!
//! The controller ([`super::controller`]) calls [`place_on_widget`] when the
//! policy is `Widget` and it knows the hovered widget's bounds; tips offered
//! by surfaces that are not widgets (popup-menu rows) keep the pointer
//! placement.

use std::cell::Cell;

use crate::geometry::{Point, Rect, Size};

/// How the central tooltip controller places a widget's tip.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TooltipPlacement {
    /// agg-gui's placement: below and right of the pointer, flipped above
    /// when there is no room below.
    #[default]
    Pointer,
    /// agg-sharp `ToolTipManager`'s placement: centred on the hovered widget,
    /// sliding back to keep the cursor inside the tip on a wide widget,
    /// [`CURSOR_CLEARANCE`] below the pointer, [`EDGE_INSET`] from the window
    /// edges (the left edge winning for a tip wider than the window).
    Widget,
}

/// agg-sharp `ToolTipManager.CursorClearance`: the gap between the pointer
/// and the top of the tip, so the tip clears the cursor drawn below it.
pub const CURSOR_CLEARANCE: f64 = 23.0;

/// The gap agg-sharp's `ToolTipManager` keeps between a tip and the window
/// edges.
pub const EDGE_INSET: f64 = 3.0;

/// How far inside the tip's edge agg-sharp keeps the cursor when a wide
/// widget pulls the tip back under it.
const CURSOR_OVERLAP_MARGIN: f64 = 10.0;

thread_local! {
    static PLACEMENT: Cell<TooltipPlacement> = const { Cell::new(TooltipPlacement::Pointer) };
}

/// Choose how the central tooltip controller places widget tips (default
/// [`TooltipPlacement::Pointer`]). Takes effect on the next frame.
pub fn set_tooltip_placement(placement: TooltipPlacement) {
    PLACEMENT.with(|p| p.set(placement));
}

/// The current tooltip placement policy.
pub fn tooltip_placement() -> TooltipPlacement {
    PLACEMENT.with(|p| p.get())
}

/// Logical → device pixels for the rounding agg-sharp does in device pixels.
fn device_scale() -> f64 {
    crate::ux_scale::effective_scale().max(1e-6)
}

/// C# `Math.Round` (banker's rounding) of a logical value, in device pixels.
fn device_round(v: f64) -> f64 {
    let s = device_scale();
    round_half_even(v * s) / s
}

/// `f64::round_ties_even`, which the crate's MSRV (1.70) predates.
fn round_half_even(x: f64) -> f64 {
    if (x - x.trunc()).abs() == 0.5 {
        2.0 * (x / 2.0).round()
    } else {
        x.round()
    }
}

/// C# `(int)` truncation of a logical value, in device pixels.
fn device_trunc(v: f64) -> f64 {
    let s = device_scale();
    (v * s).trunc() / s
}

/// agg-sharp `ToolTipManager.DoShowToolTip`'s placement of a text tip of
/// `panel` size for the widget at `owner` with the pointer at `mouse`, inside
/// `visible` (all root logical coordinates, Y up).
pub(super) fn place_on_widget(mouse: Point, owner: Rect, panel: Size, visible: Rect) -> Rect {
    let (w, h) = (panel.width, panel.height);

    // Center on the widget, not on the mouse. A tooltip anchored at the cursor
    // sits beside the thing it describes - on a toolbar that lands it under the
    // next icon over and reads as if it belonged to that one. Vertically it
    // still hangs off the mouse, clear of the cursor.
    let mut left = device_round(owner.x + owner.width * 0.5 - w / 2.0);

    // Then keep the cursor inside the tooltip. For a widget narrower than its
    // tooltip (a toolbar icon) the mouse is already well inside the centered
    // tooltip and this does nothing, but for a wide widget (a full width row)
    // centering would fling the tooltip to the middle of the window, so it
    // slides back to the cursor. The margin keeps the cursor off the edge.
    let margin = CURSOR_OVERLAP_MARGIN.min(w / 2.0);
    left = left.min(device_round(mouse.x - margin));
    left = left.max(device_round(mouse.x - w + margin));

    let mut bottom = device_trunc(mouse.y) - h - CURSOR_CLEARANCE;

    let (win_left, win_bottom) = (visible.x, visible.y);
    let win_right = visible.x + visible.width;
    let mut offset_x = 0.0;
    if left + w > win_right - EDGE_INSET {
        offset_x = win_right - (left + w) - EDGE_INSET;
    }
    // Left after right, so a tooltip too wide for the window ends up flush
    // with the left edge rather than the right - text is read from the left.
    if left + offset_x < win_left + EDGE_INSET {
        offset_x = win_left + EDGE_INSET - left;
    }
    if bottom < win_bottom + EDGE_INSET {
        // No room below: sit just above the widget.
        bottom = owner.y + owner.height + EDGE_INSET;
    }
    left += offset_x;
    Rect::new(left, bottom, w, h)
}
