//! Tests for a selected hosted card's selection ring (`hosted_card.rs`),
//! rendered in software: the ring runs just outside the drawn card, 3 units
//! wide in the accent with a soft glow beyond it, so selecting a card adds a
//! ring round it and leaves the card itself, its 1 px border included,
//! exactly as it was. Both in canvas units, so they follow the zoom.

use agg_gui::{Framebuffer, GfxCtx, Size};

use super::hosted::HostedNodeBody;
use super::tests_common::{mk_node, Memory};
use super::tests_socket_at::EmptyBody;
use super::*;
use crate::draw::NODE_RADIUS;
use crate::model::NodeView;

const VIEW: Size = Size {
    width: 800.0,
    height: 600.0,
};
/// The card's size in canvas units, even so its edges land on pixel
/// boundaries at zoom 0.5 too.
const CARD: [f64; 2] = [160.0, 100.0];
/// Where the card's bottom-left lands in the framebuffer.
const ORIGIN: [f64; 2] = [20.0, 20.0];
const FB: [u32; 2] = [240, 160];

/// One frame of a lone hosted card, painted through a layer-like
/// translate + `zoom`, `selected` or not.
fn render(zoom: f64, selected: bool) -> Framebuffer {
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
    card.chrome.selected = selected;
    let mut fb = Framebuffer::new(FB[0], FB[1]);
    {
        let mut ctx = GfxCtx::new(&mut fb);
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

/// Framebuffer pixels, `(x, y)` from the bottom-left.
type Pixels = Vec<(u32, u32)>;

/// The pixels selecting the card changes, and whether each lies inside
/// the drawn card.
fn ring_pixels(zoom: f64) -> (Pixels, Pixels) {
    let plain = render(zoom, false);
    let ringed = render(zoom, true);
    let (x0, y0) = (ORIGIN[0], ORIGIN[1]);
    let (x1, y1) = (x0 + CARD[0] * zoom, y0 + CARD[1] * zoom);
    let (mut outside, mut inside) = (Vec::new(), Vec::new());
    for y in 0..FB[1] {
        for x in 0..FB[0] {
            if rgba(&plain, x, y) == rgba(&ringed, x, y) {
                continue;
            }
            if within_card(x, y, [x0, y0, x1, y1], NODE_RADIUS * zoom) {
                inside.push((x, y));
            } else {
                outside.push((x, y));
            }
        }
    }
    (outside, inside)
}

/// Whether pixel `(x, y)` shows the drawn card: along the straight edges
/// (which land on pixel boundaries) any pixel that overlaps the card; in a
/// rounded corner, a pixel wholly inside the corner's arc. A pixel the arc
/// crosses is shared by the card's anti-aliased edge and the ring's.
fn within_card(x: u32, y: u32, [x0, y0, x1, y1]: [f64; 4], r: f64) -> bool {
    let (px, py) = (x as f64, y as f64);
    if !(px + 1.0 > x0 && px < x1 && py + 1.0 > y0 && py < y1) {
        return false;
    }
    let (cx, cy) = (px + 0.5, py + 0.5);
    let ax = if cx < x0 + r {
        x0 + r
    } else if cx > x1 - r {
        x1 - r
    } else {
        return true;
    };
    let ay = if cy < y0 + r {
        y0 + r
    } else if cy > y1 - r {
        y1 - r
    } else {
        return true;
    };
    (cx - ax).hypot(cy - ay) + std::f64::consts::FRAC_1_SQRT_2 <= r
}

fn assert_ring_outside(zoom: f64) {
    let (outside, inside) = ring_pixels(zoom);
    assert!(
        inside.is_empty(),
        "selecting the card changes nothing inside it, its border included \
         (zoom {zoom}: {} pixels changed, first {:?})",
        inside.len(),
        inside.first()
    );
    // The ring runs the length of the card's top edge, just above it.
    let top = (ORIGIN[1] + CARD[1] * zoom) as u32;
    let mid = (ORIGIN[0] + CARD[0] * zoom * 0.5) as u32;
    assert!(
        outside.contains(&(mid, top)),
        "the ring runs just above the card's top edge (zoom {zoom})"
    );
    let along_top = outside.iter().filter(|(_, y)| *y == top).count();
    assert!(
        along_top as f64 > CARD[0] * zoom * 0.8,
        "the ring runs the length of the top edge (zoom {zoom}: {along_top} pixels)"
    );
    // 3 units wide, starting 0.5 out, then 4 units of glow: nothing
    // beyond 7.5 units (plus the anti-aliased pixel the edge falls in).
    let reach = (7.5 * zoom).ceil() as u32 + 1;
    assert!(
        outside.iter().all(|(_, y)| *y < top + reach),
        "the ring and its glow stay within 7.5 units of the card (zoom {zoom})"
    );
}

#[test]
fn a_selected_cards_ring_lies_outside_it_at_zoom_1() {
    assert_ring_outside(1.0);
}

#[test]
fn a_selected_cards_ring_lies_outside_it_at_zoom_half() {
    assert_ring_outside(0.5);
}

/// The theme's accent as 0-255 channels.
fn accent_rgb() -> [i32; 3] {
    let accent = agg_gui::theme::current_visuals().accent;
    [
        (accent.r * 255.0).round() as i32,
        (accent.g * 255.0).round() as i32,
        (accent.b * 255.0).round() as i32,
    ]
}

fn is_accent(p: [u8; 4]) -> bool {
    let want = accent_rgb();
    (0..3).all(|c| (p[c] as i32 - want[c]).abs() <= 2)
}

/// At zoom 1 the ring covers the pixel rows 1 to 3 units above the card
/// whole, in the theme's accent colour: a ring 3 units wide, which reads at
/// a glance where a 2 unit one was easy to miss.
#[test]
fn a_selected_cards_ring_is_the_accent_colour_three_units_wide() {
    let ringed = render(1.0, true);
    let top = (ORIGIN[1] + CARD[1]) as u32;
    let mid = (ORIGIN[0] + CARD[0] * 0.5) as u32;
    for row in [top + 1, top + 2] {
        let p = rgba(&ringed, mid, row);
        assert!(
            is_accent(p),
            "ring pixel {p:?} at row {row}, accent {:?}",
            accent_rgb()
        );
    }
}

/// Past the ring a soft glow of the accent fades out: the rows beyond it
/// are tinted (they differ from the unselected card's) but are not the
/// solid accent, and each is fainter than the one inside it.
#[test]
fn a_selected_cards_ring_has_a_soft_glow() {
    let plain = render(1.0, false);
    let ringed = render(1.0, true);
    let top = (ORIGIN[1] + CARD[1]) as u32;
    let mid = (ORIGIN[0] + CARD[0] * 0.5) as u32;
    let alpha = |row: u32| rgba(&ringed, mid, row)[3] as i32 - rgba(&plain, mid, row)[3] as i32;
    let glow: Vec<i32> = (top + 4..top + 8).map(alpha).collect();
    let ring = alpha(top + 1);
    for (i, row) in (top + 4..top + 8).enumerate() {
        assert!(glow[i] > 0, "the glow tints row {row}: {glow:?}");
        assert!(
            glow[i] < ring,
            "row {row} is glow, fainter than the solid ring ({glow:?}, ring {ring})"
        );
    }
    assert!(
        glow.windows(2).all(|w| w[0] > w[1]),
        "the glow fades outward: {glow:?}"
    );
}
