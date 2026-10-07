//! Tests for the geometry and thumb options of [`super::SliderStyle`]
//! (`track_height`, `thumb_radius`, `thumb_ring_width`, `thumb_center`,
//! `thumb_hollow`, `fill`).  Split from `tests.rs`, which covers the colour
//! overrides and the slider's mapping/events; these paint through the
//! production `paint_subtree` path and read pixels back.

use super::*;
use crate::widget::paint_subtree;

const W: f64 = 200.0;

fn test_font() -> Arc<Font> {
    const FONT_BYTES: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");
    Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
}

/// Paint a slider at `value` (range 0..1) with `style` on black and return
/// the framebuffer plus the thumb's centre x.
fn render(value: f64, trailing: bool, style: SliderStyle) -> (crate::Framebuffer, f64) {
    let mut s = Slider::new(value, 0.0, 1.0, test_font())
        .with_trailing_fill(trailing)
        .with_show_value(false)
        .with_style(style);
    let _ = s.layout(Size::new(W, WIDGET_H));
    s.set_bounds(Rect::new(0.0, 0.0, W, WIDGET_H));
    let mut fb = crate::Framebuffer::new(W as u32, WIDGET_H as u32);
    {
        let mut ctx = crate::GfxCtx::new(&mut fb);
        ctx.clear(crate::Color::black());
        paint_subtree(&mut s, &mut ctx);
    }
    let tx = s.thumb_pos();
    (fb, tx)
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

/// An all-default style resolves to today's constants.
#[test]
fn default_style_geometry_is_unchanged() {
    let s = Slider::new(0.5, 0.0, 1.0, test_font());
    assert_eq!(s.track_height(), TRACK_H);
    assert_eq!(s.thumb_radius(), THUMB_R);
    assert_eq!(s.thumb_ring_width(), THUMB_RING_W);
    assert_eq!(s.track_radius(), TRACK_H * 0.5);
}

/// A 1 px track (MatterCAD's property sliders) leaves the row 1.5 px off
/// the centre line unpainted, where the default 4 px rail covers it.
#[test]
fn track_height_sets_the_rail_thickness() {
    let cy = WIDGET_H * 0.5;
    let style = SliderStyle {
        track: Some(RED),
        track_radius: Some(0.0),
        ..Default::default()
    };
    let (thick, _) = render(1.0, false, style);
    assert!(near(px(&thick, 100.0, cy + 1.0), rgb8(RED)));
    let (thin, _) = render(
        1.0,
        false,
        SliderStyle {
            track_height: Some(1.0),
            ..style
        },
    );
    // A set height snaps to whole pixels: one fully covered row (the row
    // starting at cy), nothing above or below it.
    assert!(
        near(px(&thin, 100.0, cy), rgb8(RED)),
        "{:?}",
        px(&thin, 100.0, cy)
    );
    assert!(
        near(px(&thin, 100.0, cy - 1.0), [0, 0, 0]),
        "{:?}",
        px(&thin, 100.0, cy - 1.0)
    );
    assert!(
        near(px(&thin, 100.0, cy + 1.0), [0, 0, 0]),
        "{:?}",
        px(&thin, 100.0, cy + 1.0)
    );
}

/// The thumb radius drives the track's end insets (and so the mapping), and
/// the ring is drawn at that radius.
#[test]
fn thumb_radius_moves_the_track_ends_and_sizes_the_ring() {
    let style = SliderStyle {
        thumb: Some(GREEN),
        thumb_radius: Some(5.0),
        thumb_ring_width: Some(2.0),
        ..Default::default()
    };
    let (fb, tx) = render(1.0, false, style);
    assert_eq!(tx, W - 5.0);
    let cy = WIDGET_H * 0.5;
    assert!(
        near(px(&fb, tx, cy + 3.0), rgb8(GREEN)),
        "{:?}",
        px(&fb, tx, cy + 3.0)
    );
    // Outside the 5 px radius nothing is painted.
    assert!(
        near(px(&fb, tx, cy + 6.0), [0, 0, 0]),
        "{:?}",
        px(&fb, tx, cy + 6.0)
    );

    let (_, t0) = render(0.0, false, style);
    assert_eq!(t0, 5.0);
}

/// `thumb_center` fills the disc inside the ring.
#[test]
fn thumb_center_fills_inside_the_ring() {
    let (fb, tx) = render(
        1.0,
        false,
        SliderStyle {
            thumb: Some(GREEN),
            thumb_center: Some(BLUE),
            ..Default::default()
        },
    );
    let cy = WIDGET_H * 0.5;
    assert!(
        near(px(&fb, tx, cy + 2.0), rgb8(BLUE)),
        "{:?}",
        px(&fb, tx, cy + 2.0)
    );
    assert!(
        near(px(&fb, tx, cy + 5.0), rgb8(GREEN)),
        "{:?}",
        px(&fb, tx, cy + 5.0)
    );
}

/// A hollow thumb is an outline: its centre shows what is beneath (here the
/// black background above the rail), its ring is the thumb colour.
#[test]
fn hollow_thumb_leaves_its_centre_unpainted() {
    let (fb, tx) = render(
        1.0,
        false,
        SliderStyle {
            thumb: Some(GREEN),
            thumb_center: Some(BLUE),
            thumb_hollow: true,
            ..Default::default()
        },
    );
    let cy = WIDGET_H * 0.5;
    assert!(
        near(px(&fb, tx, cy + 2.0), [0, 0, 0]),
        "{:?}",
        px(&fb, tx, cy + 2.0)
    );
    assert!(
        near(px(&fb, tx, cy + 5.0), rgb8(GREEN)),
        "{:?}",
        px(&fb, tx, cy + 5.0)
    );
}

/// `fill` colours the trailing fill.
#[test]
fn fill_colours_the_trailing_fill() {
    let cy = WIDGET_H * 0.5;
    let (fb, _) = render(
        1.0,
        true,
        SliderStyle {
            fill: Some(BLUE),
            ..Default::default()
        },
    );
    assert!(
        near(px(&fb, 100.0, cy), rgb8(BLUE)),
        "{:?}",
        px(&fb, 100.0, cy)
    );
    let (default, _) = render(1.0, true, SliderStyle::default());
    let v = crate::theme::current_visuals();
    assert!(near(px(&default, 100.0, cy), rgb8(v.accent)));
}
