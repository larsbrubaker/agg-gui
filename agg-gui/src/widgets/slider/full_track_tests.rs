//! Tests for [`super::SliderStyle::full_length_track`] (agg-sharp
//! `SlideView`'s rail, drawn across the whole length the thumb occupies)
//! and for the in-place restyling setters [`super::Slider::set_style`] and
//! [`super::Slider::set_trailing_fill`].  Siblings: `style_tests.rs` covers
//! the other style options, `tests.rs` the mapping and events.  These paint
//! through the production `paint_subtree` path and read pixels back.

use super::*;
use crate::geometry::Point;
use crate::widget::paint_subtree;

const W: f64 = 200.0;

fn test_font() -> Arc<Font> {
    const FONT_BYTES: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");
    Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
}

const RED: crate::Color = crate::Color {
    r: 1.0,
    g: 0.0,
    b: 0.0,
    a: 1.0,
};
const GREEN: crate::Color = crate::Color {
    r: 0.0,
    g: 1.0,
    b: 0.0,
    a: 1.0,
};
const BLUE: crate::Color = crate::Color {
    r: 0.0,
    g: 0.0,
    b: 1.0,
    a: 1.0,
};
const BLACK: [u8; 3] = [0, 0, 0];

/// A square-ended red rail and a green thumb, so the rail's ends read
/// back as whole red pixels.
fn marker_style(full: bool) -> SliderStyle {
    SliderStyle {
        track: Some(RED),
        track_radius: Some(0.0),
        thumb: Some(GREEN),
        fill: Some(BLUE),
        full_length_track: full,
        ..Default::default()
    }
}

/// A laid-out horizontal slider (range 0..1, no value label, no step or
/// smart aim, so pixels map to values exactly).
fn slider(value: f64, style: SliderStyle) -> Slider {
    let mut s = Slider::new(value, 0.0, 1.0, test_font())
        .with_step(0.0)
        .with_smart_aim(false)
        .with_trailing_fill(false)
        .with_show_value(false)
        .with_style(style);
    let _ = s.layout(Size::new(W, WIDGET_H));
    s.set_bounds(Rect::new(0.0, 0.0, W, WIDGET_H));
    s
}

/// A laid-out vertical slider, otherwise as [`slider`].
fn vertical(value: f64, style: SliderStyle) -> Slider {
    let mut s = Slider::new(value, 0.0, 1.0, test_font())
        .with_step(0.0)
        .with_smart_aim(false)
        .with_trailing_fill(false)
        .with_show_value(false)
        .with_orientation(SliderOrientation::Vertical)
        .with_style(style);
    let size = s.layout(Size::new(40.0, 400.0));
    s.set_bounds(Rect::new(0.0, 0.0, size.width, size.height));
    s
}

fn paint(s: &mut Slider) -> crate::Framebuffer {
    let b = s.bounds();
    let mut fb = crate::Framebuffer::new(b.width as u32, b.height as u32);
    {
        let mut ctx = crate::GfxCtx::new(&mut fb);
        ctx.clear(crate::Color::black());
        paint_subtree(s, &mut ctx);
    }
    fb
}

fn px(fb: &crate::Framebuffer, x: f64, y: f64) -> [u8; 3] {
    let i = ((y as u32 * fb.width() + x as u32) * 4) as usize;
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

fn center(r: Rect) -> (f64, f64) {
    (r.x + r.width * 0.5, r.y + r.height * 0.5)
}

/// The full-length rail runs from the widget's left edge to its right one,
/// where the default rail stops a thumb radius short of each end.
#[test]
fn full_length_track_spans_the_whole_width() {
    let cy = WIDGET_H * 0.5;
    let mut full = slider(0.5, marker_style(true));
    let fb = paint(&mut full);
    assert_eq!(full.rail_ends(), (0.0, W));
    for x in [0.0, W - 1.0] {
        assert!(
            near(px(&fb, x, cy), rgb8(RED)),
            "x {x}: {:?}",
            px(&fb, x, cy)
        );
    }

    let mut inset = slider(0.5, marker_style(false));
    let fb = paint(&mut inset);
    assert_eq!(inset.rail_ends(), (THUMB_R, W - THUMB_R));
    for x in [0.0, W - 1.0] {
        assert!(near(px(&fb, x, cy), BLACK), "x {x}: {:?}", px(&fb, x, cy));
    }
}

/// agg-sharp's geometry: at the minimum the thumb's leading edge is the
/// rail's start, at the maximum its trailing edge is the rail's end, so the
/// thumb's centre travels the drawn rail less one thumb width.
#[test]
fn full_length_track_holds_the_thumb_inside_it_at_both_ends() {
    let cy = WIDGET_H * 0.5;
    let mut min = slider(0.0, marker_style(true));
    let fb = paint(&mut min);
    let (start, end) = min.rail_ends();
    let t = min.thumb_rect();
    assert_eq!(t.x, start);
    assert!(
        near(px(&fb, 1.0, cy), rgb8(GREEN)),
        "{:?}",
        px(&fb, 1.0, cy)
    );

    let mut max = slider(1.0, marker_style(true));
    let fb = paint(&mut max);
    let t = max.thumb_rect();
    assert_eq!(t.x + t.width, end);
    assert!(
        near(px(&fb, W - 2.0, cy), rgb8(GREEN)),
        "{:?}",
        px(&fb, W - 2.0, cy)
    );

    let (p0, p1) = max.position_range();
    assert_eq!((end - start) - t.width, p1 - p0);
}

/// The trailing fill starts where the rail does.
#[test]
fn full_length_track_fill_starts_at_the_rail_start() {
    let cy = WIDGET_H * 0.5;
    for (full, expected) in [(true, rgb8(BLUE)), (false, BLACK)] {
        let mut s = slider(0.5, marker_style(full));
        s.set_trailing_fill(true);
        let fb = paint(&mut s);
        assert!(
            near(px(&fb, 0.0, cy), expected),
            "full {full}: {:?}",
            px(&fb, 0.0, cy)
        );
        assert!(near(px(&fb, 50.0, cy), rgb8(BLUE)), "full {full}");
    }
}

/// A vertical full-length rail runs the slider's whole height and holds
/// the thumb inside it at both ends too.
#[test]
fn full_length_track_spans_the_whole_height_when_vertical() {
    let mut s = vertical(0.5, marker_style(true));
    let h = s.bounds().height;
    let fb = paint(&mut s);
    let (a, b) = s.rail_ends();
    assert_eq!((a.min(b), a.max(b)), (0.0, h));
    let cx = THUMB_R;
    for y in [0.0, h - 1.0] {
        assert!(
            near(px(&fb, cx, y), rgb8(RED)),
            "y {y}: {:?}",
            px(&fb, cx, y)
        );
    }
    let mut inset = vertical(0.5, marker_style(false));
    let fb = paint(&mut inset);
    for y in [0.0, h - 1.0] {
        assert!(near(px(&fb, cx, y), BLACK), "y {y}: {:?}", px(&fb, cx, y));
    }

    for value in [0.0, 1.0] {
        let s = vertical(value, marker_style(true));
        let t = s.thumb_rect();
        assert!(t.y >= 0.0 && t.y + t.height <= h, "value {value}: {t:?}");
        assert!(t.y == 0.0 || t.y + t.height == h, "value {value}: {t:?}");
    }
}

/// A press on the drawn thumb's centre takes hold of it without moving it,
/// and a drag moves it with the pointer: 30 px of drag is 30 px of thumb
/// and 30 px of the centre's travel in value, whichever rail is drawn.
#[test]
fn a_press_on_the_drawn_thumb_grabs_it_in_both_modes() {
    for full in [false, true] {
        let mut s = slider(0.3, marker_style(full));
        let (tx, ty) = center(s.thumb_rect());
        let r = s.on_event(&Event::MouseDown {
            pos: Point::new(tx, ty),
            button: MouseButton::Left,
            modifiers: Default::default(),
        });
        assert_eq!(r, EventResult::Consumed);
        assert!(s.dragging, "full {full}");
        assert!((s.value() - 0.3).abs() < 1e-9, "full {full}: {}", s.value());

        s.on_event(&Event::MouseMove {
            pos: Point::new(tx + 30.0, ty),
        });
        let (moved, _) = center(s.thumb_rect());
        assert!((moved - (tx + 30.0)).abs() < 1e-9, "full {full}: {moved}");

        let (start, end) = s.rail_ends();
        let drawn = end - start;
        let travel = if full {
            drawn - s.thumb_rect().width
        } else {
            drawn
        };
        assert!(
            (s.value() - 0.3 - 30.0 / travel).abs() < 1e-9,
            "full {full}: {}",
            s.value()
        );
    }
}

/// `set_style` and `set_trailing_fill` restyle a live slider: the next
/// paint shows the change, and each asks for a redraw.
#[test]
fn set_style_and_set_trailing_fill_change_the_next_paint() {
    let cy = WIDGET_H * 0.5;
    let mut s = slider(1.0, SliderStyle::default());
    s.set_trailing_fill(true);
    let v = crate::theme::current_visuals();
    let fb = paint(&mut s);
    assert!(near(px(&fb, 100.0, cy), rgb8(v.accent)));

    let epoch = crate::animation::invalidation_epoch();
    s.set_trailing_fill(false);
    assert!(!s.trailing_fill());
    assert!(crate::animation::invalidation_epoch() > epoch);
    let fb = paint(&mut s);
    assert!(near(px(&fb, 100.0, cy), rgb8(v.track_bg)));

    let epoch = crate::animation::invalidation_epoch();
    s.set_style(marker_style(true));
    assert_eq!(s.style(), marker_style(true));
    assert!(crate::animation::invalidation_epoch() > epoch);
    let fb = paint(&mut s);
    assert!(near(px(&fb, 100.0, cy), rgb8(RED)));
    assert!(near(px(&fb, 0.0, cy), rgb8(RED)));
}
