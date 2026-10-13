//! Port of agg-sharp's `SvgIconLcdTests` (`Tests/Agg.Tests/Agg.UI/SvgIconLcdTests.cs`):
//! SVG icons ([`IconImage`]) draw with LCD subpixel coverage, as text does,
//! when they land 1:1 on whole pixels of a target that takes LCD — and exactly
//! as the plain blit everywhere else.
//!
//! C#'s `ImageBuffer` edits (`InvertLightness`, `AjustAlpha`, `WhiteToAlpha`,
//! `MirrorX`, drawing a badge into the image) are agg-gui's
//! [`IconImage::with_pixel_filter`]: the filters below port those edits over
//! the straight-alpha, top-row-first raster.  C#'s process-wide
//! `LcdRenderSettings.Enabled` is the context's own LCD switch
//! ([`GfxCtx::set_lcd_mode`]), the one LCD text takes.  `Render(icon, x, y)`
//! is [`IconImage::draw`]; `WithoutCoverage(icon)` is the same raster drawn
//! through `draw_image_rgba_arc`, which is what the blit draws without the LCD
//! path.  Pixel coordinates are Y-up in both, so the C# positions carry over.

use std::sync::Arc;

use crate::geometry::Size;
use crate::lcd_coverage::LcdBuffer;
use crate::lcd_gfx_ctx::LcdGfxCtx;
use crate::{Color, DrawCtx, Framebuffer, GfxCtx, IconImage};

const ICON_SIZE: u32 = 32;
const CANVAS_SIZE: u32 = 48;

/// The largest per-pixel luminance difference the LCD composite may have from
/// the plain blit (see the C# constant's reasoning).
const LUMINANCE_TOLERANCE: i32 = 96;

/// An L whose foot slants, in the top left of the viewBox: no axis of symmetry.
const CORNER: &str = r##"<path d="M8 8 H44 V52 H70 L86 70 H8 Z" fill="#000"/>"##;

type Rgba = [u8; 4];
const WHITE: Rgba = [255, 255, 255, 255];
const BLACK: Rgba = [0, 0, 0, 255];
const RED: Rgba = [255, 0, 0, 255];
const TRANSPARENT: Rgba = [0, 0, 0, 0];

fn svg(body: &str) -> String {
    format!(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">{body}</svg>"#)
}

/// C# `LoadIcon(path, 32, 32, invert)`: the SVG at 32×32, lightness-inverted
/// when `invert` (MatterCAD's dark theme).
fn load_svg_icon(body: &str, invert: bool) -> IconImage {
    let icon = IconImage::from_svg_data(
        svg(body).as_bytes(),
        Size::new(ICON_SIZE as f64, ICON_SIZE as f64),
    )
    .expect("icon svg");
    if invert {
        icon.with_pixel_filter(Arc::new(invert_lightness))
    } else {
        icon
    }
}

/// C# `InvertLightness`: each pixel's HSL lightness becomes `1 - l`, alpha kept.
fn invert_lightness(px: &mut [u8]) {
    for p in px.chunks_exact_mut(4) {
        let c = Color::rgba(
            p[0] as f32 / 255.0,
            p[1] as f32 / 255.0,
            p[2] as f32 / 255.0,
            1.0,
        );
        let (h, s, l) = c.to_hsl();
        let inv = Color::from_hsl(h, s, 1.0 - l, 1.0);
        p[0] = (inv.r * 255.0).round() as u8;
        p[1] = (inv.g * 255.0).round() as u8;
        p[2] = (inv.b * 255.0).round() as u8;
    }
}

/// C# `AjustAlpha(factor)`: alpha scaled (truncating), colour cleared where it
/// ends up fully transparent.
fn adjust_alpha(factor: f64) -> impl Fn(&mut [u8]) + Send + Sync {
    move |px: &mut [u8]| {
        for p in px.chunks_exact_mut(4) {
            if p[3] > 0 {
                p[3] = (p[3] as f64 * factor) as u8;
                if p[3] == 0 {
                    p[0] = 0;
                    p[1] = 0;
                    p[2] = 0;
                }
            }
        }
    }
}

/// C# `WhiteToAlpha`: transparent and near-white mostly-opaque pixels cleared.
fn white_to_alpha(px: &mut [u8]) {
    for p in px.chunks_exact_mut(4) {
        if p[3] == 0 || (p[0] > 240 && p[1] > 240 && p[2] > 240 && p[3] > 200) {
            p.copy_from_slice(&[0, 0, 0, 0]);
        }
    }
}

/// C# `MirrorX` / `MirrorY` of a square `ICON_SIZE` raster.
fn mirror(horizontal: bool) -> impl Fn(&mut [u8]) + Send + Sync {
    move |px: &mut [u8]| {
        let n = ICON_SIZE as usize;
        let src = px.to_vec();
        for y in 0..n {
            for x in 0..n {
                let (sx, sy) = if horizontal {
                    (n - 1 - x, y)
                } else {
                    (x, n - 1 - y)
                };
                let (d, s) = ((y * n + x) * 4, (sy * n + sx) * 4);
                px[d..d + 4].copy_from_slice(&src[s..s + 4]);
            }
        }
    }
}

/// C# `FillRectangle(24, 0, 32, 8, Color.Red)` into the icon (Y-up): the
/// bottom-right 8×8, which in the top-row-first raster is rows 24..32.
fn red_badge(px: &mut [u8]) {
    let n = ICON_SIZE as usize;
    for y in 24..32 {
        for x in 24..32 {
            let d = (y * n + x) * 4;
            px[d..d + 4].copy_from_slice(&RED);
        }
    }
}

fn opaque(color: Color) -> Framebuffer {
    let mut fb = Framebuffer::new(CANVAS_SIZE, CANVAS_SIZE);
    GfxCtx::new(&mut fb).clear(color);
    fb
}

/// `Render(icon, x, y)` onto `fb` with LCD on or off.
fn render(fb: &mut Framebuffer, icon: &IconImage, x: f64, y: f64, lcd: bool) {
    let mut ctx = GfxCtx::new(fb);
    ctx.set_lcd_mode(lcd);
    icon.draw(&mut ctx, x, y);
}

/// `Render(WithoutCoverage(icon), x, y)`: the same raster, plain blit.
fn render_plain(ctx: &mut dyn DrawCtx, icon: &IconImage, x: f64, y: f64) {
    let (data, w, h) = icon.raster_at_scale(1.0).expect("raster");
    let size = icon.size();
    ctx.draw_image_rgba_arc(&data, w, h, x, y, size.width, size.height);
}

fn render_plain_onto(fb: &mut Framebuffer, icon: &IconImage, x: f64, y: f64) {
    let mut ctx = GfxCtx::new(fb);
    ctx.set_lcd_mode(true);
    render_plain(&mut ctx, icon, x, y);
}

fn pixel(fb: &Framebuffer, x: u32, y: u32) -> Rgba {
    let i = ((y * fb.width() + x) * 4) as usize;
    let p = fb.pixels();
    [p[i], p[i + 1], p[i + 2], p[i + 3]]
}

/// C# `Luminance`: banker's rounding as `Math.Round`.
fn luminance(c: Rgba) -> i32 {
    (0.2126 * c[0] as f64 + 0.7152 * c[1] as f64 + 0.0722 * c[2] as f64).round_ties_even() as i32
}

fn count_chroma_pixels(fb: &Framebuffer) -> usize {
    (0..fb.height())
        .flat_map(|y| (0..fb.width()).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            let p = pixel(fb, x, y);
            p[0] != p[2]
        })
        .count()
}

/// Half-open `(left, bottom, right, top)` of every pixel visibly different
/// from `background`; all zero when none is.
fn painted_bounds(fb: &Framebuffer, background: Rgba) -> (i32, i32, i32, i32) {
    let (mut l, mut b, mut r, mut t) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    for y in 0..fb.height() as i32 {
        for x in 0..fb.width() as i32 {
            let p = pixel(fb, x as u32, y as u32);
            if (luminance(p) - luminance(background)).abs() > 32
                || (p[3] as i32 - background[3] as i32).abs() > 32
            {
                l = l.min(x);
                b = b.min(y);
                r = r.max(x + 1);
                t = t.max(y + 1);
            }
        }
    }
    if l == i32::MAX {
        (0, 0, 0, 0)
    } else {
        (l, b, r, t)
    }
}

fn width_of(bounds: (i32, i32, i32, i32)) -> i32 {
    bounds.2 - bounds.0
}

fn assert_painted_and_identical(actual: &Framebuffer, expected: &Framebuffer, background: Rgba) {
    assert!(
        actual.pixels() == expected.pixels(),
        "the draw must be byte for byte the plain blit"
    );
    assert!(
        width_of(painted_bounds(actual, background)) > 0,
        "the icon has to have painted, or the comparison proves nothing"
    );
}

/// Same painted bounding box, allowing the LCD filter's one pixel of
/// horizontal spread, and every pixel's luminance within
/// [`LUMINANCE_TOLERANCE`] of the plain blit's.
fn assert_same_footprint(actual: &Framebuffer, expected: &Framebuffer, background: Rgba) {
    let a = painted_bounds(actual, background);
    let e = painted_bounds(expected, background);
    assert!(width_of(e) > 0);
    assert!((a.0 - e.0).abs() <= 1, "left {a:?} vs {e:?}");
    assert!((a.2 - e.2).abs() <= 1, "right {a:?} vs {e:?}");
    assert_eq!(a.1, e.1, "bottom");
    assert_eq!(a.3, e.3, "top");
    let mut worst = 0;
    for y in 0..actual.height() {
        for x in 0..actual.width() {
            let d = (luminance(pixel(actual, x, y)) - luminance(pixel(expected, x, y))).abs();
            worst = worst.max(d);
        }
    }
    assert!(
        worst <= LUMINANCE_TOLERANCE,
        "worst luminance difference {worst}"
    );
}

#[test]
fn black_icon_on_white_shows_chroma_at_its_edges() {
    let icon = load_svg_icon(CORNER, false);
    let mut canvas = opaque(Color::white());
    render(&mut canvas, &icon, 8.0, 8.0, true);
    assert!(
        count_chroma_pixels(&canvas) > 0,
        "an icon drawn 1:1 on whole pixels takes the LCD path, so its edges carry R != B"
    );
    assert_eq!(
        pixel(&canvas, 8 + 4, 8 + 24),
        BLACK,
        "the L's fully covered upright is the icon's own colour"
    );
}

#[test]
fn inverted_icon_on_black_shows_chroma_and_a_white_interior() {
    let icon = load_svg_icon(CORNER, true);
    let mut canvas = opaque(Color::black());
    render(&mut canvas, &icon, 8.0, 8.0, true);
    assert!(
        count_chroma_pixels(&canvas) > 0,
        "the colour comes from the image at draw time, so a dark-theme inversion keeps the subpixel coverage"
    );
    assert_eq!(pixel(&canvas, 8 + 4, 8 + 24), WHITE);
}

/// A transparent target is refused: one alpha per pixel cannot carry three
/// channel coverages, so the icon takes the plain blit.
#[test]
fn a_straight_transparent_target_gets_the_plain_blit() {
    let icon = load_svg_icon(CORNER, true);
    let mut target = Framebuffer::new(ICON_SIZE, ICON_SIZE);
    render(&mut target, &icon, 0.0, 0.0, true);
    let mut expected = Framebuffer::new(ICON_SIZE, ICON_SIZE);
    render_plain_onto(&mut expected, &icon, 0.0, 0.0);
    assert_painted_and_identical(&target, &expected, TRANSPARENT);
}

/// A mirrored icon has no coverage that lines up with it, so it draws as a
/// plain blit — whether its pixels are mirrored (a pixel filter, C#'s
/// `MirrorX`/`MirrorY`) or the draw is (a mirroring transform).
#[test]
fn a_mirrored_icon_draws_as_a_plain_blit() {
    for horizontal in [true, false] {
        let mirrored = load_svg_icon(CORNER, false).with_pixel_filter(Arc::new(mirror(horizontal)));
        let mut canvas = opaque(Color::white());
        render(&mut canvas, &mirrored, 8.0, 8.0, true);
        let mut expected = opaque(Color::white());
        render_plain_onto(&mut expected, &mirrored, 8.0, 8.0);
        assert_painted_and_identical(&canvas, &expected, WHITE);
    }
    let icon = load_svg_icon(CORNER, false);
    for (sx, sy, tx, ty) in [(-1.0, 1.0, 48.0, 0.0), (1.0, -1.0, 0.0, 48.0)] {
        let draw = |fb: &mut Framebuffer, lcd_icon: bool| {
            let mut ctx = GfxCtx::new(fb);
            ctx.set_lcd_mode(true);
            ctx.translate(tx, ty);
            ctx.scale(sx, sy);
            if lcd_icon {
                icon.draw(&mut ctx, 8.0, 8.0);
            } else {
                render_plain(&mut ctx, &icon, 8.0, 8.0);
            }
        };
        let mut canvas = opaque(Color::white());
        draw(&mut canvas, true);
        let mut expected = opaque(Color::white());
        draw(&mut expected, false);
        assert_painted_and_identical(&canvas, &expected, WHITE);
    }
}

/// Ink drawn into the icon after load where its coverage says there is
/// nothing (a badge) must still show: such an icon falls back to the plain
/// blit rather than losing the badge.
#[test]
fn a_badge_drawn_outside_the_silhouette_still_shows() {
    let badged = load_svg_icon(CORNER, false).with_pixel_filter(Arc::new(red_badge));
    let mut canvas = opaque(Color::white());
    render(&mut canvas, &badged, 8.0, 8.0, true);
    assert_eq!(pixel(&canvas, 8 + 28, 8 + 4), RED);
}

/// A faded icon (a disabled button's) composites faded, matching the faded
/// plain blit.
#[test]
fn a_faded_icon_composites_faded() {
    let faded = load_svg_icon(CORNER, false).with_pixel_filter(Arc::new(adjust_alpha(0.2)));
    let mut lcd = opaque(Color::white());
    render(&mut lcd, &faded, 8.0, 8.0, true);
    assert!(
        count_chroma_pixels(&lcd) > 0,
        "the fade must keep the LCD composite for this to test it"
    );
    let mut expected = opaque(Color::white());
    render_plain_onto(&mut expected, &faded, 8.0, 8.0);

    let interior = pixel(&lcd, 8 + 4, 8 + 24);
    let expected_interior = pixel(&expected, 8 + 4, 8 + 24);
    assert!(
        (expected_interior[0] as i32) < 250,
        "the faded blit has to show the fade for this to compare anything"
    );
    for c in 0..3 {
        assert!(
            (interior[c] as i32 - expected_interior[c] as i32).abs() <= 3,
            "channel {c}: {interior:?} vs {expected_interior:?}"
        );
    }
    assert_same_footprint(&lcd, &expected, WHITE);
}

/// The clipping rect bounds the LCD composite, as it does the plain blit.
#[test]
fn the_clip_bounds_the_composite() {
    let icon = load_svg_icon(CORNER, false);
    let mut canvas = opaque(Color::white());
    {
        let mut ctx = GfxCtx::new(&mut canvas);
        ctx.set_lcd_mode(true);
        ctx.clip_rect(0.0, 0.0, 8.0 + 8.0, CANVAS_SIZE as f64);
        icon.draw(&mut ctx, 8.0, 8.0);
    }
    assert_eq!(
        pixel(&canvas, 8 + 4, 8 + 24),
        BLACK,
        "inside the clip the icon paints"
    );
    assert!(
        count_chroma_pixels(&canvas) > 0,
        "the composite has to have been the LCD one for this to test it"
    );
    let mut painted_outside_clip = 0;
    for y in 0..CANVAS_SIZE {
        for x in 8 + 8..CANVAS_SIZE {
            if pixel(&canvas, x, y) != WHITE {
                painted_outside_clip += 1;
            }
        }
    }
    assert_eq!(painted_outside_clip, 0);
}

/// WhiteToAlpha only lowers alpha, so the icon's coverage still describes its
/// pixels — the a_now / a_orig ratio treats what it cleared as faded out.
#[test]
fn white_to_alpha_keeps_the_subpixel_edges() {
    let cleared = load_svg_icon(CORNER, false).with_pixel_filter(Arc::new(white_to_alpha));
    let mut canvas = opaque(Color::white());
    render(&mut canvas, &cleared, 8.0, 8.0, true);
    assert!(count_chroma_pixels(&canvas) > 0);
}

/// Draw `icon` (LCD-aware when `lcd_icon`, else its plain blit) onto a fresh
/// opaque white canvas with LCD `lcd`, after `place` sets up the CTM, at
/// `(x, y)`.
fn draw_placed(
    icon: &IconImage,
    lcd: bool,
    lcd_icon: bool,
    place: &dyn Fn(&mut dyn DrawCtx),
    x: f64,
    y: f64,
) -> Framebuffer {
    let mut fb = opaque(Color::white());
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.set_lcd_mode(lcd);
        place(&mut ctx);
        if lcd_icon {
            icon.draw(&mut ctx, x, y);
        } else {
            render_plain(&mut ctx, icon, x, y);
        }
    }
    fb
}

/// The LCD composite lands where the plain blit does — same footprint, same
/// ink per pixel give or take the subpixel spread — whether the placement
/// comes from the position, the transform's translation or the image's
/// hotspot.  C#'s `OriginOffset` hotspot is subtracted from the placement,
/// so here it is a CTM translation by `-hotspot` after the graphics
/// transform's own translation.
#[test]
fn the_lcd_composite_lands_where_the_plain_blit_does() {
    for (x, y, translate_x, translate_y, hotspot_x, hotspot_y) in
        [(8, 8, 0, 0, 0, 0), (3, 4, 5, 3, 0, 0), (10, 11, 0, 0, 2, 3)]
    {
        let icon = load_svg_icon(CORNER, false);
        let place = |ctx: &mut dyn DrawCtx| {
            ctx.translate(translate_x as f64, translate_y as f64);
            ctx.translate(-hotspot_x as f64, -hotspot_y as f64);
        };
        let lcd = draw_placed(&icon, true, true, &place, x as f64, y as f64);
        let expected = draw_placed(&icon, true, false, &place, x as f64, y as f64);
        assert!(
            count_chroma_pixels(&lcd) > 0,
            "the composite has to have been the LCD one for this to test it ({x}, {y})"
        );
        assert_same_footprint(&lcd, &expected, WHITE);
    }
}

/// Anywhere the LCD composite does not apply — LCD off, a fractional
/// position, a scaled draw — the icon draws byte for byte as the same pixels
/// without coverage do.  C#'s `Render(icon, x, y, 0, scale, scale)` places
/// the image at `(x, y)` scaled about its origin: here a CTM translation to
/// `(x, y)` and a scale, with the icon drawn at the origin.
#[test]
fn without_the_lcd_path_the_icon_draws_as_a_plain_blit() {
    for (lcd_enabled, x, y, scale) in [
        (false, 8.0, 8.0, 1.0),
        (true, 8.5, 8.0, 1.0),
        (true, 8.0, 8.25, 1.0),
        (true, 8.0, 8.0, 1.25),
    ] {
        let icon = load_svg_icon(CORNER, false);
        let place = |ctx: &mut dyn DrawCtx| {
            ctx.translate(x, y);
            ctx.scale(scale, scale);
        };
        let with_coverage = draw_placed(&icon, lcd_enabled, true, &place, 0.0, 0.0);
        let expected = draw_placed(&icon, lcd_enabled, false, &place, 0.0, 0.0);
        assert_painted_and_identical(&with_coverage, &expected, WHITE);
    }
}

/// Inside an LCD widget backbuffer ([`LcdGfxCtx`]) the icon's coverage
/// merges per channel.  C#'s `LcdBuffer.ToImageBufferCollapsed` is
/// [`LcdBuffer::to_rgba8_top_down_collapsed`], flipped into a Y-up
/// framebuffer for the footprint check.
#[test]
fn an_lcd_buffer_destination_keeps_the_chroma() {
    let icon = load_svg_icon(CORNER, false);
    let mut buffer = LcdBuffer::new(CANVAS_SIZE, CANVAS_SIZE);
    {
        let mut graphics = LcdGfxCtx::new(&mut buffer);
        graphics.clear(Color::white());
        icon.draw(&mut graphics, 8.0, 8.0);
    }
    let chroma = buffer
        .color_plane()
        .chunks_exact(3)
        .filter(|p| p[0] != p[2])
        .count();
    assert!(chroma > 0);

    let mut expected = opaque(Color::white());
    render_plain_onto(&mut expected, &icon, 8.0, 8.0);
    let collapsed = buffer.to_rgba8_top_down_collapsed();
    let mut actual = Framebuffer::new(CANVAS_SIZE, CANVAS_SIZE);
    let row = CANVAS_SIZE as usize * 4;
    for (y, dst) in actual.pixels_mut().chunks_exact_mut(row).enumerate() {
        let src_y = CANVAS_SIZE as usize - 1 - y;
        dst.copy_from_slice(&collapsed[src_y * row..(src_y + 1) * row]);
    }
    assert_same_footprint(&actual, &expected, WHITE);
}

/// Port of `AnLcdIconInAStraightBackbufferCompositesLikeADirectDraw`
/// (`backbufferAlpha` 0 and 128): an LCD icon drawn into the transparent or
/// half-transparent pixels of a backbuffer, then composited onto an opaque
/// surface, matches the icon drawn straight onto that surface.  C#'s
/// straight-held `BlenderPreMultBGRA` widget backbuffer is a transparent
/// compositing layer here ([`GfxCtx::push_layer`]); the icon takes the plain
/// blit on its non-opaque pixels, because one alpha per pixel cannot carry
/// three channel coverages.  Tolerance 2 is two byte roundings (into the
/// layer, then onto the surface).
#[test]
fn an_lcd_icon_in_a_straight_backbuffer_composites_like_a_direct_draw() {
    const SURFACE: Color = Color {
        r: 128.0 / 255.0,
        g: 128.0 / 255.0,
        b: 128.0 / 255.0,
        a: 1.0,
    };
    for backbuffer_alpha in [0u8, 128] {
        let icon = load_svg_icon(CORNER, true);
        let under = Color::rgba(
            40.0 / 255.0,
            60.0 / 255.0,
            200.0 / 255.0,
            backbuffer_alpha as f32 / 255.0,
        );
        let n = CANVAS_SIZE as f64;
        let fill_under = |ctx: &mut GfxCtx| {
            ctx.set_fill_color(under);
            ctx.begin_path();
            ctx.rect(0.0, 0.0, n, n);
            ctx.fill();
        };

        let mut buffered = opaque(SURFACE);
        {
            let mut ctx = GfxCtx::new(&mut buffered);
            ctx.set_lcd_mode(true);
            ctx.push_layer(n, n);
            fill_under(&mut ctx);
            icon.draw(&mut ctx, 8.0, 8.0);
            ctx.pop_layer();
        }

        let mut direct = opaque(SURFACE);
        {
            let mut ctx = GfxCtx::new(&mut direct);
            ctx.set_lcd_mode(true);
            fill_under(&mut ctx);
            render_plain(&mut ctx, &icon, 8.0, 8.0);
        }

        let background = pixel(&direct, 0, 0);
        assert!(
            width_of(painted_bounds(&direct, background)) > 0,
            "the icon has to have painted, or the comparison proves nothing"
        );
        let worst = buffered
            .pixels()
            .iter()
            .zip(direct.pixels())
            .map(|(a, b)| (*a as i32 - *b as i32).abs())
            .max()
            .unwrap_or(0);
        assert!(
            worst <= 2,
            "backbuffer alpha {backbuffer_alpha}: worst channel difference {worst}"
        );
    }
}
