//! Rust-only tests: the viewer renders the Markdig extensions of C#'s
//! pipeline as MarkdigAgg does (`extensions.rs`): `^sup^`, `++ins++` and
//! `==mark==` bold, `~sub~` struck through, bare `www.`/`http://` links as
//! links.  agg-sharp's `MarkdownFeatureRenderingTests` has no extension cases
//! to port.

use super::parse::parse_markdown;
use super::{InlineItem, MarkdownRunStyle, ParagraphItem};

/// Each text item of the first paragraph: its text, style and link.
fn runs(markdown: &str) -> Vec<(String, MarkdownRunStyle, Option<String>, bool)> {
    let items = parse_markdown(markdown);
    let Some(ParagraphItem::Flow { items, .. }) = items.first() else {
        panic!("expected a paragraph");
    };
    items
        .iter()
        .filter_map(|item| match item {
            InlineItem::Text {
                text,
                style,
                link,
                glue,
                ..
            } => Some((text.clone(), *style, link.clone(), *glue)),
            _ => None,
        })
        .collect()
}

const BOLD: MarkdownRunStyle = MarkdownRunStyle {
    bold: true,
    italic: false,
    strikethrough: false,
};
const STRUCK: MarkdownRunStyle = MarkdownRunStyle {
    bold: false,
    italic: false,
    strikethrough: true,
};
const PLAIN: MarkdownRunStyle = MarkdownRunStyle {
    bold: false,
    italic: false,
    strikethrough: false,
};

#[test]
fn rust_only_emphasis_extras_render_bold_and_subscript_struck() {
    let runs = runs("x^2^ and ++new++ and ==hot== and H~2~O");
    let styled: Vec<(&str, MarkdownRunStyle, bool)> = runs
        .iter()
        .map(|(text, style, _, glue)| (text.as_str(), *style, *glue))
        .collect();
    assert_eq!(
        styled,
        [
            ("x", PLAIN, false),
            ("2", BOLD, true),
            ("and", PLAIN, false),
            ("new", BOLD, false),
            ("and", PLAIN, false),
            ("hot", BOLD, false),
            ("and H", PLAIN, false),
            ("2", STRUCK, true),
            ("O", PLAIN, true),
        ]
    );
}

#[test]
fn rust_only_bare_links_are_links() {
    let runs = runs("see www.example.com/a or https://x.org/b.");
    let links: Vec<(&str, Option<&str>)> = runs
        .iter()
        .map(|(text, _, link, _)| (text.as_str(), link.as_deref()))
        .collect();
    assert_eq!(
        links,
        [
            ("see", None),
            ("www.example.com/a", Some("http://www.example.com/a")),
            ("or", None),
            ("https://x.org/b", Some("https://x.org/b")),
            (".", None),
        ]
    );

    // Inside a link a bare URL is the link's text, not a link of its own.
    let runs = super::extension_tests::runs("[www.a.com](https://b.com)");
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].2.as_deref(), Some("https://b.com"));
}

#[test]
fn rust_only_emphasis_extras_are_bold_in_table_cells() {
    let items = parse_markdown("| a | b |\n|---|---|\n| x ==y== | z |");
    let Some(ParagraphItem::Table {
        rows, cell_styles, ..
    }) = items.first()
    else {
        panic!("expected a table");
    };
    assert_eq!(rows[1][0], "x y");
    let spans = &cell_styles[1][0];
    assert_eq!(spans.len(), 1);
    assert_eq!((spans[0].start, spans[0].end, spans[0].style), (2, 3, BOLD));
}

#[test]
fn rust_only_superscripts_with_spaces_render_bold() {
    // Markdig reads `x^a b^` as `x<sup>a b</sup>`.
    let runs = runs("x^a b^ y");
    let styled: Vec<(&str, MarkdownRunStyle)> = runs
        .iter()
        .map(|(text, style, _, _)| (text.as_str(), *style))
        .collect();
    assert_eq!(styled, [("x", PLAIN), ("a b", BOLD), ("y", PLAIN)]);
}

#[test]
fn rust_only_escaped_delimiters_stay_text() {
    // Markdig reads `\^a^` and `\==b==` as text: an escaped delimiter opens nothing.
    let runs = runs("\\^a^ \\==b== &#94;c^");
    assert!(
        runs.iter().all(|(_, style, _, _)| *style == PLAIN),
        "{:?}",
        runs.iter().map(|r| &r.0).collect::<Vec<_>>()
    );
}
