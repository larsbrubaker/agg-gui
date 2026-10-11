//! agg-sharp's `RichBlockLayoutTests` (`Tests/Agg.Tests/MarkdigAgg/
//! RichBlockLayoutTests.cs`), ported 1:1 against
//! `markdown_editing::block_layout`.
//!
//! C# lays out with `AggContext.DefaultFont`/`DefaultFontBold` and the code
//! block's monospace face; agg-gui has no implicit fonts, so these tests hand
//! the style the demo's Nunito Regular/Bold and Cascadia Code.  The emoji
//! tests give the regular face Noto Emoji as a fallback, as C#'s
//! `EmojiFallback` does process-wide.  Offsets count chars, so an emoji is
//! one caret position here (two in C#): the offsets C# lists as inside a
//! surrogate pair have stops here, `CaretRect(3)` after "a😀" is offset 2,
//! and `CaretStopsFallOnlyBetweenWholeCharacters` names the grapheme starts
//! (C#'s `StringInfo.ParseCombiningCharacters`) as char offsets.  `LineOf(-1)`
//! cannot be written with a `usize`; one past the end is the out-of-range
//! offset instead.

use std::collections::HashSet;
use std::sync::Arc;

use agg_gui::text::Font;
use agg_gui::widgets::markdown_editing::{
    InlineAtomKind, RichAlignment, RichBlock, RichBlockKind, RichBlockLayout, RichCaret,
    RichCaretStop, RichInline, RichLayoutError, RichLayoutFonts, RichLayoutFragment,
    RichLayoutLine, RichLayoutStyle, RichListInfo,
};
use agg_gui::{Point, Size};

const SENTENCE: &str = "The quick brown fox jumps over the lazy dog and keeps on running far away";

static REGULAR: &[u8] = include_bytes!("../../../demo/assets/Nunito_Regular.ttf");
static BOLD: &[u8] = include_bytes!("../../../demo/assets/Nunito_Bold.ttf");
static MONO: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");
static EMOJI: &[u8] = include_bytes!("../../../demo/assets/NotoEmoji-Regular.ttf");

fn font(data: &[u8]) -> Arc<Font> {
    Arc::new(Font::from_slice(data).expect("demo font"))
}

fn fonts(emoji: bool) -> RichLayoutFonts {
    let regular = Font::from_slice(REGULAR).expect("demo font");
    let regular = if emoji {
        regular.with_fallback(font(EMOJI))
    } else {
        regular
    };
    RichLayoutFonts {
        regular: Arc::new(regular),
        bold: Some(font(BOLD)),
        mono: Some(font(MONO)),
    }
}

// A fixed scale so the numbers do not depend on the machine's display.
fn style() -> RichLayoutStyle {
    RichLayoutStyle::new(fonts(false), Some(1.0))
}

fn emoji_style() -> RichLayoutStyle {
    RichLayoutStyle::new(fonts(true), Some(1.0))
}

fn paragraph(inlines: Vec<RichInline>) -> RichBlock {
    let mut block = RichBlock::new(RichBlockKind::Paragraph);
    block.inlines = inlines;
    block
}

fn layout(block: &RichBlock, width: f64, style: &RichLayoutStyle) -> RichBlockLayout {
    RichBlockLayout::layout(block, width, style, 1, false).expect("text block")
}

fn chars(s: &str) -> Vec<char> {
    s.chars().collect()
}

#[test]
fn text_wraps_into_more_lines_as_the_width_shrinks() {
    let block = paragraph(vec![RichInline::run(SENTENCE)]);
    let style = style();

    let wide = layout(&block, 5000.0, &style).lines.len();
    let medium = layout(&block, 200.0, &style).lines.len();
    let narrow = layout(&block, 80.0, &style).lines.len();

    assert_eq!(wide, 1);
    assert!(medium > wide);
    assert!(narrow > medium);

    // Every wrapped line but the last fits; wraps happen at spaces, so no line
    // starts with one.
    let sentence = chars(SENTENCE);
    for line in &layout(&block, 200.0, &style).lines {
        assert!(line.width <= 200.0);
        assert_ne!(sentence[line.start], ' ');
    }
}

#[test]
fn a_word_wider_than_the_line_breaks_between_characters() {
    let layout = layout(
        &paragraph(vec![RichInline::run("W".repeat(60))]),
        100.0,
        &style(),
    );

    assert!(layout.lines.len() > 1);
    for line in &layout.lines {
        assert!(line.width <= 100.0);
    }
}

#[test]
fn centered_and_right_aligned_lines_are_offset() {
    let width = 400.0;
    let mut block = paragraph(vec![RichInline::run("Short line")]);
    block.alignment = RichAlignment::Center;
    let centered = layout(&block, width, &style()).lines.remove(0);
    block.alignment = RichAlignment::Right;
    let right = layout(&block, width, &style()).lines.remove(0);

    assert!(centered.width > 0.0);
    assert!((centered.left - (width - centered.width) / 2.0).abs() <= 0.001);
    assert!((right.left - (width - right.width)).abs() <= 0.001);
    assert!((centered.fragments[0].x - centered.left).abs() <= 0.001);
}

#[test]
fn heading_is_taller_than_paragraph() {
    let paragraph_line = layout(&paragraph(vec![RichInline::run("Title")]), 400.0, &style())
        .lines
        .remove(0);
    let mut heading_block = paragraph(vec![RichInline::run("Title")]);
    heading_block.kind = RichBlockKind::Heading;
    heading_block.heading_level = 1;
    let heading = layout(&heading_block, 400.0, &style()).lines.remove(0);

    assert!(heading.top - heading.bottom > paragraph_line.top - paragraph_line.bottom);
    assert!(heading.width > paragraph_line.width);
}

#[test]
fn bold_is_wider_than_regular() {
    let regular = layout(
        &paragraph(vec![RichInline::run("Some words")]),
        400.0,
        &style(),
    )
    .lines
    .remove(0);
    let bold = layout(
        &paragraph(vec![RichInline::run("Some words").bold()]),
        400.0,
        &style(),
    )
    .lines
    .remove(0);

    assert!(bold.width > regular.width);
}

#[test]
fn list_depth_shifts_text_right_and_shows_the_marker() {
    let style = style();
    let list_item = |depth: usize, ordered: bool| {
        let mut block = paragraph(vec![RichInline::run("Item")]);
        block.kind = RichBlockKind::ListItem;
        block.list = Some(RichListInfo {
            depth,
            ordered,
            marker: if ordered { '.' } else { '-' },
            start_number: 1,
            loose: false,
        });
        RichBlockLayout::layout(&block, 400.0, &style, 3, false).expect("list item")
    };

    let paragraph_layout = layout(&paragraph(vec![RichInline::run("Item")]), 400.0, &style);
    let top = list_item(0, false);
    let nested = list_item(1, false);
    let numbered = list_item(0, true);

    assert!(top.lines[0].left > paragraph_layout.lines[0].left);
    assert!((nested.lines[0].left - top.lines[0].left - style.list_indent()).abs() <= 0.001);
    assert_eq!(top.marker.as_deref(), Some("\u{2022}"));
    assert!(top.marker_is_bullet);
    assert_eq!(nested.marker.as_deref(), Some("\u{25E6}"));
    assert_eq!(numbered.marker.as_deref(), Some("3."));
    assert!(numbered.marker_x < numbered.text_left);
    assert!(numbered.marker_x >= 0.0);
}

// The UI font has no emoji, so a bare emoji is zero width and its carets
// coincide with its neighbours'. Tests about emoji give the regular face Noto
// Emoji as a fallback, the way an app does.
#[test]
fn hit_test_of_every_caret_center_returns_its_offset() {
    let block = paragraph(vec![
        RichInline::run("Plain words then "),
        RichInline::run("bold text").bold(),
        RichInline::run(" and "),
        RichInline::run("slanted words").italic(),
        RichInline::run(" smile \u{1F600}\u{1F600} now "),
        RichInline::atom(InlineAtomKind::Image, "![pic](a.png)"),
        RichInline::run(" a "),
        RichInline::run("link").link("https://example.com"),
        RichInline::atom(InlineAtomKind::HardBreak, "\\\n"),
        RichInline::run("code()").code(),
        RichInline::atom(InlineAtomKind::Html, "<b>"),
        RichInline::run(" end of the\tblock"),
    ]);
    let layout = layout(&block, 120.0, &emoji_style());

    // C#'s emoji are surrogate pairs with no caret between their halves; a
    // char is one offset, so every offset has a stop here.
    assert!(layout.lines.len() > 3);
    assert_caret_round_trips(&layout, block.text_length(0, 0), &[]);
}

#[test]
fn code_block_keeps_spaces_and_breaks_at_newlines() {
    let mut block = RichBlock::new(RichBlockKind::CodeBlock);
    block.code_text =
        "int  x = 1;\n\nvar aVeryLongIdentifierThatCannotFitOnOneLineAtAll = 2;\n".into();
    let layout = layout(&block, 150.0, &style());

    // Line 1, the empty line, the long line char-wrapped onto at least two,
    // and the empty line after the trailing newline.
    assert!(layout.lines.len() >= 5);
    assert_eq!(layout.lines[0].fragments[0].text, "int  x = 1;");
    assert_eq!(layout.lines[1].start, layout.lines[1].end - 1);
    assert!(layout.code_background.is_some());
    assert_caret_round_trips(&layout, block.text_length(0, 0), &[]);
}

#[test]
fn empty_block_has_one_line_with_a_caret() {
    let layout = layout(&paragraph(vec![]), 400.0, &style());
    let caret = layout.caret_rect_at(0, false, 1.0).expect("offset 0");

    assert_eq!(layout.lines.len(), 1);
    assert!(caret.height > 5.0);
    assert!(layout.height > caret.height);
    assert_eq!(layout.hit_test(Point::new(200.0, 0.0)), RichCaret::new(0));
    assert_eq!(layout.hit_test(caret.center()), RichCaret::new(0));
}

#[test]
fn table_blocks_are_left_to_the_table_layout() {
    let block = RichBlock::new(RichBlockKind::Table);

    let result = RichBlockLayout::layout(&block, 400.0, &style(), 1, false);
    assert!(matches!(result, Err(RichLayoutError::TableBlock)));
}

#[test]
fn clicking_just_after_an_emoji_puts_the_caret_after_it() {
    let layout = layout(
        &paragraph(vec![RichInline::run("a\u{1F600}b")]),
        400.0,
        &emoji_style(),
    );
    // After the emoji is offset 2 here (3 in C#'s UTF-16 offsets).
    let after_emoji = layout.caret_rect_at(2, false, 1.0).expect("offset 2");
    let center = after_emoji.center();

    assert!(
        after_emoji.left()
            > layout
                .caret_rect_at(1, false, 1.0)
                .expect("offset 1")
                .left()
    );
    assert_eq!(
        layout.hit_test(Point::new(center.x + 0.5, center.y)),
        RichCaret::new(2)
    );
    assert_eq!(
        layout.hit_test(Point::new(center.x - 0.5, center.y)),
        RichCaret::new(2)
    );
}

#[test]
fn a_word_broken_by_character_has_a_caret_at_the_line_end() {
    let layout = layout(
        &paragraph(vec![RichInline::run("W".repeat(60))]),
        100.0,
        &style(),
    );
    let first = &layout.lines[0];
    let second = &layout.lines[1];

    let click_right = layout.hit_test(Point::new(first.left + first.width + 30.0, first.baseline));
    assert_eq!(click_right, RichCaret::at_line_end(second.start));
    let caret = layout.caret_rect(click_right, 1.0).expect("line-end caret");
    assert!((caret.center().x - (first.left + first.width)).abs() <= 0.001);
    assert!(std::ptr::eq(
        layout.line_of(second.start, true).expect("in range"),
        first
    ));
    assert!(std::ptr::eq(
        layout.line_of(second.start, false).expect("in range"),
        second
    ));
    assert_caret_round_trips(&layout, 60, &[]);
}

#[test]
fn a_box_pushed_down_with_no_space_leaves_a_caret_at_the_line_end() {
    let mut style = style();
    style.image_size = Some(Box::new(|_| Size::new(80.0, 10.0)));
    let block = paragraph(vec![
        RichInline::run("word"),
        RichInline::atom(InlineAtomKind::Image, "![i](i.png)"),
    ]);
    let layout = layout(&block, 100.0, &style);
    let first = &layout.lines[0];

    assert_eq!(layout.lines.len(), 2);
    assert_eq!(
        layout.hit_test(Point::new(first.left + first.width + 30.0, first.baseline)),
        RichCaret::at_line_end(4)
    );
    assert_caret_round_trips(&layout, 5, &[]);
}

#[test]
fn text_atoms_wrap_like_words() {
    let style = style();
    let block = paragraph(vec![
        RichInline::run("see "),
        RichInline::atom(InlineAtomKind::Autolink, "<https://x.com>"),
        RichInline::run("."),
    ]);

    // Narrower than the link and its "." the pair breaks by character like a
    // long word, which may put the "." first; at every width it fits, it stays
    // on the link's line.
    let link_and_dot: f64 = layout(&block, 1000.0, &style).lines[0]
        .fragments
        .iter()
        .map(|f| {
            if f.inline_index.is_some_and(|i| i > 0) {
                f.width
            } else {
                0.0
            }
        })
        .sum();

    let mut width = 200.0;
    while width >= link_and_dot {
        let layout = layout(&block, width, &style);
        for line in &layout.lines {
            // The "." after the autolink (offset 5) never starts a line on its own.
            assert_ne!(line.start, 5);
        }
        assert_caret_round_trips(&layout, 6, &[]);
        width -= 1.0;
    }

    let long_link = paragraph(vec![
        RichInline::run("a "),
        RichInline::atom(
            InlineAtomKind::Autolink,
            "<https://example.com/a/very/long/path/that/goes/on>",
        ),
    ]);
    let long_layout = layout(&long_link, 80.0, &style);
    assert!(long_layout.lines.len() > 2);
    for line in &long_layout.lines {
        assert!(line.width <= 80.0);
    }
    assert_caret_round_trips(&long_layout, 3, &[]);

    // Every piece of the split link stands for the whole atom at offset 2.
    let link_length = long_link.inlines[1]
        .as_atom()
        .expect("atom")
        .raw_markdown
        .chars()
        .count() as f64;
    let mut pieces: Vec<(&RichLayoutLine, &RichLayoutFragment)> = Vec::new();
    for line in &long_layout.lines {
        for fragment in &line.fragments {
            if fragment.atom.is_some() {
                pieces.push((line, fragment));
                assert_eq!(fragment.start, 2);
                assert_eq!(fragment.length, 1);
            }
        }
    }

    // A click on any piece goes before or after the whole link by which half
    // of it was clicked, on lines with no caret stop of their own too.
    assert!(pieces.len() > 2);
    assert!(long_layout
        .lines
        .iter()
        .any(|line| line.caret_stops.is_empty()));
    for (line, piece) in &pieces {
        for x in [piece.x + 0.01, piece.x + piece.width - 0.01] {
            let piece_chars = piece.text.chars().count() as f64;
            let fraction = (piece.start_in_inline as f64
                + (x - piece.x) / piece.width * piece_chars)
                / link_length;
            let expected = RichCaret::new(if fraction < 0.5 { 2 } else { 3 });
            assert_eq!(long_layout.hit_test(Point::new(x, line.baseline)), expected);
        }
    }

    let (first_line, first_piece) = pieces[0];
    let (last_line, last_piece) = pieces[pieces.len() - 1];
    assert_eq!(
        long_layout.hit_test(Point::new(first_piece.x + 0.01, first_line.baseline)),
        RichCaret::new(2)
    );
    assert_eq!(
        long_layout.hit_test(Point::new(
            last_piece.x + last_piece.width - 0.01,
            last_line.baseline
        )),
        RichCaret::new(3)
    );
}

#[test]
fn a_mark_after_a_wrap_space_stays_with_it() {
    // "́" is a combining acute accent on the space before it.
    let block = paragraph(vec![RichInline::run("word \u{0301}word word")]);
    let mut width = 200.0;
    while width >= 10.0 {
        let layout = layout(&block, width, &style());
        for line in &layout.lines {
            assert_ne!(line.start, 5);
        }
        width -= 1.0;
    }
}

#[test]
fn caret_stops_fall_only_between_whole_characters() {
    // A ZWJ family emoji with a variation selector, two flags, Devanagari with
    // a spacing mark, e + combining acute.
    let text = "a\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}\u{FE0F}b\u{1F1FA}\u{1F1F8}\u{1F1EB}\u{1F1F7}c\u{0915}\u{093F}de\u{0301}f";
    let layout = layout(&paragraph(vec![RichInline::run(text)]), 1000.0, &style());
    let stops: HashSet<usize> = layout
        .lines
        .iter()
        .flat_map(|line| line.caret_stops.iter().map(|stop| stop.caret.offset))
        .collect();

    // The grapheme starts (C# asks StringInfo.ParseCombiningCharacters), in
    // chars: a, the family, b, each flag, c, the Devanagari pair, d, e + accent,
    // f, and the end.
    let expected: HashSet<usize> = [0, 1, 7, 8, 10, 12, 13, 15, 16, 18, 19]
        .into_iter()
        .collect();
    assert_eq!(text.chars().count(), 19);
    assert_eq!(stops, expected);

    // Narrow enough to break by character, no line starts inside one.
    let narrow = RichBlockLayout::layout(
        &paragraph(vec![RichInline::run(text)]),
        1.0,
        &style(),
        1,
        false,
    )
    .expect("text block");
    for line in &narrow.lines {
        assert!(expected.contains(&line.start));
    }
}

#[test]
fn tabs_advance_to_the_next_four_space_stop() {
    let style = style();
    let paragraph_layout = layout(
        &paragraph(vec![RichInline::run("a\tb\t\tc")]),
        400.0,
        &style,
    );
    let mut code_block = RichBlock::new(RichBlockKind::CodeBlock);
    code_block.code_text = "\tx\ty".into();
    let code = layout(&code_block, 400.0, &style);
    for layout in [&paragraph_layout, &code] {
        let line = &layout.lines[0];
        let tab_stop = 4.0
            * line.fragments[0]
                .face
                .as_ref()
                .expect("text face")
                .advance(' ');
        for i in 1..line.caret_stops.len() {
            assert!(line.caret_stops[i].x > line.caret_stops[i - 1].x);
        }

        for fragment in &line.fragments {
            if fragment.text == "\t" {
                assert!(fragment.width > 0.0);
                let end = fragment.x + fragment.width - line.left;
                assert!((end / tab_stop - (end / tab_stop).round()).abs() < 0.0001);
            }
        }
    }
}

#[test]
fn trailing_space_carets_stay_inside_the_column() {
    // The clamped stops share one x, so a click reaches only the first of
    // them; that is deliberate (the spaces past the edge are invisible) and
    // why this test checks positions, not the click round trip.
    let mut block = paragraph(vec![RichInline::run(format!("word{}", " ".repeat(40)))]);
    block.alignment = RichAlignment::Right;
    let layout = layout(&block, 200.0, &style());

    for offset in 0..=block.text_length(0, 0) {
        assert!(
            layout
                .caret_rect_at(offset, false, 1.0)
                .expect("in range")
                .center()
                .x
                <= 200.0
        );
    }
}

#[test]
fn a_wide_list_number_grows_the_gutter() {
    let mut block = paragraph(vec![RichInline::run("Item")]);
    block.kind = RichBlockKind::ListItem;
    block.list = Some(RichListInfo {
        ordered: true,
        depth: 0,
        marker: '.',
        start_number: 1,
        loose: false,
    });
    let style = style();
    let small = RichBlockLayout::layout(&block, 400.0, &style, 1, false).expect("list item");
    let wide = RichBlockLayout::layout(&block, 400.0, &style, 1_000_000, false).expect("list item");

    assert!(wide.marker_x >= 0.0);
    assert!(wide.text_left > small.text_left);
    // C# asks for LineOf(-1); one past the end is the out-of-range offset here.
    assert_eq!(
        wide.line_of(5, false).err(),
        Some(RichLayoutError::OffsetOutOfRange {
            offset: 5,
            length: 4
        })
    );
}

// Every offset but the listed ones (inside a character) has a caret; clicking
// any caret's centre gives back that caret, line-end carets included.
fn assert_caret_round_trips(
    layout: &RichBlockLayout,
    length: usize,
    offsets_without_stops: &[usize],
) {
    let stops: Vec<RichCaretStop> = layout
        .lines
        .iter()
        .flat_map(|line| line.caret_stops.iter().copied())
        .collect();

    for offset in 0..=length {
        let has_stop = stops.iter().any(|stop| stop.caret.offset == offset);
        assert_eq!(
            has_stop,
            !offsets_without_stops.contains(&offset),
            "offset {offset}"
        );
    }

    for stop in &stops {
        let caret = layout.caret_rect(stop.caret, 1.0).expect("stop in range");
        assert_eq!(layout.hit_test(caret.center()), stop.caret);
    }

    // Lines run top to bottom and stay inside the block.
    for i in 1..layout.lines.len() {
        assert!(layout.lines[i].top <= layout.lines[i - 1].bottom + 0.001);
    }

    assert!(layout.lines[layout.lines.len() - 1].bottom >= 0.0);
    assert!(layout.lines[0].top <= layout.height);
}
