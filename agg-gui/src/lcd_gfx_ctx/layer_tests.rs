//! Tests for `LcdGfxCtx`'s compositing layers (`layers.rs`): layer alpha,
//! the rounded layer clip (agg-sharp `RoundedClipCoverage`), HiDPI layer
//! sizing, and the plane-fill `clear_rect` against the trait's default
//! clipped Src fill. The older push/pop tests are in `tests.rs`.

use super::*;

fn px(buf: &LcdBuffer, x: u32, y: u32) -> ([u8; 3], [u8; 3]) {
    let i = ((y * buf.width() + x) * 3) as usize;
    let c = buf.color_plane();
    let a = buf.alpha_plane();
    ([c[i], c[i + 1], c[i + 2]], [a[i], a[i + 1], a[i + 2]])
}

/// The body of `DrawCtx::clear_rect`'s trait default, run on an
/// `LcdGfxCtx` through the production `defaults::clear_clip`.
fn default_clear_rect(ctx: &mut LcdGfxCtx, x: f64, y: f64, w: f64, h: f64, color: Color) {
    ctx.save();
    ctx.reset_transform();
    ctx.clip_rect(x, y, w, h);
    crate::draw_ctx::defaults::clear_clip(ctx, color);
    ctx.restore();
}

#[test]
fn rust_only_lcd_clear_rect_matches_the_default_src_fill() {
    let paint = |use_override: bool| {
        let mut buf = LcdBuffer::new(40, 30);
        {
            let mut ctx = LcdGfxCtx::new(&mut buf);
            ctx.clear(Color::white());
            ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, 1.0));
            ctx.set_font(Arc::new(
                Font::from_slice(include_bytes!("../../../demo/assets/CascadiaCode.ttf"))
                    .expect("font"),
            ));
            ctx.set_font_size(14.0);
            ctx.fill_text("Hg", 2.0, 8.0);
            // A transform and a clip are in force; clear_rect ignores the
            // transform and honours the clip.
            ctx.translate(3.0, 2.0);
            ctx.clip_rect(2.0, 1.0, 30.0, 20.0);
            let body = Color::from_rgb8(30, 140, 220);
            if use_override {
                ctx.clear_rect(4.0, 5.0, 40.0, 12.0, body);
            } else {
                default_clear_rect(&mut ctx, 4.0, 5.0, 40.0, 12.0, body);
            }
        }
        buf
    };
    let fast = paint(true);
    let slow = paint(false);
    let body = ([30, 140, 220], [255; 3]);
    // The cleared rectangle is the request (4..44, 5..17) cut by the clip
    // (5..35, 3..23): x 5..35, y 5..17.
    for y in 0..30 {
        for x in 0..40 {
            let inside = (5..35).contains(&x) && (5..17).contains(&y);
            let edge_column = x == 5 || x == 34;
            if inside {
                // Every cleared pixel holds the colour exactly, as
                // agg-sharp's `Clear` (a flat fill of the clip) leaves it.
                assert_eq!(px(&fast, x, y), body, "cleared pixel ({x}, {y})");
            }
            if !edge_column || !inside {
                // Everywhere else the plane fill and the default route
                // agree byte for byte. In the two edge columns the default
                // route's LCD filter reaches past the clip and leaves the
                // outer subpixel partly covered, which a clear must not.
                assert_eq!(px(&fast, x, y), px(&slow, x, y), "pixel ({x}, {y})");
            }
        }
    }
    // Text outside the clear survived.
    assert!(fast.color_plane().iter().any(|v| *v < 128));
}

#[test]
fn rust_only_lcd_clear_rect_replaces_with_a_translucent_colour() {
    let mut buf = LcdBuffer::new(10, 10);
    {
        let mut ctx = LcdGfxCtx::new(&mut buf);
        ctx.clear(Color::white());
        ctx.clear_rect(2.0, 2.0, 4.0, 4.0, Color::rgba(1.0, 0.0, 0.0, 0.5));
    }
    let mut want = LcdBuffer::new(1, 1);
    want.clear(Color::rgba(1.0, 0.0, 0.0, 0.5));
    let want = px(&want, 0, 0);
    assert_eq!(px(&buf, 3, 3), want, "inside: replaced, not blended");
    assert_eq!(px(&buf, 1, 3), ([255; 3], [255; 3]), "outside: untouched");
    assert_eq!(
        px(&buf, 6, 3),
        ([255; 3], [255; 3]),
        "right edge is exclusive"
    );
}

#[test]
fn rust_only_lcd_layer_alpha_blends_content_over_parent() {
    let mut buf = LcdBuffer::new(10, 10);
    {
        let mut ctx = LcdGfxCtx::new(&mut buf);
        ctx.clear(Color::white());
        assert!(ctx.supports_compositing_layers());
        ctx.push_layer_with_alpha(10.0, 10.0, 0.5);
        ctx.set_fill_color(Color::black());
        ctx.begin_path();
        ctx.rect(0.0, 0.0, 10.0, 10.0);
        ctx.fill();
        ctx.pop_layer();
    }
    // Half black over white, opaque.
    assert_eq!(px(&buf, 5, 5), ([128; 3], [255; 3]));
}

#[test]
fn rust_only_lcd_rounded_layer_clip_follows_agg_sharp_coverage() {
    let mut buf = LcdBuffer::new(40, 40);
    {
        let mut ctx = LcdGfxCtx::new(&mut buf);
        ctx.clear(Color::white());
        ctx.translate(5.0, 5.0);
        ctx.push_layer(30.0, 30.0);
        ctx.set_fill_color(Color::black());
        ctx.begin_path();
        ctx.rect(-10.0, -10.0, 50.0, 50.0);
        ctx.fill();
        ctx.set_layer_rounded_clip(0.0, 0.0, 30.0, 30.0, 10.0);
        ctx.pop_layer();
    }
    // The corner pixel of the clip keeps the parent.
    assert_eq!(px(&buf, 5, 5), ([255; 3], [255; 3]));
    // The middle and a straight edge take the layer.
    assert_eq!(px(&buf, 20, 20), ([0; 3], [255; 3]));
    // (Two columns in: the LCD filter leaves the outermost column of any
    // fill that reaches a buffer's edge with subpixel fringe.)
    assert_eq!(px(&buf, 7, 20), ([0; 3], [255; 3]));
    // Pixel (7, 7) has its centre at layer (2.5, 2.5): the arc centre is
    // (10, 10), so the signed distance is 7.5√2 − 10 ≈ 0.6066 and the
    // coverage clamp(0.5 − 0.6066) = 0: still the parent.
    assert_eq!(px(&buf, 7, 7), ([255; 3], [255; 3]));
    // Pixel (8, 7): centre (3.5, 2.5), distance √(6.5² + 7.5²) − 10 ≈
    // −0.0754, coverage ≈ 0.5754, so 255 × (1 − 0.5754) ≈ 108.
    let (c, a) = px(&buf, 8, 7);
    assert!(
        c.iter().all(|v| (*v as i32 - 108).abs() <= 1),
        "ramp pixel {c:?}"
    );
    assert_eq!(a, [255; 3]);
    // Outside the layer the parent is untouched.
    assert_eq!(px(&buf, 2, 20), ([255; 3], [255; 3]));
}

#[test]
fn rust_only_lcd_layer_keeps_the_device_scale() {
    let mut buf = LcdBuffer::new(40, 40);
    {
        let mut ctx = LcdGfxCtx::new(&mut buf);
        ctx.clear(Color::white());
        ctx.scale(2.0, 2.0);
        ctx.translate(5.0, 5.0);
        ctx.push_layer(10.0, 10.0);
        ctx.set_fill_color(Color::black());
        ctx.begin_path();
        ctx.rect(0.0, 0.0, 10.0, 10.0);
        ctx.fill();
        ctx.pop_layer();
    }
    // Logical (5..15) at scale 2 is physical (10..30).
    assert_eq!(px(&buf, 12, 12).0, [0; 3]);
    assert_eq!(px(&buf, 27, 27).0, [0; 3]);
    assert_eq!(px(&buf, 32, 20).0, [255; 3]);
    assert_eq!(px(&buf, 8, 20).0, [255; 3]);
}
