//! Unit tests for [`super::Slider`].
//!
//! Split out of `slider/mod.rs` to keep that file under the project's 800-line
//! limit. As a child module these tests still reach the widget's private
//! internals (`normalized`, `thumb_pos`, `commit`, `props`, …), so they exercise
//! the real production code paths rather than copies.

use super::*;
use crate::geometry::Point;

const FONT_BYTES: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");

fn test_font() -> Arc<Font> {
    Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
}

fn mouse_down(x: f64, y: f64) -> Event {
    Event::MouseDown {
        pos: Point::new(x, y),
        button: MouseButton::Left,
        modifiers: Default::default(),
    }
}

fn mouse_move(x: f64, y: f64) -> Event {
    Event::MouseMove {
        pos: Point::new(x, y),
    }
}

/// The Sliders-demo range sliders span `-∞..=∞` (logarithmic). Constructing
/// one and dragging it must keep the value finite — historically the
/// implicit default step `(max - min) / 100` was `∞`, and step-snapping in
/// `commit` turned every interaction into `NaN`, which then poisoned the
/// shared value cell (and, through it, the demo slider's bounds).
#[test]
fn infinite_range_drag_stays_finite() {
    let cell = Rc::new(Cell::new(10000.0));
    let mut s = Slider::new(10000.0, -f64::INFINITY, f64::INFINITY, test_font())
        .with_logarithmic(true)
        .with_value_cell(Rc::clone(&cell));
    let _ = s.layout(Size::new(300.0, WIDGET_H));
    s.set_bounds(Rect::new(0.0, 0.0, 300.0, WIDGET_H));
    assert!(s.value().is_finite(), "initial value {}", s.value());
    assert!(!s.format_value().contains("NaN"), "label {}", s.format_value());

    // Click/drag around the middle of the track.
    s.on_event(&mouse_down(150.0, WIDGET_H * 0.5));
    assert!(
        s.value().is_finite(),
        "value after drag = {} (cell {})",
        s.value(),
        cell.get()
    );
    assert!(cell.get().is_finite(), "cell poisoned to {}", cell.get());
}

/// The demo slider's initial position/label must be finite for the demo's
/// default config (value 10 in 0..=10000, logarithmic).
#[test]
fn demo_slider_initial_is_finite() {
    let mut s = Slider::new(10.0, 0.0, 10000.0, test_font())
        .with_logarithmic(true)
        .with_step(0.0);
    let _ = s.layout(Size::new(300.0, WIDGET_H));
    assert!(s.normalized().is_finite(), "normalized {}", s.normalized());
    assert!(s.thumb_pos().is_finite(), "thumb {}", s.thumb_pos());
    assert!(!s.format_value().contains("NaN"));
}

/// A vertical slider must produce a finite thumb position and, when dragged
/// along its axis, move the value monotonically without ever going NaN.
#[test]
fn vertical_drag_is_monotonic_and_finite() {
    let mut s = Slider::new(10.0, 0.0, 10000.0, test_font())
        .with_logarithmic(true)
        .with_step(0.0)
        .with_orientation(SliderOrientation::Vertical);
    let _ = s.layout(Size::new(120.0, VERT_LEN));
    s.set_bounds(Rect::new(0.0, 0.0, 120.0, VERT_LEN));
    assert!(s.thumb_pos().is_finite(), "thumb {}", s.thumb_pos());

    // Press near the bottom (high y = low value), then drag toward the top.
    s.on_event(&mouse_down(THUMB_R, VERT_LEN - THUMB_R - 1.0));
    let low = s.value();
    assert!(low.is_finite(), "low {low}");
    s.on_event(&mouse_move(THUMB_R, VERT_LEN * 0.5));
    let mid = s.value();
    assert!(mid.is_finite(), "mid {mid}");
    s.on_event(&mouse_move(THUMB_R, THUMB_R + 1.0));
    let high = s.value();
    assert!(high.is_finite(), "high {high}");
    // Up = increase (Y-up mapping): dragging toward the top raises the value.
    assert!(low <= mid && mid <= high, "not monotonic: {low} {mid} {high}");
}

/// Defensive: a `NaN` written into the value cell (a single poisoned frame)
/// must not corrupt the slider — it keeps its previous finite value.
#[test]
fn nan_in_value_cell_is_ignored() {
    let cell = Rc::new(Cell::new(10.0));
    let mut s = Slider::new(10.0, 0.0, 10000.0, test_font())
        .with_logarithmic(true)
        .with_value_cell(Rc::clone(&cell));
    let _ = s.layout(Size::new(300.0, WIDGET_H));
    assert_eq!(s.value(), 10.0);
    cell.set(f64::NAN);
    let _ = s.layout(Size::new(300.0, WIDGET_H));
    assert_eq!(s.value(), 10.0, "NaN frame should be ignored, kept previous");
}

/// Defensive: `set_value(NaN)` is rejected and the previous value kept.
#[test]
fn set_value_rejects_nan() {
    let mut s = Slider::new(10.0, 0.0, 10000.0, test_font());
    s.set_value(f64::NAN);
    assert_eq!(s.value(), 10.0, "value {}", s.value());
}

/// `slider_math` deliberately supports reversed (high-to-low) ranges, so
/// constructing a slider with one must not panic — `f64::clamp` does when
/// min > max.
#[test]
fn new_with_reversed_range_does_not_panic() {
    let s = Slider::new(5.0, 10.0, 0.0, test_font());
    assert_eq!(s.value(), 5.0);
    // Out-of-range values clamp to the nearer end of the ordered range.
    let s = Slider::new(-1.0, 10.0, 0.0, test_font());
    assert_eq!(s.value(), 0.0);
}

/// Same for the external-cell binding, which clamps on construction and on
/// every layout read.
#[test]
fn value_cell_with_reversed_range_does_not_panic() {
    let cell = Rc::new(Cell::new(42.0));
    let mut s = Slider::new(5.0, 10.0, 0.0, test_font()).with_value_cell(Rc::clone(&cell));
    assert_eq!(s.value(), 10.0);
    cell.set(-3.0);
    let _ = s.layout(Size::new(200.0, 22.0));
    assert_eq!(s.value(), 0.0);
}
