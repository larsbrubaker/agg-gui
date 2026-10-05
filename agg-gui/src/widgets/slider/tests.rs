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
    assert!(
        !s.format_value().contains("NaN"),
        "label {}",
        s.format_value()
    );

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
    assert!(
        low <= mid && mid <= high,
        "not monotonic: {low} {mid} {high}"
    );
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
    assert_eq!(
        s.value(),
        10.0,
        "NaN frame should be ignored, kept previous"
    );
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

// ── SliderStyle (per-instance track / thumb overrides) ─────────────────────

const STYLE_W: f64 = 200.0;

/// Paint a max-valued slider (thumb at the right end, no trailing fill so
/// the rail's left end is bare) and return the framebuffer plus thumb x.
fn render_styled(style: Option<SliderStyle>) -> (crate::Framebuffer, f64) {
    let mut s = Slider::new(1.0, 0.0, 1.0, test_font())
        .with_trailing_fill(false)
        .with_show_value(false);
    if let Some(style) = style {
        s = s.with_style(style);
    }
    let _ = s.layout(Size::new(STYLE_W, WIDGET_H));
    s.set_bounds(Rect::new(0.0, 0.0, STYLE_W, WIDGET_H));
    let mut fb = crate::Framebuffer::new(STYLE_W as u32, WIDGET_H as u32);
    {
        let mut ctx = crate::GfxCtx::new(&mut fb);
        ctx.clear(crate::Color::black());
        paint_subtree(&mut s, &mut ctx);
    }
    let tx = s.thumb_pos();
    (fb, tx)
}

fn px(fb: &crate::Framebuffer, x: u32, y: u32) -> [u8; 3] {
    let i = ((y * fb.width() + x) * 4) as usize;
    let p = &fb.pixels()[i..i + 4];
    [p[0], p[1], p[2]]
}

fn rgb8(c: crate::Color) -> [u8; 3] {
    [
        (c.r * 255.0).round() as u8,
        (c.g * 255.0).round() as u8,
        (c.b * 255.0).round() as u8,
    ]
}

fn near(a: [u8; 3], b: [u8; 3]) -> bool {
    a.iter().zip(b.iter()).all(|(x, y)| x.abs_diff(*y) <= 3)
}

/// Mid-rail pixel (far from the thumb) and a pixel on the thumb's ring.
fn rail_px(fb: &crate::Framebuffer) -> [u8; 3] {
    px(fb, 100, (WIDGET_H * 0.5) as u32)
}
fn ring_px(fb: &crate::Framebuffer, tx: f64) -> [u8; 3] {
    px(fb, tx as u32, (WIDGET_H * 0.5 + THUMB_R - 1.25) as u32)
}

#[test]
fn slider_default_style_matches_visuals() {
    let v = crate::theme::current_visuals();
    for style in [None, Some(SliderStyle::default())] {
        let (fb, tx) = render_styled(style);
        assert!(
            near(rail_px(&fb), rgb8(v.track_bg)),
            "rail {:?}",
            rail_px(&fb)
        );
        assert!(
            near(ring_px(&fb, tx), rgb8(v.accent)),
            "thumb {:?}",
            ring_px(&fb, tx)
        );
    }
}

#[test]
fn slider_style_track_and_thumb_colors_are_used() {
    let track = crate::Color::rgb(1.0, 0.0, 0.0);
    let thumb = crate::Color::rgb(0.0, 1.0, 0.0);
    let (fb, tx) = render_styled(Some(SliderStyle {
        track: Some(track),
        thumb: Some(thumb),
        ..Default::default()
    }));
    assert!(near(rail_px(&fb), rgb8(track)), "rail {:?}", rail_px(&fb));
    assert!(
        near(ring_px(&fb, tx), rgb8(thumb)),
        "thumb {:?}",
        ring_px(&fb, tx)
    );
}

#[test]
fn slider_style_track_radius_is_used() {
    let track = crate::Color::rgb(1.0, 0.0, 0.0);
    // Top-left corner pixel of the rail's bare left end.
    let corner = |radius: Option<f64>| {
        let (fb, _) = render_styled(Some(SliderStyle {
            track: Some(track),
            track_radius: radius,
            ..Default::default()
        }));
        px(&fb, THUMB_R as u32, (WIDGET_H * 0.5 - TRACK_H * 0.5) as u32)[0]
    };
    let square = corner(Some(0.0));
    let pill = corner(None);
    assert!(
        square > 240,
        "radius 0 corner should be fully covered: {square}"
    );
    assert!(
        pill + 40 < square,
        "default pill corner {pill} vs square {square}"
    );
}
