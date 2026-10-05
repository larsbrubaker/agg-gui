//! `ComboBox::with_style` — per-instance closed-box styling.
//!
//! Paints a real `ComboBox` into a software framebuffer and samples pixels
//! to prove (a) an unstyled / all-`None` combo still paints from the global
//! `Visuals` at the default 24 px height, and (b) each `ComboBoxStyle`
//! override (fill, border, hover fill, radius, height) is honoured.

use super::*;
use crate::text::Font;
use crate::widget::{paint_subtree, Widget};
use crate::widgets::combo_box::ComboBoxStyle;
use crate::{ComboBox, Event};
use std::sync::Arc;

const W: f64 = 160.0;

fn combo() -> ComboBox {
    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    ComboBox::new(vec!["Alpha", "Beta"], 0, font)
}

fn render(combo: &mut ComboBox, fb_h: u32) -> Framebuffer {
    combo.layout(Size::new(W, fb_h as f64));
    let mut fb = Framebuffer::new(W as u32, fb_h);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::black());
        paint_subtree(combo, &mut ctx);
    }
    fb
}

fn sample(fb: &Framebuffer, x: u32, y: u32) -> [u8; 3] {
    let i = ((y * fb.width() + x) * 4) as usize;
    let p = &fb.pixels()[i..i + 4];
    [p[0], p[1], p[2]]
}

fn to_u8(c: Color) -> [u8; 3] {
    [
        (c.r * 255.0).round() as u8,
        (c.g * 255.0).round() as u8,
        (c.b * 255.0).round() as u8,
    ]
}

fn close(a: [u8; 3], b: [u8; 3]) -> bool {
    a.iter().zip(b.iter()).all(|(x, y)| x.abs_diff(*y) <= 3)
}

/// Interior pixel left of the label (label starts at x = 8), mid-height.
const INTERIOR: (u32, u32) = (4, 12);

#[test]
fn combo_default_style_matches_visuals_and_height() {
    let bg = to_u8(crate::theme::current_visuals().widget_bg);
    for mut c in [combo(), combo().with_style(ComboBoxStyle::default())] {
        assert_eq!(c.layout(Size::new(W, 60.0)).height, 24.0);
        let fb = render(&mut c, 30);
        let px = sample(&fb, INTERIOR.0, INTERIOR.1);
        assert!(close(px, bg), "default fill {px:?} != widget_bg {bg:?}");
        // Above the 24 px box stays clear.
        assert_eq!(sample(&fb, 80, 27), [0, 0, 0]);
    }
}

#[test]
fn combo_style_fill_border_and_hover_are_used() {
    let fill = Color::rgb(1.0, 0.0, 0.0);
    let hover = Color::rgb(0.0, 0.0, 1.0);
    let mut c = combo().with_style(ComboBoxStyle {
        fill: Some(fill),
        border: Some(Color::rgb(0.0, 1.0, 0.0)),
        hover_fill: Some(hover),
        radius: Some(0.0),
        ..Default::default()
    });
    let fb = render(&mut c, 30);
    let px = sample(&fb, INTERIOR.0, INTERIOR.1);
    assert!(close(px, to_u8(fill)), "fill override not used: {px:?}");
    // The 1 px outline straddles the left edge; green must dominate there.
    let edge = sample(&fb, 0, 12);
    assert!(
        edge[1] > 100 && edge[1] > edge[0] && edge[1] > edge[2],
        "border override not used: {edge:?}"
    );

    c.on_event(&Event::MouseMove {
        pos: crate::Point::new(4.0, 12.0),
    });
    let fb = render(&mut c, 30);
    let px = sample(&fb, INTERIOR.0, INTERIOR.1);
    assert!(close(px, to_u8(hover)), "hover fill not used: {px:?}");

    c.on_event(&Event::MouseMove {
        pos: crate::Point::new(4.0, 100.0),
    });
    let fb = render(&mut c, 30);
    let px = sample(&fb, INTERIOR.0, INTERIOR.1);
    assert!(
        close(px, to_u8(fill)),
        "fill not restored after hover: {px:?}"
    );
}

#[test]
fn combo_style_radius_and_height_are_used() {
    let fill = Color::rgb(1.0, 0.0, 0.0);
    let square = |r: f64| {
        combo().with_style(ComboBoxStyle {
            fill: Some(fill),
            border: Some(fill),
            radius: Some(r),
            ..Default::default()
        })
    };
    // Square corners fill the corner pixel; a 12 px radius leaves it empty.
    let fb = render(&mut square(0.0), 30);
    assert!(
        sample(&fb, 1, 1)[0] > 200,
        "radius 0 corner should be filled"
    );
    let fb = render(&mut square(12.0), 30);
    assert!(
        sample(&fb, 1, 1)[0] < 40,
        "radius 12 corner should be empty"
    );

    let mut tall = combo().with_style(ComboBoxStyle {
        fill: Some(fill),
        height: Some(40.0),
        ..Default::default()
    });
    assert_eq!(tall.layout(Size::new(W, 60.0)).height, 40.0);
    assert!(tall.hit_test(crate::Point::new(10.0, 35.0)));
    let mut short = combo();
    short.layout(Size::new(W, 60.0));
    assert!(!short.hit_test(crate::Point::new(10.0, 35.0)));
    let fb = render(&mut tall, 50);
    assert!(
        close(sample(&fb, 4, 36), to_u8(fill)),
        "tall box not painted"
    );
}
