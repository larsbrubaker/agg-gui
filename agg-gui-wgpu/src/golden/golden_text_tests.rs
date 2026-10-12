//! Port of agg-sharp `Agg.GoldenImages/GoldenTextTests.cs` with its scenes
//! (`GoldenTextScenes.cs`): the text suite rendered on wgpu, grayscale and
//! LCD. The LCD cases matter most — WebGPU's colour write masks are
//! pipeline state, so the three-channel composite is three pipelines, and
//! these goldens say whether each is right.
//!
//! C# compares with PNGs from its own renderer; here (see `harness.rs`) the
//! reference is agg-gui's software `GfxCtx` drawing the same scene with the
//! same LCD setting. C#'s `TypeFacePrinter.SnapBaselinesToWholePixels = true`
//! is agg-gui's (thread-local) hinting toggle, turned on for each case.

use std::sync::Arc;

use agg_gui::color::Color;
use agg_gui::draw_ctx::DrawCtx;
use agg_gui::font_settings::set_hinting_enabled;
use agg_gui::text::Font;

use super::harness::render_both_with;

/// `WebGpuOffscreenCapture`'s default size.
const W: u32 = 512;
const H: u32 = 384;

/// The string every ladder draws: ascenders, descenders, digits, a ligature
/// pair and punctuation, which is where hinting and coverage differences
/// show first.
const SAMPLE: &str = "Handgloves 0123 fi.,;";

const SIZES: [f64; 11] = [
    7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 14.0, 18.0, 24.0, 32.0, 40.0,
];

fn font() -> Arc<Font> {
    let bytes: &[u8] = include_bytes!("../../assets/fonts/NotoSans-Regular.ttf");
    Arc::new(Font::from_slice(bytes).expect("parse the test font"))
}

/// C#'s `DarkBackground`, `ColorF(0.09, 0.11, 0.16, 1)`.
fn dark_background() -> Color {
    Color::rgb(0.09, 0.11, 0.16)
}

fn rgb8(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgba8(r, g, b, 255)
}

/// C#'s `DrawString(text, x, y, pointSize, color: color)`.
fn draw_string(
    ctx: &mut dyn DrawCtx,
    font: &Arc<Font>,
    text: &str,
    x: f64,
    y: f64,
    size: f64,
    color: Color,
) {
    ctx.set_font(Arc::clone(font));
    ctx.set_font_size(size);
    ctx.set_fill_color(color);
    ctx.fill_text(text, x, y);
}

fn fill_rect(ctx: &mut dyn DrawCtx, left: f64, bottom: f64, right: f64, top: f64, color: Color) {
    ctx.set_fill_color(color);
    ctx.begin_path();
    ctx.rect(left, bottom, right - left, top - bottom);
    ctx.fill();
}

/// The sample once per size, bottom to top.
fn draw_size_ladder(ctx: &mut dyn DrawCtx, color: Color) {
    let font = font();
    let mut y = 8.0;
    for size in SIZES {
        draw_string(
            ctx,
            &font,
            &format!("{size:.0} {SAMPLE}"),
            8.0,
            y,
            size,
            color,
        );
        y += size + 6.0;
    }
}

/// The same short string at every quarter-pixel x and y offset.
fn draw_sub_pixel_grid(ctx: &mut dyn DrawCtx, color: Color, point_size: f64) {
    let font = font();
    for row in 0..4 {
        for column in 0..4 {
            let (r, c) = (row as f64, column as f64);
            let x = 12.0 + (c * 124.0) + (c * 0.25);
            let y = 40.0 + (r * 80.0) + (r * 0.25);
            draw_string(ctx, &font, "Wave iIl1", x, y, point_size, color);
        }
    }
}

fn size_ladder_dark_on_light(ctx: &mut dyn DrawCtx) {
    draw_size_ladder(ctx, Color::black());
}

fn size_ladder_light_on_dark(ctx: &mut dyn DrawCtx) {
    draw_size_ladder(ctx, rgb8(238, 238, 230));
}

fn sub_pixel_placement(ctx: &mut dyn DrawCtx) {
    draw_sub_pixel_grid(ctx, Color::black(), 12.0);
    draw_sub_pixel_grid(ctx, rgb8(200, 40, 40), 9.0);
}

/// Text over shapes rather than the cleared background, so the composite
/// blends against colours.
fn over_filled_shapes(ctx: &mut dyn DrawCtx) {
    fill_rect(ctx, 0.0, 0.0, 512.0, 128.0, rgb8(220, 60, 40));
    fill_rect(ctx, 0.0, 128.0, 512.0, 256.0, rgb8(40, 110, 200));
    fill_rect(ctx, 0.0, 256.0, 512.0, 384.0, rgb8(245, 245, 245));
    let font = font();
    draw_string(ctx, &font, SAMPLE, 10.0, 40.0, 18.0, Color::white());
    draw_string(ctx, &font, SAMPLE, 10.0, 80.0, 12.0, Color::black());
    draw_string(ctx, &font, SAMPLE, 10.0, 168.0, 18.0, Color::white());
    draw_string(ctx, &font, SAMPLE, 10.0, 208.0, 12.0, rgb8(255, 240, 120));
    draw_string(ctx, &font, SAMPLE, 10.0, 296.0, 18.0, Color::black());
    draw_string(ctx, &font, SAMPLE, 10.0, 336.0, 12.0, rgb8(60, 60, 60));
}

/// Both paths in one frame: full-colour geometry and text after the LCD
/// passes show any channel mask that leaked.
fn lcd_then_ordinary(ctx: &mut dyn DrawCtx) {
    let font = font();
    draw_string(ctx, &font, SAMPLE, 10.0, 340.0, 20.0, Color::black());
    fill_rect(ctx, 20.0, 250.0, 240.0, 320.0, rgb8(220, 60, 40));
    fill_rect(ctx, 270.0, 250.0, 490.0, 320.0, rgb8(40, 110, 200));
    // C#'s `new Ellipse(256, 150, 90, 60)`.
    ctx.set_fill_color(rgb8(30, 160, 90));
    ctx.begin_path();
    ctx.ellipse(
        256.0,
        150.0,
        90.0,
        60.0,
        0.0,
        0.0,
        2.0 * std::f64::consts::PI,
        false,
    );
    ctx.fill();
    draw_string(ctx, &font, SAMPLE, 10.0, 40.0, 20.0, rgb8(20, 20, 20));
}

fn check(name: &str, lcd: bool, background: Color, draw: fn(&mut dyn DrawCtx)) {
    set_hinting_enabled(true);
    let Some(pair) = render_both_with(W, H, background, lcd, &|ctx| draw(ctx)) else {
        return;
    };
    pair.assert_matches(name, 2);
}

#[test]
fn anti_aliased_size_ladder() {
    check(
        "Text.Aa.SizeLadder",
        false,
        Color::white(),
        size_ladder_dark_on_light,
    );
}

#[test]
fn anti_aliased_light_on_dark() {
    check(
        "Text.Aa.LightOnDark",
        false,
        dark_background(),
        size_ladder_light_on_dark,
    );
}

#[test]
fn anti_aliased_sub_pixel_placement() {
    check(
        "Text.Aa.SubPixel",
        false,
        Color::white(),
        sub_pixel_placement,
    );
}

#[test]
fn lcd_size_ladder() {
    check(
        "Text.Lcd.SizeLadder",
        true,
        Color::white(),
        size_ladder_dark_on_light,
    );
}

#[test]
fn lcd_light_on_dark() {
    check(
        "Text.Lcd.LightOnDark",
        true,
        dark_background(),
        size_ladder_light_on_dark,
    );
}

#[test]
fn lcd_sub_pixel_placement() {
    check(
        "Text.Lcd.SubPixel",
        true,
        Color::white(),
        sub_pixel_placement,
    );
}

#[test]
fn lcd_over_filled_shapes() {
    check(
        "Text.Lcd.OverShapes",
        true,
        Color::white(),
        over_filled_shapes,
    );
}

#[test]
fn lcd_followed_by_ordinary_drawing() {
    check(
        "Text.Lcd.ThenOrdinary",
        true,
        Color::white(),
        lcd_then_ordinary,
    );
}
