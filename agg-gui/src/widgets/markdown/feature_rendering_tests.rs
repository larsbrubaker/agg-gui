//! Emphasis tests ported 1:1 from agg-sharp
//! `Tests/Agg.Tests/MarkdigAgg/MarkdownFeatureRenderingTests.cs`
//! (`EmphasisRendersItalicBoldAndBoth`, `PairedHtmlStyleTagsStyleTheWordsBetween`,
//! `UnpairedAttributedOrCrossedHtmlStyleTagsAreDropped`,
//! `ItalicWordsDrawSlantedGlyphs`), plus `rust_only_*` tests for the places
//! emphasis has to reach in this port: headings, list items, links, table
//! cells, the matching-text highlight, spacing at a span's edge, and copy as
//! HTML.
//!
//! C# reads each word's `TextWidget` (`Italic`, `Bold`, `StrikeThrough`,
//! `Printer.TypeFaceStyle.FauxItalic`); here a word's style comes from
//! [`MarkdownView::laid_out_words`] and its face from `run_font`.

use std::collections::HashMap;
use std::sync::Arc;

use agg_rust::basics::{is_stop, VertexSource};
use agg_rust::conv_curve::ConvCurve;

use crate::geometry::Size;
use crate::text::{measure_advance, shape_text, Font};
use crate::widget::Widget;

use super::{LayoutItem, LineRun, MarkdownRunStyle, MarkdownView};

// Liberation Sans, agg-sharp's default face, whose plain 'l' stem the slant test relies on.
const TEST_FONT: &[u8] = include_bytes!("../../../../demo/assets/LiberationSans-Regular.ttf");

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(TEST_FONT).expect("test font"))
}

fn render_markdown(markdown: &str) -> MarkdownView {
    crate::widget::set_current_viewport(Size::new(2000.0, 600.0));
    let mut view = MarkdownView::new(markdown, font());
    view.layout(Size::new(2000.0, 2000.0));
    view
}

// Each word with its style, as the C# test's dictionary of text widgets.
fn render_words(markdown: &str) -> (MarkdownView, HashMap<String, MarkdownRunStyle>) {
    let view = render_markdown(markdown);
    let words = view.laid_out_words().into_iter().collect();
    (view, words)
}

fn style(italic: bool, bold: bool, strikethrough: bool) -> MarkdownRunStyle {
    MarkdownRunStyle {
        bold,
        italic,
        strikethrough,
    }
}

#[test]
fn emphasis_renders_italic_bold_and_both() {
    let (view, words) =
        render_words("plain *slanted* **heavy** ***both*** _under_ __double__ ~~gone~~");

    let assert_style = |word: &str, italic: bool, bold: bool| {
        let got = words[word];
        assert_eq!(got.italic, italic, "{word} italic");
        assert_eq!(got.bold, bold, "{word} bold");
        // The flag must reach the face that draws, whichever order the nested spans styled the word in.
        assert_eq!(view.run_font(got).faux_italic(), italic, "{word} face");
    };

    assert_style("plain", false, false);
    assert_style("slanted", true, false);
    assert_style("heavy", false, true);
    assert_style("both", true, true);
    assert_style("under", true, false);
    assert_style("double", false, true);
    assert_style("gone", false, false);
    assert!(words["gone"].strikethrough);
}

#[test]
fn paired_html_style_tags_style_the_words_between() {
    // The rich editor writes these where markdown delimiters cannot flank, so the viewer must show them.
    let (view, words) = render_words(
        "a <strong>heavy</strong> <b>bee</b> <em>slanted</em> <i>eye</i> <del>gone</del> <s>struck</s> \
         <strike>old</strike> <b><i>both</i></b> <B>upper</b> b",
    );

    let assert_style = |word: &str, italic: bool, bold: bool, strike: bool| {
        let got = words[word];
        assert_eq!(got.italic, italic, "{word} italic");
        assert_eq!(got.bold, bold, "{word} bold");
        assert_eq!(got.strikethrough, strike, "{word} strike");
        assert_eq!(view.run_font(got).faux_italic(), italic, "{word} face");
    };

    assert_style("a", false, false, false);
    assert_style("heavy", false, true, false);
    assert_style("bee", false, true, false);
    assert_style("slanted", true, false, false);
    assert_style("eye", true, false, false);
    assert_style("gone", false, false, true);
    assert_style("struck", false, false, true);
    assert_style("old", false, false, true);
    assert_style("both", true, true, false);
    assert_style("upper", false, true, false);
    assert_style("b", false, false, false);
    assert!(!words.keys().any(|word| word.contains('<')));
}

#[test]
fn unpaired_attributed_or_crossed_html_style_tags_are_dropped() {
    for markdown in [
        "<b>open tail",
        "<b class=y>classy</b> tail",
        "<b>crossed <i>over</b> end</i> tail",
    ] {
        let (_, words) = render_words(markdown);
        for (word, got) in &words {
            assert!(!got.bold, "{markdown}: {word} bold");
            assert!(!got.italic, "{markdown}: {word} italic");
        }

        assert!(words.contains_key("tail"), "{markdown}: {words:?}");
        assert!(!words.keys().any(|word| word.contains('<')), "{markdown}");
    }
}

/// Rightmost outline x of `ch` drawn at the origin in `font`.
fn glyph_right(font: &Font, ch: &str, size: f64) -> f64 {
    let (mut paths, _) = shape_text(font, ch, size, 0.0, 0.0);
    let mut right = f64::NEG_INFINITY;
    for path in &mut paths {
        let mut curves = ConvCurve::new(path);
        curves.rewind(0);
        loop {
            let (mut x, mut y) = (0.0, 0.0);
            if is_stop(curves.vertex(&mut x, &mut y)) {
                break;
            }
            right = right.max(x);
        }
    }
    right
}

#[test]
fn italic_words_draw_slanted_glyphs() {
    let (view, words) = render_words("plain *slanted*");
    let size = view.font_size;

    // An 'l' is a vertical stem, so a sheared one reaches further right at the same advance.
    let upright = view.run_font(words["plain"]);
    let slanted = view.run_font(words["slanted"]);
    let upright_right = glyph_right(&upright, "l", size);
    let slanted_right = glyph_right(&slanted, "l", size);

    assert!(
        slanted_right > upright_right + 1.0,
        "{slanted_right} vs {upright_right}"
    );
    assert_eq!(
        measure_advance(&slanted, "l", size),
        measure_advance(&upright, "l", size)
    );
}

#[test]
fn rust_only_emphasis_reaches_headings_list_items_links_and_cells() {
    let (_, words) = render_words(
        "# Big *tilt*\n\n- item **heavy**\n\n[go *there*](https://example.com)\n\n\
         | Name | Note |\n| --- | --- |\n| **Bolt** | ~~old~~ x |",
    );
    assert_eq!(words["tilt"], style(true, false, false));
    assert_eq!(words["Big"], MarkdownRunStyle::default());
    assert_eq!(words["heavy"], style(false, true, false));
    assert_eq!(words["item"], MarkdownRunStyle::default());
    assert_eq!(words["there"], style(true, false, false));
    assert_eq!(words["Bolt"], style(false, true, false));
    assert_eq!(words["old"], style(false, false, true));
    assert_eq!(words["x"], MarkdownRunStyle::default());
    // The header row is a row of its own (pulldown-cmark's TableHead).
    assert!(words.contains_key("Name") && words.contains_key("Note"));
}

#[test]
fn rust_only_bold_runs_measure_in_the_bold_face() {
    let bold_face = Arc::new(
        Font::from_slice(include_bytes!("../../../../demo/assets/Nunito_Bold.ttf"))
            .expect("bold face"),
    );
    crate::widget::set_current_viewport(Size::new(2000.0, 600.0));
    let mut view = MarkdownView::new("**heavy**", font()).with_bold_font(Arc::clone(&bold_face));
    view.layout(Size::new(2000.0, 2000.0));
    let LayoutItem::Line { runs, .. } = &view.items[0] else {
        panic!("a text row");
    };
    let LineRun::Text { width, style, .. } = &runs[0] else {
        panic!("a text run");
    };
    assert!(style.bold);
    assert!(Arc::ptr_eq(&view.run_font(*style), &bold_face));
    assert_eq!(*width, measure_advance(&bold_face, "heavy", view.font_size));
}

#[test]
fn rust_only_span_edges_keep_the_source_spacing() {
    let view = render_markdown("say **bold**, then *slant* end");
    let text: String = view.laid_out_text().to_string();
    assert!(text.contains("say bold, then slant end"), "{text:?}");
}

#[test]
fn rust_only_matching_text_highlights_inside_emphasis() {
    crate::widget::set_current_viewport(Size::new(2000.0, 600.0));
    let mut view =
        MarkdownView::new("*At least 1 part must be selected*", font()).with_matching_text("part");
    view.layout(Size::new(2000.0, 2000.0));
    let LayoutItem::Line { runs, .. } = &view.items[0] else {
        panic!("a text row");
    };
    let highlighted: Vec<_> = runs
        .iter()
        .filter_map(|run| match run {
            LineRun::Text {
                text,
                highlight: true,
                style,
                ..
            } => Some((text.trim().to_string(), *style)),
            _ => None,
        })
        .collect();
    assert_eq!(
        highlighted,
        vec![("part".to_string(), style(true, false, false))]
    );
    for run in runs {
        if let LineRun::Text { style: s, .. } = run {
            assert!(s.italic);
        }
    }
}
