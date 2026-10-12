//! Port of agg-sharp `Agg.GoldenImages/ImageRenderTransformTests.cs`: an
//! image lands where a vector fill of its rectangle would, under the full
//! transform composed with the image's own placement, in software and on
//! the GPU alike.
//!
//! C#'s `Render(image, x, y, angle, scaleX, scaleY)` is, on a `DrawCtx`,
//! `translate(x, y)`, `rotate(angle)`, `scale(scaleX, scaleY)` and then
//! `draw_image_rgba` into the image's own rectangle at the origin — the same
//! composed `scale * rotate * translate * graphics` transform.

use agg_gui::color::Color;
use agg_gui::draw_ctx::DrawCtx;
use agg_rust::trans_affine::TransAffine;

use super::harness::{at, gpu, software};

const FRAME: u32 = 96;
const IMAGE_W: u32 = 16;
const IMAGE_H: u32 = 8;

/// One placement of the image under one graphics transform.
struct Placement {
    name: &'static str,
    graphics: TransAffine,
    x: f64,
    y: f64,
    angle: f64,
    scale_x: f64,
    scale_y: f64,
}

fn then(mut a: TransAffine, b: TransAffine) -> TransAffine {
    a.multiply(&b);
    a
}

fn placements() -> Vec<Placement> {
    let p = |name, graphics, x, y, angle, scale_x, scale_y| Placement {
        name,
        graphics,
        x,
        y,
        angle,
        scale_x,
        scale_y,
    };
    vec![
        p("identity", TransAffine::new(), 5.0, 6.0, 0.0, 1.0, 1.0),
        p(
            "uniform scale 2",
            TransAffine::new_scaling(2.0, 2.0),
            5.0,
            6.0,
            0.0,
            1.0,
            1.0,
        ),
        p(
            "translate and scale",
            then(
                TransAffine::new_scaling(1.5, 2.0),
                TransAffine::new_translation(10.0, 4.0),
            ),
            3.0,
            2.0,
            0.0,
            1.25,
            1.0,
        ),
        p(
            "rotation",
            then(
                TransAffine::new_rotation(30.0_f64.to_radians()),
                TransAffine::new_translation(40.0, 5.0),
            ),
            4.0,
            3.0,
            0.2,
            1.5,
            1.5,
        ),
        p(
            "y flip",
            then(
                TransAffine::new_scaling(1.0, -1.0),
                TransAffine::new_translation(10.0, 80.0),
            ),
            6.0,
            20.0,
            0.3,
            2.0,
            2.0,
        ),
    ]
}

/// Left half red, right half blue (RGBA8, top row first), so a flip or a
/// turn shows as well as a size.
fn two_tone_image() -> Vec<u8> {
    let mut out = Vec::with_capacity((IMAGE_W * IMAGE_H * 4) as usize);
    for _ in 0..IMAGE_H {
        for x in 0..IMAGE_W {
            out.extend_from_slice(if x < IMAGE_W / 2 {
                &[255, 0, 0, 255]
            } else {
                &[0, 0, 255, 255]
            });
        }
    }
    out
}

fn place(ctx: &mut dyn DrawCtx, p: &Placement) {
    ctx.set_transform(p.graphics);
    ctx.translate(p.x, p.y);
    ctx.rotate(p.angle);
    ctx.scale(p.scale_x, p.scale_y);
}

fn fill_rect(ctx: &mut dyn DrawCtx, x: f64, w: f64, color: Color) {
    ctx.set_fill_color(color);
    ctx.begin_path();
    ctx.rect(x, 0.0, w, IMAGE_H as f64);
    ctx.fill();
}

#[test]
fn image_follows_the_transform_like_a_vector_fill() {
    let image = two_tone_image();
    for p in placements() {
        let half = (IMAGE_W / 2) as f64;
        let reference = software(FRAME, FRAME, Color::white(), &|ctx| {
            place(ctx, &p);
            fill_rect(ctx, 0.0, half, Color::rgb(1.0, 0.0, 0.0));
            fill_rect(ctx, half, half, Color::rgb(0.0, 0.0, 1.0));
        });
        let draw = |ctx: &mut dyn DrawCtx| {
            place(ctx, &p);
            let (w, h) = (IMAGE_W as f64, IMAGE_H as f64);
            ctx.draw_image_rgba(&image, IMAGE_W, IMAGE_H, 0.0, 0.0, w, h);
        };
        let soft = software(FRAME, FRAME, Color::white(), &draw);
        assert_matches_reference(&reference, &soft, &format!("software, {}", p.name));
        let Some(gpu_image) = gpu(FRAME, FRAME, Color::white(), &draw) else {
            eprintln!("SKIP: no GPU adapter");
            return;
        };
        assert_matches_reference(&reference, &gpu_image, &format!("gpu, {}", p.name));
    }
}

/// Every pixel whose 3×3 neighbourhood is one solid colour in the vector
/// reference must be that colour in `actual`; the one-pixel band along
/// every edge is left free, where anti-aliasing and image filtering
/// legitimately differ.
fn assert_matches_reference(reference: &[u8], actual: &[u8], what: &str) {
    let mut checked = 0;
    for y in 1..FRAME - 1 {
        for x in 1..FRAME - 1 {
            let expected = at(reference, FRAME, x, y);
            let solid = (y - 1..=y + 1)
                .all(|ny| (x - 1..=x + 1).all(|nx| at(reference, FRAME, nx, ny) == expected));
            if !solid {
                continue;
            }
            checked += 1;
            let got = at(actual, FRAME, x, y);
            let delta = (0..3)
                .map(|c| (expected[c] as i32 - got[c] as i32).abs())
                .max()
                .unwrap_or(0);
            assert!(
                delta <= 8,
                "{what}: at ({x}, {y}) top-row-first expected {expected:?} got {got:?}"
            );
        }
    }
    assert!(
        checked > FRAME * FRAME / 2,
        "{what}: only {checked} pixels checked"
    );
}
