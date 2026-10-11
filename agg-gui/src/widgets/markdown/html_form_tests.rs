//! Rust-only tests: the viewer's HTML form (`html_form.rs`) writes the
//! Markdig extensions as Markdig writes them, so the rich editor's HTML judge
//! can tell a rewrite that loses one.  Expectations are Markdig's own HTML
//! for the same source (whitespace collapsed), where the two writers agree.

use super::markdown_html;

#[test]
fn rust_only_extensions_render_as_markdig_renders_them() {
    assert_eq!(
        markdown_html("x^a b^ ==m== ++i++ H~2~O ~~d~~ see www.x.com/a and https://y.org."),
        "<p>x<sup>a b</sup> <mark>m</mark> <ins>i</ins> H<sub>2</sub>O <del>d</del> see \
         <a href=\"http://www.x.com/a\">www.x.com/a</a> and \
         <a href=\"https://y.org\">https://y.org</a>.</p>"
    );
    // Escaped delimiters stay text, as in Markdig.
    assert_eq!(markdown_html("\\^a^ \\==b=="), "<p>^a^ ==b==</p>");
}

#[test]
fn rust_only_style_tags_normalise_as_the_csharp_judge_does() {
    assert_eq!(
        markdown_html("<b>a</b> <i>b</i> <s>c</s> <strike>d</strike>"),
        markdown_html("**a** *b* ~~c~~ ~~d~~")
    );
}

#[test]
fn rust_only_grid_tables_render_with_and_without_a_header() {
    // Markdig: a headerless grid table has no <thead>; its rows are <td> rows.
    let html = markdown_html("+---+---+\n| a | b |\n+---+---+\n");
    assert!(!html.contains("<thead>"), "{html}");
    assert!(html.contains("<td>a</td><td>b</td>"), "{html}");

    let html = markdown_html("+---+---+\n| a | b |\n+===+===+\n| c | d |\n+---+---+\n");
    assert!(
        html.contains("<thead><tr><th>a</th><th>b</th></tr></thead>"),
        "{html}"
    );
    assert!(html.contains("<td>c</td><td>d</td>"), "{html}");

    // A rewrite that drops the header separator renders differently.
    assert_ne!(
        markdown_html("+---+\n| a |\n+===+\n"),
        markdown_html("+---+\n| a |\n+---+\n")
    );
}
