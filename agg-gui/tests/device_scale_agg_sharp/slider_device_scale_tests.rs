//! agg-sharp `SliderDeviceScaleTests`
//! (Tests/Agg.Tests/Agg.UI/SliderDeviceScaleTests.cs): the geometry a
//! `Slider` picks for itself — the thumb, its own height, the value readout's
//! place — grows with the device scale.
//!
//! agg-gui lays a slider out in logical units and the App paints it through
//! `ctx.scale(device_scale)`, so a logical size times the scale is the C#
//! device-pixel measurement, and the caller's length is logical at every
//! scale (C# callers pass `160 * DeviceScale`). Each test also measures the
//! painted pixels, within two pixels of antialiasing.
//!
//! `AVerticalSliderDropsItsReadoutByAScaledAmount`: agg-gui's readout sits
//! in a strip beside the track, centred on the slider, not under it; its
//! distance from the slider's top edge is what has to scale.

use agg_gui::{device_scale, Rect, Size, Slider, SliderOrientation, Widget};

use super::support::{at_device_scale, font, paint_widget_scaled, painted_extent, Region};

/// The slider sized as a caller does: a 160 long track, no readout.
fn new_slider() -> Slider {
    let mut slider = Slider::new(0.5, 0.0, 1.0, font()).with_show_value(false);
    let size = slider.layout(Size::new(160.0, 100.0));
    slider.set_bounds(Rect::new(0.0, 0.0, size.width, size.height));
    slider
}

/// The thumb's device-pixel size, laid out and painted.
fn thumb_size() -> ((f64, f64), (f64, f64)) {
    let mut slider = new_slider();
    let thumb = slider.thumb_rect();
    let s = device_scale();
    let laid_out = (thumb.width * s, thumb.height * s);
    let fb = paint_widget_scaled(&mut slider);
    // The track is thinner than the thumb, so the painted extent of the
    // thumb's own columns is the thumb.
    let region = Region::of_logical(
        Rect::new(thumb.x, 0.0, thumb.width, slider.bounds().height),
        &fb,
    );
    let painted = painted_extent(&fb, region, super::support::WHITE);
    (laid_out, painted)
}

#[test]
fn thumb_grows_with_device_scale() {
    let (at_one, painted_one) = at_device_scale(1.0, thumb_size);
    let (at_two, painted_two) = at_device_scale(2.0, thumb_size);
    assert!(
        (at_two.0 - at_one.0 * 2.0).abs() <= 0.001,
        "the thumb is the slider's own default size, so it has to double when everything else does"
    );
    assert!((at_two.1 - at_one.1 * 2.0).abs() <= 0.001);
    assert!(painted_one.1 > 0.0, "the thumb paints");
    assert!(
        (painted_two.1 - painted_one.1 * 2.0).abs() <= 2.0,
        "{painted_one:?} {painted_two:?}"
    );
}

#[test]
fn the_drawn_thumb_and_track_both_grow_with_device_scale() {
    // The laid-out height is the slider's own; its width the caller's.
    // Measuring the painted ink catches a thumb that scaled but a track that
    // did not, and vice versa.
    let bounds = || {
        let mut slider = new_slider();
        let b = slider.bounds();
        let s = device_scale();
        let fb = paint_widget_scaled(&mut slider);
        let painted = painted_extent(&fb, Region::whole(&fb), super::support::WHITE);
        ((b.width * s, b.height * s), painted)
    };
    let (at_one, painted_one) = at_device_scale(1.0, bounds);
    let (at_two, painted_two) = at_device_scale(2.0, bounds);
    assert!((at_two.1 - at_one.1 * 2.0).abs() <= 0.001);
    assert!((at_two.0 - at_one.0 * 2.0).abs() <= 0.001);
    assert!(
        (painted_two.0 - painted_one.0 * 2.0).abs() <= 2.0,
        "{painted_one:?} {painted_two:?}"
    );
    assert!(
        (painted_two.1 - painted_one.1 * 2.0).abs() <= 2.0,
        "{painted_one:?} {painted_two:?}"
    );
}

#[test]
fn a_vertical_slider_drops_its_readout_by_a_scaled_amount() {
    let one = at_device_scale(1.0, readout_drop_on_vertical_slider);
    let two = at_device_scale(2.0, readout_drop_on_vertical_slider);
    assert!(one > 0.0, "the readout paints");
    assert!(
        (two - one * 2.0).abs() <= 2.0,
        "the readout has to clear a label whose text has doubled: {one} {two}"
    );
}

/// Device pixels from the slider's top edge down to the top of the readout's
/// ink, in the readout strip right of the track.
fn readout_drop_on_vertical_slider() -> f64 {
    let mut slider = Slider::new(0.5, 0.0, 1.0, font())
        .with_orientation(SliderOrientation::Vertical)
        .with_decimals(2);
    let size = slider.layout(Size::new(100.0, 400.0));
    slider.set_bounds(Rect::new(0.0, 0.0, size.width, size.height));
    let fb = paint_widget_scaled(&mut slider);
    let strip = Region::of_logical(Rect::new(50.0, 0.0, 50.0, size.height), &fb);
    let top = (strip.y0..strip.y1).rev().find(|&y| {
        (strip.x0..strip.x1).any(|x| super::support::pixel(&fb, x, y) != super::support::WHITE)
    });
    match top {
        Some(y) => size.height * device_scale() - y as f64,
        None => 0.0,
    }
}
