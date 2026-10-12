//! Port of agg-sharp `Agg.GoldenImages/GpuFillRuleTests.cs`: the fill rule
//! on `WgpuGfxCtx` against a real device, compared with the software
//! rasterizer filling the same path under the same rule.
//!
//! Two circles wound the same way, overlapping: non-zero fills the overlap,
//! even-odd leaves it empty. The GPU anti-aliases with a halo, not AGG's
//! area coverage, so an edge pixel can be off by half the shade's range;
//! `harness::EDGE_TOLERANCE` (C#'s `Tolerance`) allows that, and is still
//! well under the full range a wrong rule gets wrong over the whole overlap.
//! C#'s `RenderInRect` of a `ColoredVertexSource` marked even-odd is
//! agg-gui's `VectorIcon::paint` of an even-odd `IconPath`.

use agg_gui::color::Color;
use agg_gui::draw_ctx::DrawCtx;
use agg_gui::geometry::Rect;
use agg_gui::paints::FillRule;
use agg_gui::vector_icon::{IconColor, VectorIcon};

use super::harness::{at, gpu, render_both};

const W: u32 = 120;
const H: u32 = 80;

fn shade() -> Color {
    Color::from_rgba8(30, 60, 90, 255)
}

/// C#'s `new Ellipse(x, y, r, r, 64)`: 64 steps counter-clockwise from +x.
fn ellipse(ctx: &mut dyn DrawCtx, cx: f64, cy: f64, r: f64) {
    for i in 0..64 {
        let a = i as f64 / 64.0 * 2.0 * std::f64::consts::PI;
        let (x, y) = (cx + a.cos() * r, cy + a.sin() * r);
        if i == 0 {
            ctx.move_to(x, y);
        } else {
            ctx.line_to(x, y);
        }
    }
    ctx.close_path();
}

fn two_circles(ctx: &mut dyn DrawCtx) {
    ctx.begin_path();
    ellipse(ctx, 45.0, 40.0, 25.0);
    ellipse(ctx, 75.0, 40.0, 25.0);
}

/// Pixel `(x, y)` with y up, as C#'s `GetPixel`.
fn pixel(data: &[u8], x: u32, y: u32) -> [u8; 4] {
    at(data, W, x, H - 1 - y)
}

fn as_bytes(c: Color) -> [u8; 4] {
    let c = c.to_rgba8();
    [c.r, c.g, c.b, c.a]
}

/// C#'s `[Arguments(fill_even_odd)]` and `[Arguments(fill_non_zero)]`.
#[test]
fn overlap_fills_as_software_does() {
    for rule in [FillRule::EvenOdd, FillRule::NonZero] {
        let Some(pair) = render_both(W, H, Color::white(), &|ctx| {
            ctx.set_fill_rule(rule);
            ctx.set_fill_color(shade());
            two_circles(ctx);
            ctx.fill();
            ctx.set_fill_rule(FillRule::NonZero);
        }) else {
            return;
        };
        // The middle of the overlap: empty under even-odd, filled under non-zero.
        assert_eq!(
            pixel(&pair.gpu, 60, 40),
            pixel(&pair.software, 60, 40),
            "{rule:?}"
        );
        let expected = match rule {
            FillRule::EvenOdd => as_bytes(Color::white()),
            FillRule::NonZero => as_bytes(shade()),
        };
        assert_eq!(pixel(&pair.software, 60, 40), expected, "{rule:?}");
        pair.assert_matches(&format!("overlap_{rule:?}"), 2);
    }
}

/// A cached tessellation of the same geometry must not carry the first
/// rule's triangles into a fill under the other rule.
#[test]
fn same_path_under_the_other_rule_is_tessellated_again() {
    let Some(image) = gpu(W, H, Color::white(), &|ctx| {
        ctx.set_fill_color(shade());
        ctx.set_fill_rule(FillRule::EvenOdd);
        two_circles(ctx);
        ctx.fill();
        ctx.set_fill_rule(FillRule::NonZero);
        two_circles(ctx);
        ctx.fill();
    }) else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    assert_eq!(pixel(&image, 60, 40), as_bytes(shade()));
}

/// The two circles as an even-odd icon path, in an 80-unit view box painted
/// into the square (20, 0)–(100, 80), so they land where `two_circles` puts
/// them.
fn even_odd_icon() -> VectorIcon {
    let d = "M0,40 A25,25 0 1,0 50,40 A25,25 0 1,0 0,40 Z \
             M30,40 A25,25 0 1,0 80,40 A25,25 0 1,0 30,40 Z";
    VectorIcon::new(80.0)
        .with_svg_path(d, IconColor::Literal(shade()), FillRule::EvenOdd)
        .expect("parse the two circles")
}

/// The icon fills by its own rule on the GPU, and puts non-zero back after:
/// the same circles filled next, with no rule set, fill their overlap.
#[test]
fn render_in_rect_fills_even_odd_sources_by_their_rule() {
    let icon = even_odd_icon();
    let rect = Rect::new(20.0, 0.0, 80.0, 80.0);
    let Some(image) = gpu(W, H, Color::white(), &|ctx| {
        icon.paint(ctx, rect, Color::black())
    }) else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    assert_eq!(pixel(&image, 60, 40), as_bytes(Color::white()));

    let Some(after) = gpu(W, H, Color::white(), &|ctx| {
        icon.paint(ctx, rect, Color::black());
        ctx.set_fill_color(shade());
        two_circles(ctx);
        ctx.fill();
    }) else {
        return;
    };
    assert_eq!(pixel(&after, 60, 40), as_bytes(shade()));
}
