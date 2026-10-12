//! Port of agg-sharp `Agg.GoldenImages/TextWidgetInsetTests.cs`: a label
//! placed by its parent's padding or its own margin draws its text at that
//! inset, on the software `GfxCtx` and on the GPU (where the label's
//! backbuffer composites as a texture).
//!
//! C#'s `TextWidget` is agg-gui's `Label`, its `FlowLayoutWidget` a
//! `FlexColumn` and its stretched `GuiWidget` a `Container`; the trees paint
//! through `harness::render_tree_both`.

use std::sync::Arc;

use agg_gui::color::Color;
use agg_gui::text::Font;
use agg_gui::widget::Widget;
use agg_gui::widgets::{Container, FlexColumn, Label};
use agg_gui::{HAnchor, Insets, Rect, Size, VAnchor};

use super::harness::{at, render_tree_both};

const FRAME_W: u32 = 160;
const FRAME_H: u32 = 64;

fn font() -> Arc<Font> {
    let bytes: &[u8] = include_bytes!("../../assets/fonts/NotoSans-Regular.ttf");
    Arc::new(Font::from_slice(bytes).expect("parse the test font"))
}

fn laid_out(mut root: Box<dyn Widget>) -> Box<dyn Widget> {
    root.layout(Size::new(FRAME_W as f64, FRAME_H as f64));
    root.set_bounds(Rect::new(0.0, 0.0, FRAME_W as f64, FRAME_H as f64));
    root
}

fn margin_tree() -> Box<dyn Widget> {
    let label = Label::new("GUI Demo", font())
        .with_font_size(10.0)
        .with_color(Color::white())
        .with_h_anchor(HAnchor::LEFT)
        .with_v_anchor(VAnchor::CENTER)
        .with_margin(Insets {
            left: 20.0,
            ..Insets::ZERO
        });
    laid_out(Box::new(
        Container::new()
            .with_h_anchor(HAnchor::STRETCH)
            .with_v_anchor(VAnchor::STRETCH)
            .add(Box::new(label)),
    ))
}

fn padding_tree() -> Box<dyn Widget> {
    let label = Label::new("Demos", font())
        .with_font_size(11.0)
        .with_color(Color::white())
        .with_strong(true)
        .with_h_anchor(HAnchor::LEFT);
    laid_out(Box::new(
        FlexColumn::new()
            .with_inner_padding(Insets {
                left: 20.0,
                bottom: 8.0,
                right: 8.0,
                top: 8.0,
            })
            .add(Box::new(label)),
    ))
}

fn leftmost_inked_column(image: &[u8]) -> Option<u32> {
    (0..FRAME_W).find(|&x| (0..FRAME_H).any(|y| at(image, FRAME_W, x, y)[0] > 64))
}

#[test]
fn text_starts_at_the_inset() {
    for padding in [false, true] {
        let build: &dyn Fn() -> Box<dyn Widget> =
            if padding { &padding_tree } else { &margin_tree };
        let Some(pair) = render_tree_both(FRAME_W, FRAME_H, Color::black(), build) else {
            return;
        };
        for (gpu, image) in [(false, &pair.software), (true, &pair.gpu)] {
            let left = leftmost_inked_column(image);
            // Glyph side bearings put the first ink a pixel or two right of
            // the inset, never left of it.
            assert!(
                left.is_some_and(|l| (20..=23).contains(&l)),
                "gpu {gpu}, padding {padding}: the text should start at the 20px inset, first ink at x {left:?}"
            );
        }
    }
}
