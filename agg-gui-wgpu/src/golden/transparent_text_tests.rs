//! Port of agg-sharp `Agg.GoldenImages/TransparentTextTests.cs`: text drawn
//! with a fully transparent colour draws nothing, on the GPU and in software
//! (C# once read alpha 0 as "no colour given" and drew black); text drawn
//! without setting a colour draws in the default black.

use std::sync::Arc;

use agg_gui::color::Color;
use agg_gui::draw_ctx::DrawCtx;
use agg_gui::text::Font;

use super::harness::{gpu, software};

const W: u32 = 200;
const H: u32 = 60;

fn font() -> Arc<Font> {
    let bytes: &[u8] = include_bytes!("../../assets/fonts/NotoSans-Regular.ttf");
    Arc::new(Font::from_slice(bytes).expect("parse the test font"))
}

/// C#'s `DrawString(text, 10, 20, 20, color)`: 20 pt at (10, 20).
fn draw_string(ctx: &mut dyn DrawCtx, text: &str, color: Option<Color>) {
    ctx.set_font(font());
    ctx.set_font_size(20.0);
    if let Some(color) = color {
        ctx.set_fill_color(color);
    }
    ctx.fill_text(text, 10.0, 20.0);
}

fn count_non_white(data: &[u8]) -> usize {
    data.chunks(4)
        .filter(|p| p[0] < 250 || p[1] < 250 || p[2] < 250)
        .count()
}

fn transparent_white() -> Option<Color> {
    Some(Color::from_rgba8(255, 255, 255, 0))
}

#[test]
fn transparent_text_draws_nothing_in_software() {
    let image = software(W, H, Color::white(), &|ctx| {
        draw_string(ctx, "Hidden text", transparent_white())
    });
    assert_eq!(count_non_white(&image), 0);
}

#[test]
fn omitted_color_still_draws_black() {
    let image = software(W, H, Color::white(), &|ctx| {
        draw_string(ctx, "Shown text", None)
    });
    assert!(count_non_white(&image) > 50);
}

#[test]
fn transparent_text_draws_nothing_on_the_gpu() {
    let Some(image) = gpu(W, H, Color::white(), &|ctx| {
        draw_string(ctx, "Hidden text", transparent_white())
    }) else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    assert_eq!(count_non_white(&image), 0);
}
