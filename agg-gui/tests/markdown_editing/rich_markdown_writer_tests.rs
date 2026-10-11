//! agg-sharp's `RichMarkdownWriterTests`
//! (`Tests/Agg.Tests/MarkdigAgg/RichMarkdownWriterTests.cs`), ported 1:1
//! against `markdown_editing::inline_writer` and `markdown_writer`.
//!
//! Waiting for the HTML judge (rendering with the viewer's extensions), and
//! ported with it: `RewrittenTextBlocksAndGroupsRenderTheSame`,
//! `StyleEdgesRenderTheSame` and the HTML read-back loop at the end of
//! `LiteralSyntaxIsEscaped`.

use agg_gui::widgets::markdown_editing::inline_writer;
use agg_gui::widgets::markdown_editing::{markdown_parser, markdown_writer};
use agg_gui::widgets::markdown_editing::{
    InlineAtomKind, RichBlock, RichBlockKind, RichDocument, RichInline, RichInlineContext,
};

fn write(inlines: &[RichInline]) -> String {
    inline_writer::write(inlines, RichInlineContext::Paragraph)
}

#[derive(Default)]
struct Style {
    bold: bool,
    italic: bool,
    strike: bool,
    code: bool,
    url: Option<&'static str>,
}

fn run_styled(text: &str, style: Style) -> RichInline {
    let mut run = RichInline::run(text);
    run.bold = style.bold;
    run.italic = style.italic;
    run.strike = style.strike;
    run.link_url = style.url.map(str::to_string);
    if style.code {
        run = run.code();
    }
    run
}

fn run(text: &str) -> RichInline {
    RichInline::run(text)
}

fn bold() -> Style {
    Style {
        bold: true,
        ..Style::default()
    }
}

fn italic() -> Style {
    Style {
        italic: true,
        ..Style::default()
    }
}

fn code() -> Style {
    Style {
        code: true,
        ..Style::default()
    }
}

fn url(url: &'static str) -> Style {
    Style {
        url: Some(url),
        ..Style::default()
    }
}

#[test]
fn nested_styles_share_delimiters() {
    assert_eq!(
        write(&[
            run_styled("a ", bold()),
            run_styled(
                "b",
                Style {
                    bold: true,
                    italic: true,
                    ..Style::default()
                }
            ),
            run_styled(" c", bold()),
        ]),
        "**a *b* c**"
    );
    assert_eq!(
        write(&[run_styled(
            "both",
            Style {
                bold: true,
                italic: true,
                ..Style::default()
            }
        )]),
        "***both***"
    );
    assert_eq!(
        write(&[run("x"), run_styled("y", italic()), run("z")]),
        "x*y*z"
    );
    assert_eq!(
        write(&[run_styled(
            "gone",
            Style {
                strike: true,
                ..Style::default()
            }
        )]),
        "~~gone~~"
    );
}

#[test]
fn whitespace_moves_outside_delimiters() {
    assert_eq!(
        write(&[run("a"), run_styled(" bold ", bold()), run("b")]),
        "a **bold** b"
    );
    assert_eq!(write(&[run("a"), run_styled(" ", bold()), run("b")]), "a b");
    assert_eq!(
        write(&[
            run("x "),
            run_styled(
                " it ",
                Style {
                    italic: true,
                    bold: true,
                    ..Style::default()
                }
            ),
        ]),
        "x  ***it*** "
    );
}

#[test]
fn literal_syntax_is_escaped() {
    assert_eq!(
        write(&[run("2 * 3 and [x] `y`")]),
        "2 \\* 3 and \\[x\\] \\`y\\`"
    );
    assert_eq!(
        write(&[run("snake_case _under_")]),
        "snake_case \\_under\\_"
    );
    assert_eq!(write(&[run("# not a heading")]), "\\# not a heading");
    assert_eq!(write(&[run("1. not a list")]), "1\\. not a list");
    assert_eq!(write(&[run("- not a bullet")]), "\\- not a bullet");
    assert_eq!(write(&[run("> not a quote")]), "\\> not a quote");
    assert_eq!(write(&[run("<b>not html</b>")]), "\\<b>not html\\</b>");
    assert_eq!(
        write(&[run("&lt; stays text, & too")]),
        "\\&lt; stays text, & too"
    );
    assert_eq!(write(&[run("a < b")]), "a < b");
}

#[test]
fn unflanked_emphasis_falls_back_to_tags() {
    // a**"b"**c would be literal asterisks: only the span that cannot flank uses a tag.
    assert_eq!(
        write(&[run("a"), run_styled("\"b\"", bold()), run("c")]),
        "a<strong>\"b\"</strong>c"
    );
    assert_eq!(
        write(&[run("a"), run_styled("b", bold()), run("c")]),
        "a**b**c"
    );
    assert_eq!(
        write(&[
            run_styled(
                "b",
                Style {
                    code: true,
                    bold: true,
                    ..Style::default()
                }
            ),
            run("c")
        ]),
        "<strong>`b`</strong>c"
    );
}

#[test]
fn table_cells_escape_pipes_outside_code() {
    let cell = inline_writer::write(
        &[run("a|b "), run_styled("c|d", code())],
        RichInlineContext::TableCell,
    );
    assert_eq!(cell, "a\\|b `c|d`");
    assert_eq!(write(&[run("a|b")]), "a|b");
}

#[test]
fn link_url_and_title_keep_entities_and_lines() {
    let mut link = run_styled("t", url("/a&amp;b\nc"));
    link.link_title = Some("x &copy;\ny".to_string());
    let written = write(&[link]);
    assert_eq!(written, "[t](/a&amp;amp;b%0Ac \"x &amp;copy; y\")");

    let read = &markdown_parser::parse(&written).blocks[0].inlines[0];
    assert!(read.is_run());
    assert_eq!(read.link_url.as_deref(), Some("/a&amp;b%0Ac"));
    assert_eq!(read.link_title.as_deref(), Some("x &copy; y"));
}

#[test]
fn multi_line_heading_content_stays_one_heading() {
    let hard_break = RichInline::atom(InlineAtomKind::HardBreak, "  \n");
    let html = RichInline::atom(InlineAtomKind::Html, "<span\nclass=\"x\">");
    let cases: [(u8, Vec<RichInline>, &str); 3] = [
        (3, vec![run("a"), hard_break, run("b")], "### a b"),
        (
            1,
            vec![run("a "), html.clone(), run("b")],
            "a <span\nclass=\"x\">b\n===",
        ),
        (
            3,
            vec![run("a "), html, run("b")],
            "### a <span class=\"x\">b",
        ),
    ];
    for (level, inlines, expected) in cases {
        let mut document = RichDocument::new();
        let mut heading = RichBlock::new(RichBlockKind::Heading);
        heading.heading_level = level;
        heading.inlines = inlines;
        heading.dirty = true;
        document.blocks.push(heading);
        let written = markdown_writer::write(&document);
        assert_eq!(written, expected);

        let blocks = markdown_parser::parse(&written).blocks;
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].kind, RichBlockKind::Heading);
        assert_eq!(blocks[0].heading_level, level);
    }
}

#[test]
fn headings_are_atx() {
    let mut document = markdown_parser::parse("Title\n=====\n\nText\n");
    document.blocks[0].dirty = true;
    assert_eq!(markdown_writer::write(&document), "# Title\n\nText\n");

    document.blocks[0].inlines = vec![run("C #")];
    assert_eq!(markdown_writer::write(&document), "# C \\#\n\nText\n");
}

#[test]
fn links_keep_styled_text_inside() {
    let link = run_styled("see ", url("http://x.com"));
    let bold_in_link = run_styled(
        "this",
        Style {
            url: Some("http://x.com"),
            bold: true,
            ..Style::default()
        },
    );
    assert_eq!(write(&[link, bold_in_link]), "[see **this**](http://x.com)");

    let mut titled = run_styled("t", url("a b(c)"));
    titled.link_title = Some("say \"hi\"".to_string());
    assert_eq!(write(&[titled]), "[t](<a b(c)> \"say \\\"hi\\\"\")");

    let mut reference = run_styled("text", url("http://docs"));
    reference.link_label = Some("docs".to_string());
    assert_eq!(write(&[reference]), "[text][docs]");

    // "!" before a link would make it an image.
    assert_eq!(
        write(&[run("wow!"), run_styled("l", url("u"))]),
        "wow\\![l](u)"
    );
}

#[test]
fn atom_inside_style_stays_inside() {
    let image = RichInline::atom(InlineAtomKind::Image, "![i](a.png)").bold();
    assert_eq!(
        write(&[
            run_styled("a ", bold()),
            image.clone(),
            run_styled(" b", bold())
        ]),
        "**a ![i](a.png) b**"
    );
    assert_eq!(write(std::slice::from_ref(&image)), "**![i](a.png)**");
    assert_eq!(write(&[image, run(" b")]), "**![i](a.png)** b");

    let hard_break = RichInline::atom(InlineAtomKind::HardBreak, "  \n");
    assert_eq!(write(&[run("a"), hard_break, run("- b")]), "a  \n\\- b");
}

#[test]
fn code_uses_enough_backticks() {
    assert_eq!(write(&[run_styled("x", code())]), "`x`");
    assert_eq!(write(&[run_styled("a`b", code())]), "``a`b``");
    assert_eq!(write(&[run_styled("`x", code())]), "`` `x ``");
    assert_eq!(write(&[run_styled(" x ", code())]), "`  x  `");
    assert_eq!(
        write(&[run_styled(
            "*not em*",
            Style {
                code: true,
                bold: true,
                ..Style::default()
            }
        )]),
        "**`*not em*`**"
    );
}
