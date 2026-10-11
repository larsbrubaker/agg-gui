//! agg-sharp's `Tests/Agg.Tests/Agg.UI/ToolTipDeviceScaleTests.cs`: the gap
//! between the mouse and a tooltip's top edge is a design measurement (it
//! clears a cursor the OS draws at the display's scale), so in device pixels
//! it grows with the device scale.
//!
//! agg-gui places tips in logical units, where the clearance is always
//! [`CURSOR_CLEARANCE`](crate::widgets::tooltip::CURSOR_CLEARANCE); the device
//! gap is that times the effective scale, which is what C#'s
//! `CursorClearance * DeviceScale` measures. The hover and the window are
//! fed to the App in device pixels, as a shell does.

use crate::geometry::Point;
use crate::Rect;

use super::tool_tip_placement::show_tool_tip;

#[test]
fn the_gap_above_the_cursor_grows_with_device_scale() {
    assert!((gap_above_mouse(1.0) - 23.0).abs() <= 0.001);
    assert!(
        (gap_above_mouse(2.0) - 46.0).abs() <= 0.001,
        "the cursor the tooltip has to clear is twice as tall on a Retina display"
    );
}

/// How far, in device pixels, the top edge of a shown tooltip sits below the
/// mouse (agg-sharp's comment says "above": its `Top` is the edge facing
/// the cursor in its y-up window).
fn gap_above_mouse(device_scale: f64) -> f64 {
    // Roomy in every direction, so none of the screen edge nudges can fire and the only thing
    // setting the vertical position is the cursor clearance.
    let hovered = Rect::new(200.0, 350.0, 100.0, 100.0);
    let mouse_position = Point::new(250.0, 400.0);
    let tool_tip = show_tool_tip(hovered, mouse_position, 80.0, device_scale);
    (mouse_position.y - (tool_tip.y + tool_tip.height)) * device_scale
}
