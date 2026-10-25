//! Tests for [`TextArea::with_muted_range`] (`muted.rs`): the muted bytes
//! paint in the hint colour and the rest in the text colour, the range splits
//! across wrapped lines and over a highlighter's runs, a stale range is
//! clamped, and moving the range alone changes the backbuffer signature (text
//! that turns final keeps its bytes, so only the colour changes). The pixel
//! test paints the production widget through the theme tests' software
//! `GfxCtx` helpers.

use std::cell::Cell;
use std::rc::Rc;

use super::theme_tests::{area, colors, render, rgb, rgb_of, State};
use super::*;
use crate::color::Color;
use crate::framebuffer::Framebuffer;

const RED: Color = Color::rgb(1.0, 0.0, 0.0);
const GREY: Color = Color::rgb(0.5, 0.5, 0.5);

fn line(start: usize, text: &str) -> WrappedLine {
    WrappedLine {
        start,
        end: start + text.len(),
        text: text.to_string(),
        hard_break: false,
    }
}

#[test]
fn a_line_without_a_muted_range_or_highlighter_is_plain() {
    let ta = area("abc", "", colors());
    assert!(ta.line_spans(&line(0, "abc"), GREY).is_none());
}

#[test]
fn the_muted_range_is_cut_to_each_wrapped_line() {
    let range = Rc::new(Cell::new(Some((4, 12))));
    let ta = area("abc defgh ijk", "", colors()).with_muted_range(Rc::clone(&range));
    // "abc defgh " then "ijk": the range covers "defgh " and "ij".
    assert_eq!(
        ta.line_spans(&line(0, "abc defgh "), GREY),
        Some(vec![(4, 10, GREY)])
    );
    assert_eq!(
        ta.line_spans(&line(10, "ijk"), GREY),
        Some(vec![(0, 2, GREY)])
    );
    // A range that ends before a line leaves it plain; an empty one is none.
    range.set(Some((0, 2)));
    assert!(ta.line_spans(&line(10, "ijk"), GREY).is_none());
    range.set(Some((3, 3)));
    assert!(ta.line_spans(&line(0, "abc defgh "), GREY).is_none());
}

#[test]
fn a_stale_range_past_the_text_is_clamped_to_the_line() {
    let range = Rc::new(Cell::new(Some((2, 400))));
    let ta = area("abcd", "", colors()).with_muted_range(range);
    assert_eq!(
        ta.line_spans(&line(0, "abcd"), GREY),
        Some(vec![(2, 4, GREY)])
    );
}

#[test]
fn the_muted_run_wins_over_a_highlighted_run_it_overlaps() {
    let range = Rc::new(Cell::new(Some((3, 6))));
    let ta = area("abcdefgh", "", colors())
        .with_highlighter(|_| vec![(1, 8, RED)])
        .with_muted_range(range);
    assert_eq!(
        ta.line_spans(&line(0, "abcdefgh"), GREY),
        Some(vec![(1, 3, RED), (6, 8, RED), (3, 6, GREY)])
    );
}

#[test]
fn moving_only_the_range_changes_the_cache_signature() {
    let range = Rc::new(Cell::new(Some((0, 3))));
    let ta = area("abc", "", colors()).with_muted_range(Rc::clone(&range));
    let muted = ta.cache_sig();
    range.set(None);
    assert!(ta.cache_sig() != muted, "turning final must re-raster");
}

/// Whether any pixel in columns `x0..x1` is within a few levels of `want`.
fn shows(fb: &Framebuffer, x0: u32, x1: u32, want: Color) -> bool {
    let want = rgb_of(want);
    (4..fb.height() - 4).any(|y| {
        (x0..x1).any(|x| {
            rgb(fb, x, y)
                .iter()
                .zip(want)
                .all(|(a, b)| a.abs_diff(b) <= 8)
        })
    })
}

#[test]
fn muted_words_paint_in_the_hint_colour_and_the_rest_in_the_text_colour() {
    let theme = colors();
    let (text, hint) = (theme.text_color.unwrap(), theme.placeholder_color.unwrap());
    // "WWW " is typed, "MMM" is still unconfirmed.
    let range = Rc::new(Cell::new(Some((4, 7))));
    let mut ta = area("WWW MMM", "", theme).with_muted_range(Rc::clone(&range));
    let fb = render(&mut ta, State::Normal);
    // At 24 px each glyph of the monospace font is about 14 px wide, from
    // the 8 px padding: "WWW" ends near x = 50, "MMM" starts near x = 64.
    assert!(shows(&fb, 8, 50, text), "the typed words are text-coloured");
    assert!(
        !shows(&fb, 8, 50, hint),
        "no hint colour in the typed words"
    );
    assert!(
        shows(&fb, 66, 110, hint),
        "the muted words are hint-coloured"
    );
    assert!(
        !shows(&fb, 66, 110, text),
        "no text colour in the muted words"
    );

    // Once final, the same bytes paint in the text colour.
    range.set(None);
    let fb = render(&mut ta, State::Normal);
    assert!(shows(&fb, 66, 110, text));
    assert!(!shows(&fb, 66, 110, hint));
}
