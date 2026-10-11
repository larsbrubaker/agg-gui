//! agg-sharp's `RichEditorPolishTests` (`Tests/Agg.Tests/MarkdigAgg/
//! RichEditorPolishTests.cs`): one-line aligned HTML in the parser and
//! writer, the space after a list's last item (`document_view::ends_list`)
//! and the code and stripe shading on light and dark themes.  C#'s
//! `[Arguments]` rows run as a loop, each case named in its message.
//!
//! C# measures the link with `ThemeConfig.ContrastRatio`, the unrounded WCAG
//! ratio of two 8-bit colours; agg-gui has no ThemeConfig, so `wcag_ratio`
//! below is that measuring stick.

use std::sync::Arc;

use agg_gui::text::Font;
use agg_gui::widgets::markdown_editing::document_view::ends_list;
use agg_gui::widgets::markdown_editing::edit_operations as ops;
use agg_gui::widgets::markdown_editing::markdown_parser::parse;
use agg_gui::widgets::markdown_editing::markdown_writer::write;
use agg_gui::widgets::markdown_editing::{
    DocPosition, RichAlignment, RichBlockKind, RichBlockLayout, RichInline, RichLayoutFonts,
    RichLayoutStyle, RichThemeColors,
};
use agg_gui::Color;

static REGULAR: &[u8] = include_bytes!("../../../demo/assets/Nunito_Regular.ttf");

fn byte(c: f32) -> i32 {
    (c * 255.0).round() as i32
}

// C# `ThemeConfig.ContrastRatio`: WCAG 2 on 8-bit channels, unrounded.
fn wcag_ratio(a: Color, b: Color) -> f64 {
    fn channel(c: f32) -> f64 {
        let c = byte(c) as f64 / 255.0;
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }
    let lum = |c: Color| 0.2126 * channel(c.r) + 0.7152 * channel(c.g) + 0.0722 * channel(c.b);
    let (la, lb) = (lum(a), lum(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

#[test]
fn code_and_stripe_shading_show_on_light_and_dark_themes() {
    let font = Arc::new(Font::from_slice(REGULAR).expect("demo font"));
    let themes = [
        (Color::from_rgb8(0x33, 0x33, 0x33), Color::white()),
        (
            Color::from_rgb8(0xdd, 0xdd, 0xdd),
            Color::from_rgb8(0x33, 0x33, 0x33),
        ),
    ];
    for (text, background) in themes {
        let theme = RichThemeColors {
            text,
            // C#'s ThemeConfig default accent.
            accent: Color::from_rgb8(0x7A, 0xD7, 0xF0),
            background,
        };
        let style = RichLayoutStyle::with_theme(
            RichLayoutFonts {
                regular: Arc::clone(&font),
                bold: None,
                mono: None,
            },
            Some(&theme),
            Some(1.0),
        );
        for shade in [style.code_background_color(), style.table_stripe_color()] {
            // C# `background.Blend(new Color(shade, 255), shade.Alpha0To255 / 255.0)`.
            let weight = byte(shade.a) as f32 / 255.0;
            let shown = background.lerp(shade.with_alpha(1.0), weight);
            let difference = (byte(shown.r) - byte(background.r)).abs();
            assert!(
                difference >= 10,
                "shading a reader cannot see marks nothing"
            );
        }

        assert!(wcag_ratio(style.link_color, background) >= 4.4);
    }
}

fn style() -> RichLayoutStyle {
    let font = Arc::new(Font::from_slice(REGULAR).expect("demo font"));
    RichLayoutStyle::new(
        RichLayoutFonts {
            regular: font,
            bold: None,
            mono: None,
        },
        Some(1.0),
    )
}

fn run_text(inline: &RichInline) -> Option<&str> {
    inline.as_run().map(|run| run.text.as_str())
}

#[test]
fn one_line_aligned_element_is_an_aligned_block_that_round_trips() {
    let cases = [
        (
            "<div align=\"center\">Centered: version 3</div>\n",
            RichBlockKind::Paragraph,
            RichAlignment::Center,
        ),
        (
            "Intro\n\n<p align='right'> Right **bold** </p>\n\nAfter\n",
            RichBlockKind::Paragraph,
            RichAlignment::Right,
        ),
        (
            "<h2 align=\"center\">Title</h2>\r\n",
            RichBlockKind::Heading,
            RichAlignment::Center,
        ),
    ];
    for (markdown, kind, alignment) in cases {
        let document = parse(markdown);

        let aligned = document.blocks.iter().find(|b| b.alignment == alignment);
        let aligned = aligned.unwrap_or_else(|| panic!("{markdown:?}: no {alignment:?} block"));
        assert_eq!(aligned.kind, kind, "{markdown:?}");
        assert_eq!(write(&document), markdown, "{markdown:?}");
    }
}

#[test]
fn one_line_aligned_heading_keeps_its_level_and_text() {
    let document = parse("<h3 align=\"right\">Lid</h3>\n");

    assert_eq!(document.blocks.len(), 1);
    assert_eq!(document.blocks[0].heading_level, 3);
    assert_eq!(run_text(&document.blocks[0].inlines[0]), Some("Lid"));
}

#[test]
fn enter_before_a_one_line_aligned_heading_keeps_it_a_heading() {
    for alignment in ["left", "center"] {
        // The heading the Enter pushes down is still clean, so it is written
        // from its own source: that source must be its markdown ("## Title"),
        // not the bare text inside the tags.
        let mut document = parse(&format!("<h2 align=\"{alignment}\">Title</h2>\n"));
        ops::split_block(&mut document, DocPosition::new(0, 0)).unwrap();

        let reread = parse(&write(&document));
        let title = reread
            .blocks
            .iter()
            .find(|b| b.inlines.first().and_then(run_text) == Some("Title"));
        let title = title.unwrap_or_else(|| panic!("{alignment}: no Title block"));
        assert_eq!(title.kind, RichBlockKind::Heading, "{alignment}");
        assert_eq!(title.heading_level, 2, "{alignment}");
    }
}

#[test]
fn clean_one_line_aligned_heading_source_is_its_markdown() {
    let document = parse("<h2 align=\"center\">Title</h2>\n");

    assert_eq!(document.blocks[0].original_source, "## Title");
}

#[test]
fn edited_one_line_aligned_element_is_written_in_the_wrapper_form() {
    let mut document = parse("<div align=\"center\">Old</div>\n");
    let block = &mut document.blocks[0];
    block.inlines = vec![RichInline::run("New")];
    block.dirty = true;

    let written = write(&document);
    assert_eq!(written, "<div align=\"center\">\n\nNew\n\n</div>\n");

    // And the written form reads back as the same centered paragraph.
    let reread = parse(&written);
    assert_eq!(reread.blocks.len(), 1);
    assert_eq!(reread.blocks[0].alignment, RichAlignment::Center);
}

#[test]
fn other_html_stays_raw() {
    for markdown in [
        "<div align=\"center\"><div>nested</div></div>\n",
        "<div align=\"center\" class=\"x\">extra attribute</div>\n",
        "<div align=\"center\"># not a paragraph</div>\n",
    ] {
        let document = parse(markdown);

        assert_eq!(document.blocks[0].kind, RichBlockKind::Raw, "{markdown:?}");
        assert_eq!(write(&document), markdown, "{markdown:?}");
    }
}

#[test]
fn list_end_takes_paragraph_spacing() {
    let document = parse("- a\n- b\n\n1. c\n\nAfter\n");
    let blocks = &document.blocks;
    let style = style();

    assert_eq!(blocks.len(), 4);
    assert!(!ends_list(blocks, 0));
    assert!(
        ends_list(blocks, 1),
        "a bullet list straight into a numbered one needs a gap"
    );
    assert!(
        ends_list(blocks, 2),
        "a list straight into a paragraph needs a gap"
    );
    assert!(!ends_list(blocks, 3));

    let inside = RichBlockLayout::layout(&blocks[0], 400.0, &style, 1, false)
        .unwrap()
        .height;
    let last = RichBlockLayout::layout(&blocks[1], 400.0, &style, 1, true)
        .unwrap()
        .height;
    let expected =
        style.block_spacing(&blocks[3], false).1 - style.block_spacing(&blocks[0], false).1;
    assert!(((last - inside) - expected).abs() <= 0.001);
    assert_eq!(
        style.block_spacing(&blocks[1], true).1,
        style.block_spacing(&blocks[3], false).1
    );
}

#[test]
fn nested_item_before_its_parents_sibling_does_not_end_the_list() {
    let blocks = parse("- a\n  - b\n- c\n").blocks;

    assert!(!ends_list(&blocks, 1));
    assert!(ends_list(&blocks, 2));
}
