//! agg-sharp's `Tests/Agg.Tests/Agg.UI/ScrollBarDeviceScaleTests.cs`, on
//! [`ScrollView`]: the bar's width follows a device-scale change made after
//! startup, and an application's own width is not scaled a second time.
//!
//! agg-sharp's `ScrollBar.ScrollBarWidth` and `GrowThumbBy` are device
//! pixels, defaulting to 15 and 3 times `DeviceScale`, and the bug was baking
//! that product at type load. agg-gui's [`ScrollBarStyle`] is in logical
//! units, read on every layout, and the device scale is applied when
//! painting, so each device-pixel expectation here is the logical value
//! times [`device_scale`]. `GrowThumbBy` is the inset of a resting thumb,
//! which agg-gui draws as a Floating bar's `floating_width`
//! (`bar_width - 2 * GrowThumbBy`, as mattercad-app sets it); the inset is
//! half of what the dormant thumb leaves of the strip. Both scale and style
//! are thread-local and restored by a guard.

use crate::device_scale::{device_scale, set_device_scale};
use crate::{
    ScrollBarKind, ScrollBarStyle, ScrollBarVisibility, ScrollView, Size, SizedBox, Widget,
};

struct ScaleGuard(f64);

impl Drop for ScaleGuard {
    fn drop(&mut self) {
        set_device_scale(self.0);
    }
}

/// A floating bar `bar_width` across with its thumb inset `grow` on each
/// side at rest, no margins.
fn style(bar_width: f64, grow: f64) -> ScrollBarStyle {
    ScrollBarStyle {
        bar_width,
        floating_width: bar_width - 2.0 * grow,
        outer_margin: 0.0,
        inner_margin: 0.0,
        kind: ScrollBarKind::Floating,
        ..ScrollBarStyle::default()
    }
}

/// The bar's width and resting thumb inset in device pixels, laid out now.
fn device_widths(style: ScrollBarStyle) -> (f64, f64) {
    let mut scroll = ScrollView::new(Box::new(SizedBox::new().with_height(500.0)))
        .with_style(style)
        .with_bar_visibility(ScrollBarVisibility::VisibleWhenNeeded);
    scroll.layout(Size::new(200.0, 100.0));
    let bar = scroll.scroll_bar_properties().vertical;
    let scale = device_scale();
    (
        bar.strip.width * scale,
        (bar.strip.width - bar.thumb.width) / 2.0 * scale,
    )
}

fn close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= 0.001,
        "expected {expected}, got {actual}"
    );
}

#[test]
fn default_widths_follow_a_later_device_scale_change() {
    let _guard = ScaleGuard(device_scale());
    // agg-sharp's defaults, in logical units: 15 across, inset 3
    let defaults = style(15.0, 3.0);
    set_device_scale(1.0);
    let (width, grow) = device_widths(defaults);
    close(width, 15.0);
    close(grow, 3.0);

    set_device_scale(2.0);

    // the default width has to be read at the scale in force now, not at startup
    let (width, grow) = device_widths(defaults);
    close(width, 30.0);
    close(grow, 6.0);
}

#[test]
fn an_explicit_width_wins_over_the_scaled_default() {
    let _guard = ScaleGuard(device_scale());
    set_device_scale(2.0);

    // An application that wants 22 device pixels at scale 2 sets 11 logical units, and nothing may
    // scale it a second time.
    let (width, grow) = device_widths(style(22.0 / 2.0, 4.0 / 2.0));
    close(width, 22.0);
    close(grow, 4.0);
}
