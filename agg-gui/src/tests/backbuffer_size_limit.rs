//! A CPU backbuffer larger than the destination can draw in one piece.
//!
//! A GPU destination draws a backbuffered widget's cached bitmap as one
//! texture, and a texture has a size limit (8192 px with wgpu's default
//! limits). A Label as wide as a long line of text went past it: the blit was
//! a wgpu validation error, which wgpu's default error handler turns into a
//! panic. While its bitmap would be too large, the widget now paints directly.
//! The GPU half of this is `agg-gui-wgpu`'s `oversized_backbuffer_tests`.

use std::sync::Arc;

use crate::draw_ctx::DrawCtx;
use crate::geometry::{Rect, Size};
use crate::text::Font;
use crate::widget::{paint_subtree, Widget};
use crate::widgets::Label;

use super::paint_recorder::PaintRecorder;

const LIMIT: u32 = 1000;

/// A Label laid out `width` logical px wide.
fn label(width: f64) -> Label {
    let font = Arc::new(Font::from_slice(super::TEST_FONT).expect("font"));
    let mut label = Label::new("A line of text", font);
    label.layout(Size::new(width, 40.0));
    label.set_bounds(Rect::new(0.0, 0.0, width, 20.0));
    label
}

/// One paint of a `width`-wide Label onto a destination whose images are
/// limited to `LIMIT` px.
fn paint(width: f64) -> PaintRecorder {
    let mut rec = PaintRecorder::new();
    rec.max_image_dimension = Some(LIMIT);
    paint_subtree(&mut label(width), &mut rec);
    rec
}

#[test]
fn a_backbuffer_at_the_limit_is_blitted() {
    let rec = paint(LIMIT as f64);
    assert_eq!(rec.image_clips.len(), 1, "one cached bitmap");
    assert!(rec.texts.is_empty(), "the text went into the bitmap");
}

#[test]
fn a_backbuffer_past_the_limit_paints_directly() {
    let rec = paint(LIMIT as f64 + 1.0);
    assert!(rec.image_clips.is_empty(), "no bitmap past the limit");
    assert_eq!(
        rec.texts
            .iter()
            .map(|(t, _, _)| t.as_str())
            .collect::<Vec<_>>(),
        ["A line of text"],
        "the Label paints its text directly"
    );
}

/// The device scale counts: the limit is in physical pixels.
#[test]
fn the_limit_is_in_physical_pixels() {
    let mut rec = PaintRecorder::new();
    rec.max_image_dimension = Some(LIMIT);
    rec.scale(2.0, 2.0);
    paint_subtree(&mut label(LIMIT as f64 / 2.0 + 1.0), &mut rec);
    assert!(
        rec.image_clips.is_empty(),
        "1002 physical px is past the limit"
    );
}

/// With no limit (the software `GfxCtx`, the default) nothing changes.
#[test]
fn a_destination_with_no_limit_always_gets_the_bitmap() {
    let mut rec = PaintRecorder::new();
    paint_subtree(&mut label(20_000.0), &mut rec);
    assert_eq!(rec.image_clips.len(), 1);
}
