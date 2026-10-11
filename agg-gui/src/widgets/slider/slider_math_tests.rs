//! agg-sharp's `SliderMathTests` (Agg.Tests/Agg.UI/SliderMathTests.cs), ported
//! 1:1 against [`super::Slider`] and [`crate::widgets::slider_math`].
//!
//! The C# class was itself ported from `slider_math.rs`'s own tests, so three of
//! its cases are duplicates and live there:
//! - `LinearMappingRoundTripsAndPinsOutsideTheRange` →
//!   `slider_math::tests::linear_mapping_roundtrips` + `linear_clamps_outside_range`
//! - `LogarithmicMapsGeometricallyIncludingZeroAndInfinity` →
//!   `slider_math::tests::logarithmic_midpoint_is_geometric_mean`,
//!   `logarithmic_reversed_range`, `logarithmic_spanning_zero_puts_zero_at_cutoff`,
//!   `logarithmic_spanning_infinity`, `infinite_range_initial_mapping_is_finite`
//! - `SmartAimPicksTheRoundestValue` → `slider_math::tests::smart_aim_round_numbers`,
//!   `smart_aim_integers`, `smart_aim_nan_and_infinity`
//!
//! Differences from agg-sharp's legacy `Slider`, ported as behaviour:
//! - agg-gui's slider always steps with its arrow keys (agg-sharp's
//!   `KeyboardStepping = false` legacy mode, leaving arrows to the app, does not
//!   exist), and a value set in code fires no `on_change` (agg-sharp's
//!   `ValueChanged` count); there is no `Position0To1` setter or runtime range
//!   setter, so `DefaultSliderIsUnchanged` and `RangeAndOptionChangesRecommitTheValue`
//!   keep the parts agg-gui has.
//! - a click anywhere on the track starts a drag (no separate thumb grab), so
//!   `MouseTrackClickAndDragSnapWithStepAndSmartAim` asserts the drag state.
//! - `KeyStepsAreAGrabAndARelease`: agg-gui has no grab callback; a key step is
//!   one `on_change` followed by one `on_release`.

use super::*;
use crate::geometry::Point;
use crate::widgets::slider_math::clamp_value_to_range;

const FONT_BYTES: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");

fn test_font() -> Arc<Font> {
    Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
}

/// A 200 px slider (C#'s `new Slider(Vector2.Zero, 200, min, max)`), laid out
/// without the value strip so the track spans the widget as in agg-sharp.
fn slider(min: f64, max: f64) -> Slider {
    let mut s = Slider::new(min, min, max, test_font())
        .with_step(0.0)
        .with_smart_aim(false)
        .with_show_value(false);
    let _ = s.layout(Size::new(200.0, WIDGET_H));
    s.set_bounds(Rect::new(0.0, 0.0, 200.0, WIDGET_H));
    s
}

fn key(k: Key) -> Event {
    Event::KeyDown {
        key: k,
        modifiers: Default::default(),
    }
}

fn mouse(kind: u8, x: f64) -> Event {
    let pos = Point::new(x, WIDGET_H * 0.5);
    match kind {
        0 => Event::MouseDown {
            pos,
            button: MouseButton::Left,
            modifiers: Default::default(),
        },
        1 => Event::MouseMove { pos },
        _ => Event::MouseUp {
            pos,
            button: MouseButton::Left,
            modifiers: Default::default(),
        },
    }
}

/// Pixel position (along the track) of `value`.
fn pixel_of(s: &Slider, value: f64) -> f64 {
    let (p0, p1) = s.position_range();
    p0 + normalized_from_value(value, s.props.min, s.props.max, &s.spec) * (p1 - p0)
}

#[test]
fn commit_clamps_steps_and_rounds() {
    assert_eq!(clamp_value_to_range(-1.0, 10.0, 0.0), 0.0);
    assert_eq!(clamp_value_to_range(11.0, 10.0, 0.0), 10.0);
    assert_eq!(slider(0.0, 100.0).commit(123.0), 100.0);
    assert_eq!(
        slider(0.0, 100.0)
            .with_clamping(SliderClamping::Never)
            .commit(123.0),
        123.0
    );

    // Steps count from the minimum; an infinite minimum has no multiples, so no snapping.
    assert_eq!(slider(2.0, 100.0).with_step(10.0).commit(17.0), 22.0);
    assert_eq!(
        slider(f64::NEG_INFINITY, f64::INFINITY)
            .with_step(10.0)
            .commit(17.0),
        17.0
    );
    assert_eq!(slider(0.0, 10.0).with_integer(true).commit(2.5), 3.0);
}

#[test]
fn auto_decimals_follow_step_then_magnitude() {
    let at = |v: f64, step: f64, integer: bool| {
        let mut s = slider(0.0, 100000.0).with_step(step).with_integer(integer);
        s.props.value = v;
        s.auto_decimals()
    };
    assert_eq!(at(3.14159, 0.0, true), 0);
    assert_eq!(at(3.14159, 0.1, false), 1);
    assert_eq!(at(10000.0, 0.0, false), 0);
    assert_eq!(at(10.0, 0.0, false), 2);
    assert_eq!(at(0.001, 0.0, false), 6);
}

#[test]
fn slider_value_options() {
    // Logarithmic: the value survives exactly and sits at its geometric position.
    let mut s = slider(1.0, 100.0).with_logarithmic(true);
    s.set_value(10.0);
    assert_eq!(s.value(), 10.0);
    assert!((s.normalized() - 0.5).abs() < 1e-9);

    // Clamping: Always clamps a value set in code, Never and Edits keep it (the thumb pins to the end).
    let mut s = slider(0.0, 10.0).with_clamping(SliderClamping::Never);
    s.set_value(20.0);
    assert_eq!(s.value(), 20.0);
    assert_eq!(s.normalized(), 1.0);
    s.clamping = SliderClamping::Always;
    s.set_value(20.0);
    assert_eq!(s.value(), 10.0);

    // Steps and the arrow keys: a step per press, clamped as an edit.
    let mut s = slider(0.0, 10.0)
        .with_step(2.0)
        .with_clamping(SliderClamping::Edits);
    s.set_value(3.1);
    assert_eq!(s.value(), 4.0);
    s.on_event(&key(Key::ArrowRight));
    assert_eq!(s.value(), 6.0);
    s.set_value(10.0);
    s.on_event(&key(Key::ArrowRight));
    assert_eq!(s.value(), 10.0);
    s.on_event(&key(Key::ArrowLeft));
    assert_eq!(s.value(), 8.0);
}

#[test]
fn default_slider_is_unchanged() {
    // No option on: the linear, clamped slider.
    let mut s = slider(0.0, 10.0);
    s.set_value(2.5);
    assert_eq!(s.normalized(), 0.25);
    assert_eq!(s.value(), 2.5);
    s.set_value(20.0);
    assert_eq!(s.value(), 10.0);
}

#[test]
fn clamp_to_range_treats_a_na_n_bound_as_open() {
    // As Rust's total_cmp orders a positive NaN: above everything, so the value passes that side.
    assert_eq!(clamp_value_to_range(5.0, 0.0, f64::NAN), 5.0);
    assert_eq!(clamp_value_to_range(-1.0, 0.0, f64::NAN), 0.0);
    assert_eq!(clamp_value_to_range(5.0, f64::NAN, 0.0), 5.0);
    assert_eq!(clamp_value_to_range(-1.0, f64::NAN, 0.0), 0.0);
}

#[test]
fn range_and_option_changes_recommit_the_value() {
    // Turning an option on keeps the value rather than reading the old linear position logarithmically.
    let mut s = slider(1.0, 100.0);
    s.set_value(10.0);
    let mut log = s.with_logarithmic(true);
    assert!((log.value() - 10.0).abs() < 1e-9);
    assert!((log.normalized() - 0.5).abs() < 1e-9);
    log.set_value(10.0);
    assert!((log.value() - 10.0).abs() < 1e-9);
}

#[test]
fn key_steps_are_a_grab_and_a_release() {
    // Apps that commit on release see each key step as a finished edit.
    let events = Rc::new(std::cell::RefCell::new(Vec::<&str>::new()));
    let (c, r) = (Rc::clone(&events), Rc::clone(&events));
    let mut s = slider(0.0, 10.0)
        .with_step(1.0)
        .on_change(move |_| c.borrow_mut().push("change"))
        .on_release(move |_| r.borrow_mut().push("release"));
    s.on_event(&key(Key::ArrowRight));
    assert_eq!(events.borrow().join(","), "change,release");
}

#[test]
fn vertical_slider_steps_with_up_and_down() {
    let mut s = slider(0.0, 10.0)
        .with_step(1.0)
        .with_orientation(SliderOrientation::Vertical);
    s.set_value(5.0);
    s.on_event(&key(Key::ArrowUp));
    assert_eq!(s.value(), 6.0);
    s.on_event(&key(Key::ArrowDown));
    s.on_event(&key(Key::ArrowDown));
    assert_eq!(s.value(), 4.0);

    // Left and right are not along a vertical slider's axis.
    s.on_event(&key(Key::ArrowRight));
    assert_eq!(s.value(), 4.0);
}

#[test]
fn mouse_track_click_and_drag_snap_with_step_and_smart_aim() {
    let mut s = slider(0.0, 100.0).with_step(10.0).with_smart_aim(true);

    // A click on the track at 47 snaps to the step.
    let at47 = pixel_of(&s, 47.0);
    s.on_event(&mouse(0, at47));
    s.on_event(&mouse(2, at47));
    assert_eq!(s.value(), 50.0);

    // Grabbing the thumb and dragging to 63 lands on 60.
    let thumb = s.thumb_pos();
    s.on_event(&mouse(0, thumb));
    assert!(s.dragging);
    let at63 = pixel_of(&s, 63.0);
    s.on_event(&mouse(1, at63));
    s.on_event(&mouse(2, at63));
    assert_eq!(s.value(), 60.0);
}

/// A value set in code under `Edits` clamping is not an edit, so it is kept
/// (the thumb pins to the end); before, `set_value` clamped it like a drag.
#[test]
fn rust_only_edits_clamping_keeps_a_value_set_in_code() {
    let mut s = slider(0.0, 10.0).with_clamping(SliderClamping::Edits);
    s.set_value(20.0);
    assert_eq!(s.value(), 20.0);
    assert_eq!(s.normalized(), 1.0);
    // An edit clamps.
    s.on_event(&key(Key::ArrowLeft));
    assert!(s.value() <= 10.0);
}
