//! Tests for a hosted card's body and title-bar fills (`hosted_card.rs`),
//! rendered in software: the body fills the whole card, title strip
//! included, and the title colour lies over it, as agg-sharp's
//! `WindowWidget.DrawBody` paints a MatterCAD node window. With a
//! translucent title tint (MatterCAD's `MinimalShade`, #0000000F, over a
//! body of `BackgroundColor` at alpha 235) the title bar reads as a light
//! tint of the body, not as the tint over the canvas and the card's shadow.

use agg_gui::{Color, Framebuffer, GfxCtx, Size};

use super::hosted::HostedNodeBody;
use super::tests_common::{mk_node, Memory};
use super::tests_socket_at::EmptyBody;
use super::*;
use crate::draw::TITLE_HEIGHT;
use crate::model::NodeView;

const VIEW: Size = Size {
    width: 800.0,
    height: 600.0,
};
const CARD: [f64; 2] = [160.0, 100.0];
const ORIGIN: [f64; 2] = [20.0, 20.0];
const FB: [u32; 2] = [240, 160];
/// The dark canvas under the card, as 0..255 grey.
const CANVAS: f64 = 30.0;
/// The card body's grey and alpha (MatterCAD's `BackgroundColor` at 235).
const BODY: (f64, f64) = (240.0, 235.0);
/// The title tint's alpha (MatterCAD's `MinimalShade`, black at 15).
const TITLE_ALPHA: f64 = 15.0;

/// One frame of a lone hosted card with a translucent body and title over
/// a dark canvas, painted through a translate + `zoom`.
fn render(zoom: f64) -> Framebuffer {
    let m = Memory {
        nodes: vec![mk_node(1, "Box", [100.0, 400.0])],
        ..Memory::default()
    };
    let shared: SharedModel = Arc::new(Mutex::new(m));
    let mut editor = NodeEditor::new(shared).with_body_factory(|_n: &NodeView| {
        Some(HostedNodeBody::new(Box::new(EmptyBody::default())))
    });
    editor.set_bounds(Rect::new(0.0, 0.0, VIEW.width, VIEW.height));
    editor.layout(VIEW);
    let card = editor
        .children_mut()
        .last_mut()
        .and_then(|l| l.children_mut().first_mut())
        .and_then(|c| c.as_any_mut())
        .and_then(|a| a.downcast_mut::<HostedCard>())
        .expect("hosted card");
    card.set_bounds(Rect::new(0.0, 0.0, CARD[0], CARD[1]));
    let g = (BODY.0 / 255.0) as f32;
    card.chrome.body = Color::rgba(g, g, g, (BODY.1 / 255.0) as f32);
    card.chrome.title_color = Color::rgba(0.0, 0.0, 0.0, (TITLE_ALPHA / 255.0) as f32);
    // An invisible shadow, so only the canvas lies under the card.
    card.chrome.shadow = Some(CardShadow {
        blur: 3.5,
        offset: [0.0, -1.5],
        color: Some(Color::rgba(0.0, 0.0, 0.0, 0.0)),
    });
    let mut fb = Framebuffer::new(FB[0], FB[1]);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        let c = (CANVAS / 255.0) as f32;
        ctx.set_fill_color(Color::rgb(c, c, c));
        ctx.begin_path();
        ctx.rect(0.0, 0.0, FB[0] as f64, FB[1] as f64);
        ctx.fill();
        ctx.translate(ORIGIN[0], ORIGIN[1]);
        ctx.scale(zoom, zoom);
        card.paint(&mut ctx);
    }
    fb
}

fn rgba(fb: &Framebuffer, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * fb.width() + x) * 4) as usize;
    let p = &fb.pixels()[i..i + 4];
    [p[0], p[1], p[2], p[3]]
}

/// `top` with alpha `a` (0..255) over `under`, per channel.
fn over(top: f64, a: f64, under: f64) -> f64 {
    under + (top - under) * a / 255.0
}

fn assert_grey(p: [u8; 4], want: f64, what: &str) {
    for (c, &v) in p.iter().take(3).enumerate() {
        assert!(
            (v as f64 - want).abs() <= 2.0,
            "{what}: channel {c} is {v}, want {want:.1} (pixel {p:?})"
        );
    }
}

/// The body pixel is the body over the canvas; the title pixel, clear of
/// the label and the separator, is the title tint over that body.
fn assert_title_over_body(zoom: f64) {
    let fb = render(zoom);
    let x = (ORIGIN[0] + (CARD[0] - 20.0) * zoom) as u32;
    let body_y = (ORIGIN[1] + CARD[1] * 0.4 * zoom) as u32;
    let title_y = (ORIGIN[1] + (CARD[1] - TITLE_HEIGHT * 0.5) * zoom) as u32;
    let body = over(BODY.0, BODY.1, CANVAS);
    assert_grey(rgba(&fb, x, body_y), body, &format!("body at zoom {zoom}"));
    assert_grey(
        rgba(&fb, x, title_y),
        over(0.0, TITLE_ALPHA, body),
        &format!("title bar at zoom {zoom}"),
    );
}

#[test]
fn a_translucent_title_lies_over_the_body_at_zoom_1() {
    assert_title_over_body(1.0);
}

#[test]
fn a_translucent_title_lies_over_the_body_at_zoom_half() {
    assert_title_over_body(0.5);
}
