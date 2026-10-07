//! Tests for [`super::ToggleSwitch`]'s per-instance [`ToggleSwitchStyle`]:
//! the default geometry and colours are unchanged, and each override is
//! honoured by the production `layout` / `paint` / `hit_test` paths.

use super::*;
use crate::geometry::Point;
use crate::widget::paint_subtree;

fn render(sw: &mut ToggleSwitch) -> crate::Framebuffer {
    let size = sw.layout(Size::new(200.0, 200.0));
    sw.set_bounds(Rect::new(0.0, 0.0, size.width, size.height));
    let mut fb = crate::Framebuffer::new(size.width.ceil() as u32, size.height.ceil() as u32);
    {
        let mut ctx = crate::GfxCtx::new(&mut fb);
        ctx.clear(Color::black());
        paint_subtree(sw, &mut ctx);
    }
    fb
}

fn px(fb: &crate::Framebuffer, x: f64, y: f64) -> [u8; 3] {
    let i = ((y as u32 * fb.width() + x as u32) * 4) as usize;
    let p = &fb.pixels()[i..i + 4];
    [p[0], p[1], p[2]]
}

/// `c` composited over the black background the tests clear to.
fn rgb8(c: Color) -> [u8; 3] {
    [
        (c.r * c.a * 255.0).round() as u8,
        (c.g * c.a * 255.0).round() as u8,
        (c.b * c.a * 255.0).round() as u8,
    ]
}

fn near(a: [u8; 3], b: [u8; 3]) -> bool {
    a.iter().zip(b.iter()).all(|(x, y)| x.abs_diff(*y) <= 3)
}

#[test]
fn default_geometry_is_unchanged() {
    for mut sw in [
        ToggleSwitch::new(false),
        ToggleSwitch::new(false).with_style(ToggleSwitchStyle::default()),
    ] {
        assert_eq!(sw.layout(Size::new(500.0, 500.0)), Size::new(34.0, 20.0));
        assert_eq!(sw.circle_cx_at(0.0), PILL_HALO + CIRCLE_MARGIN + CIRCLE_R);
        assert_eq!(
            sw.circle_cx_at(1.0),
            PILL_HALO + PILL_W - CIRCLE_MARGIN - CIRCLE_R
        );
        assert_eq!(sw.circle_cy(), PILL_HALO + PILL_H * 0.5);
        assert_eq!(sw.knob_r(), CIRCLE_R);
        assert!(!sw.hit_test(Point::new(0.5, 10.0)));
        assert!(sw.hit_test(Point::new(1.5, 10.0)));
    }
}

#[test]
fn default_colours_are_unchanged() {
    let v = crate::theme::current_visuals();
    let mut off = ToggleSwitch::new(false);
    let fb = render(&mut off);
    let cy = PILL_HALO + PILL_H * 0.5;
    // Knob centre is white; the bar beyond the knob is widget_stroke.
    assert!(near(px(&fb, off.circle_cx_at(0.0), cy), [255, 255, 255]));
    assert!(
        near(px(&fb, 26.0, cy), rgb8(v.widget_stroke)),
        "{:?}",
        px(&fb, 26.0, cy)
    );

    let mut on = ToggleSwitch::new(true);
    let fb = render(&mut on);
    assert!(
        near(px(&fb, 6.0, cy), rgb8(v.accent)),
        "{:?}",
        px(&fb, 6.0, cy)
    );
}

/// MatterCAD's `RoundedToggleSwitch` look: a grey knob larger than a light
/// bar; the knob overhangs the bar and the widget grows to keep it inside.
#[test]
fn style_colours_and_oversized_knob() {
    let knob = Color::rgb(
        0x99 as f32 / 255.0,
        0x99 as f32 / 255.0,
        0x99 as f32 / 255.0,
    );
    let bar = Color::rgb(
        0xdd as f32 / 255.0,
        0xdd as f32 / 255.0,
        0xdd as f32 / 255.0,
    );
    let style = ToggleSwitchStyle {
        track_off: Some(bar),
        knob_off: Some(knob),
        track_width: Some(37.6),
        track_height: Some(12.6),
        knob_radius: Some(9.0),
        ..Default::default()
    };
    let mut sw = ToggleSwitch::new(false).with_style(style);
    // Overhang 9 - 6.3 = 2.7 per side, plus the 1 px halo.
    let size = sw.layout(Size::new(500.0, 500.0));
    assert!((size.width - (37.6 + 2.0 * 3.7)).abs() < 1e-9);
    assert!((size.height - 18.0 - 2.0).abs() < 1e-9);
    let fb = render(&mut sw);
    let cy = sw.circle_cy();
    assert!((cy - 10.0).abs() < 1e-9);
    let cx = sw.circle_cx_at(0.0);
    assert!(near(px(&fb, cx, cy), rgb8(knob)), "{:?}", px(&fb, cx, cy));
    // Above the bar but inside the knob: knob colour.
    assert!(
        near(px(&fb, cx, cy + 7.0), rgb8(knob)),
        "{:?}",
        px(&fb, cx, cy + 7.0)
    );
    // Far end of the bar: bar colour, even when hovered.
    sw.hovered = true;
    let fb = render(&mut sw);
    assert!(
        near(px(&fb, 30.0, cy), rgb8(bar)),
        "{:?}",
        px(&fb, 30.0, cy)
    );
    // The overhanging knob is hittable.
    assert!(sw.hit_test(Point::new(cx, cy + 8.5)));
}

#[test]
fn on_colours_and_outlines_are_used() {
    let red = Color::rgb(1.0, 0.0, 0.0);
    let green = Color::rgb(0.0, 1.0, 0.0);
    let blue = Color::rgb(0.0, 0.0, 1.0);
    let mut sw = ToggleSwitch::new(true).with_style(ToggleSwitchStyle {
        track_on: Some(red),
        knob_on: Some(green),
        track_outline: Some(blue),
        track_outline_width: Some(2.0),
        ..Default::default()
    });
    let fb = render(&mut sw);
    let cy = sw.circle_cy();
    assert!(near(px(&fb, sw.circle_cx_at(1.0), cy), rgb8(green)));
    assert!(near(px(&fb, 8.0, cy), rgb8(red)), "{:?}", px(&fb, 8.0, cy));
    // The outline straddles the bar's top edge (y = 19).
    assert!(
        near(px(&fb, 17.0, 18.5), rgb8(blue)),
        "{:?}",
        px(&fb, 17.0, 18.5)
    );
    assert_eq!(sw.ripple_color(&crate::theme::current_visuals()), green);
}
