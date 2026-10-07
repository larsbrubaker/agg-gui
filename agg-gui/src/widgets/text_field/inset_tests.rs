//! Per-side text insets (`with_text_insets`) and the `min_size` floor in
//! `TextField::layout`.
//!
//! Both are additive: without them a field lays out, paints and maps clicks
//! exactly as with the uniform `padding`.  Driven through the production
//! `layout` / `on_event` / `caret_x` paths, no logic copies.

use std::sync::Arc;

use super::*;
use crate::event::{Event, Modifiers, MouseButton};
use crate::font_settings::LineBox;
use crate::geometry::Point;
use crate::layout_props::Insets;
use crate::widget::Widget;

const FONT_BYTES: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
}

const ROOM: Size = Size {
    width: 400.0,
    height: 400.0,
};

fn press(f: &mut TextField, x: f64) {
    for ev in [
        Event::MouseDown {
            pos: Point::new(x, 10.0),
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        },
        Event::MouseUp {
            pos: Point::new(x, 10.0),
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        },
    ] {
        f.on_event(&ev);
    }
}

#[test]
fn default_insets_follow_padding() {
    let f = TextField::new(font()).with_padding(5.0);
    let i = f.text_insets();
    assert_eq!((i.left, i.right, i.top, i.bottom), (5.0, 5.0, 5.0, 5.0));
    assert_eq!(f.text_center_y(30.0), 15.0);

    let mut em = TextField::new(font())
        .with_font_size(12.0)
        .with_padding(3.0)
        .with_line_box(LineBox::Em);
    assert_eq!(em.layout(ROOM), Size::new(400.0, 18.0));

    let mut standard = TextField::new(font())
        .with_font_size(12.0)
        .with_line_box(LineBox::Standard);
    assert_eq!(
        standard.layout(ROOM),
        Size::new(400.0, (12.0_f64 * 2.4).max(28.0))
    );

    // Caret at offset 0 sits at the left padding.
    let mut f = TextField::new(font()).with_padding(5.0);
    f.layout(ROOM);
    f.on_event(&Event::FocusGained);
    assert_eq!(f.caret_x(), 5.0);
}

#[test]
fn leading_inset_moves_text_without_growing_the_field() {
    let mut f = TextField::new(font())
        .with_font_size(12.0)
        .with_padding(3.0)
        .with_line_box(LineBox::Em)
        .with_text_insets(Insets {
            left: 40.0,
            right: 3.0,
            top: 3.0,
            bottom: 3.0,
        })
        .with_text("abcd");
    // Same height as a plain 3 px padded field.
    assert_eq!(f.layout(ROOM).height, 18.0);
    assert_eq!(f.text_center_y(18.0), 9.0);

    f.on_event(&Event::FocusGained);
    // Clicking just past the second glyph from the leading inset puts the
    // caret after "ab"; the caret draws from the inset too.
    let adv = measure_advance(&f.active_font(), "ab", 12.0);
    press(&mut f, 40.0 + adv);
    assert_eq!(f.cursor_pos(), 2);
    assert!((f.caret_x() - (40.0 + adv)).abs() < 1e-9);
}

#[test]
fn asymmetric_vertical_insets_set_height_and_centre() {
    let mut f = TextField::new(font())
        .with_font_size(10.0)
        .with_line_box(LineBox::Em)
        .with_text_insets(Insets {
            left: 2.0,
            right: 2.0,
            top: 6.0,
            bottom: 2.0,
        });
    assert_eq!(f.layout(ROOM).height, 18.0);
    // Y-up: the text line centres within [bottom, h - top] = [2, 12].
    assert_eq!(f.text_center_y(18.0), 7.0);
}

#[test]
fn horizontal_insets_bound_the_scroll_window() {
    let text = "a long run of text that overflows the field";
    let mut f = TextField::new(font())
        .with_font_size(12.0)
        .with_text_insets(Insets {
            left: 30.0,
            right: 10.0,
            top: 4.0,
            bottom: 4.0,
        })
        .with_text(text);
    let size = f.layout(Size::new(120.0, 40.0));
    f.set_bounds(crate::geometry::Rect::new(
        0.0,
        0.0,
        size.width,
        size.height,
    ));
    f.ensure_cursor_visible();
    // The caret (at the end) lands on the right edge of the text window:
    // width minus the right inset.
    assert!((f.caret_x() - (120.0 - 10.0)).abs() < 1e-9);
}

#[test]
fn layout_honours_min_size() {
    // Height floor above the natural height (a themed field design height).
    let mut tall = TextField::new(font())
        .with_font_size(12.0)
        .with_padding(3.0)
        .with_line_box(LineBox::Em)
        .with_min_size(Size::new(0.0, 25.0));
    assert_eq!(tall.layout(ROOM), Size::new(400.0, 25.0));

    // A floor below the natural size changes nothing.
    let mut loose = TextField::new(font())
        .with_font_size(12.0)
        .with_line_box(LineBox::Standard)
        .with_min_size(Size::new(10.0, 10.0));
    assert_eq!(
        loose.layout(ROOM),
        Size::new(400.0, (12.0_f64 * 2.4).max(28.0))
    );

    // Width floor wider than the available width.
    let mut wide = TextField::new(font())
        .with_font_size(12.0)
        .with_line_box(LineBox::Standard)
        .with_min_size(Size::new(500.0, 0.0));
    assert_eq!(wide.layout(ROOM).width, 500.0);
}

#[test]
fn set_text_insets_after_build_relayouts_and_drops_the_cache_signature() {
    let mut f = TextField::new(font())
        .with_font_size(12.0)
        .with_padding(3.0)
        .with_line_box(LineBox::Em)
        .with_text("abcd");
    assert_eq!(f.layout(ROOM).height, 18.0);
    let before = f.last_sig.clone();
    assert!(before.is_some());

    f.set_text_insets(Insets {
        left: 40.0,
        right: 3.0,
        top: 6.0,
        bottom: 4.0,
    });
    assert_eq!(f.text_insets().left, 40.0);
    // Height follows the new vertical insets on the next layout, and the
    // backbuffer signature changes so the stale bitmap is not blitted.
    assert_eq!(f.layout(ROOM).height, 22.0);
    assert!(f.last_sig != before);

    // The caret now starts at the new leading inset.
    f.on_event(&Event::FocusGained);
    press(&mut f, 0.0);
    assert_eq!(f.cursor_pos(), 0);
    assert_eq!(f.caret_x(), 40.0);
}

#[test]
fn set_text_insets_matching_the_builder_lays_out_identically() {
    let insets = Insets {
        left: 10.0,
        right: 2.0,
        top: 1.0,
        bottom: 5.0,
    };
    let mut built = TextField::new(font())
        .with_font_size(12.0)
        .with_line_box(LineBox::Em)
        .with_text_insets(insets);
    let mut set = TextField::new(font())
        .with_font_size(12.0)
        .with_line_box(LineBox::Em);
    set.set_text_insets(insets);
    assert_eq!(built.layout(ROOM), set.layout(ROOM));
    assert_eq!(built.text_center_y(18.0), set.text_center_y(18.0));
}
