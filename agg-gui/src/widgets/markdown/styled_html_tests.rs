//! Tests for the clipboard's styled HTML (`styled_html.rs`,
//! `html_rewriter.rs`).
//!
//! `styled_html_uses_striped_rows_bold_headers_and_no_header_background_fill`
//! ports agg-sharp's `MarkdownTableTests` test of that name; the rest are
//! Rust-only, pinning `CssColor`'s blend, the element styles, the C# header
//! alignment fix and the rewriter on malformed HTML.

use super::styled_html::{css_color, style_html_fragment, to_styled_html, HtmlStyleTheme};
use crate::color::Color;

/// The `style` attribute of the start tag at `at` in `html` ("" when none).
fn style_at(html: &str, at: usize) -> String {
    let tag_end = at + html[at..].find('>').expect("a closed tag");
    let tag = &html[at..tag_end];
    match tag.find("style=\"") {
        Some(start) => {
            let value = &tag[start + 7..];
            value[..value.find('"').expect("a closed value")].to_string()
        }
        None => String::new(),
    }
}

/// The start tags `<name` (followed by a space or `>`) in `html`, by offset.
fn tags(html: &str, name: &str) -> Vec<usize> {
    let open = format!("<{name}");
    html.match_indices(&open)
        .filter(|(at, _)| matches!(html.as_bytes().get(at + open.len()), Some(b' ' | b'>')))
        .map(|(at, _)| at)
        .collect()
}

#[test]
fn styled_html_uses_striped_rows_bold_headers_and_no_header_background_fill() {
    let html = to_styled_html(
        "| Left | Center | Right |\n| :--- | :---: | ---: |\n| A | B | C |\n| D | E | F |\n",
        &HtmlStyleTheme::default(),
    );

    let header_cell = tags(&html, "th");
    let body_start = html.find("<tbody>").expect("a table body");
    let body_end = html.find("</tbody>").expect("a closed table body");
    let body_rows: Vec<usize> = tags(&html, "tr")
        .into_iter()
        .filter(|at| (body_start..body_end).contains(at))
        .collect();

    assert!(!header_cell.is_empty());
    assert_eq!(body_rows.len(), 2);
    let header_style = style_at(&html, header_cell[0]);
    assert!(header_style.contains("font-weight:700"), "{header_style}");
    assert!(header_style.contains("text-align:left"), "{header_style}");
    assert!(!header_style.contains("background-color"), "{header_style}");
    assert!(style_at(&html, body_rows[1]).contains("background-color"));
}

#[test]
fn rust_only_aligned_header_keeps_its_alignment() {
    // C# appended its `text-align:left` default after the header's own
    // alignment (it looked for an `align` attribute Markdig never writes).
    let html = to_styled_html(
        "| Left | Center | Right |\n| :--- | :---: | ---: |\n| A | B | C |\n",
        &HtmlStyleTheme::default(),
    );
    let headers = tags(&html, "th");
    let center = style_at(&html, headers[1]);
    let right = style_at(&html, headers[2]);
    assert!(center.ends_with("text-align:center;"), "{center}");
    assert!(!center.contains("text-align:left"), "{center}");
    assert!(right.ends_with("text-align:right;"), "{right}");
    // A cell with an `align` attribute keeps it, as in C#.
    let cell = style_html_fragment(
        "<table><tr><th align=\"Right\">x</th></tr></table>",
        &HtmlStyleTheme::default(),
    );
    assert!(cell.contains("font-weight:700;text-align:right;"), "{cell}");
}

#[test]
fn rust_only_css_color_blends_alpha_over_the_background() {
    assert_eq!(
        css_color(Color::from_rgb8(0x33, 0x33, 0x33), None),
        "#333333"
    );
    assert_eq!(
        css_color(Color::from_rgb8(0x7A, 0xD7, 0xF0), Some(Color::black())),
        "#7AD7F0",
        "an opaque colour ignores the background"
    );
    // MinimalShade (#0000000F) over white: AGG's rgba8 lerp, 255 toward 0 by 15/255.
    assert_eq!(css_color(Color::from_rgba8(0, 0, 0, 0x0F), None), "#F0F0F0");
    assert_eq!(
        css_color(
            Color::from_rgba8(0xFF, 0xFF, 0xFF, 128),
            Some(Color::black())
        ),
        "#808080"
    );
    assert_eq!(
        css_color(
            Color::from_rgba8(0, 0, 0, 0),
            Some(Color::from_rgb8(1, 2, 3))
        ),
        "#010203",
        "a clear colour is the background"
    );
}

#[test]
fn rust_only_styled_html_styles_the_root_and_inline_elements() {
    assert_eq!(to_styled_html("", &HtmlStyleTheme::default()), "");
    assert_eq!(style_html_fragment(" \n", &HtmlStyleTheme::default()), "");

    let theme = HtmlStyleTheme {
        default_font_size: 12.0,
        ..HtmlStyleTheme::default()
    };
    let html = to_styled_html("A **b** *i* ~~s~~ [l](https://x.com) `c`\n", &theme);
    assert!(html.starts_with(
        "<div style=\"font-family:Arial, Helvetica, sans-serif;font-size:12pt;line-height:1.45;color:#333333;background-color:#FFFFFF;\"><p style=\"margin:0 0 12px 0;\">"
    ), "{html}");
    assert!(html.ends_with("</div>"), "{html}");
    assert!(
        html.contains("<strong style=\"font-weight:700;\">b</strong>"),
        "{html}"
    );
    assert!(
        html.contains("<em style=\"font-style:italic;\">i</em>"),
        "{html}"
    );
    assert!(
        html.contains("<del style=\"text-decoration:line-through;\">s</del>"),
        "{html}"
    );
    assert!(
        html.contains(
            "<a href=\"https://x.com\" style=\"color:#7AD7F0;text-decoration:underline;\">l</a>"
        ),
        "{html}"
    );
    assert!(html.contains(
        "<code style=\"font-family:Consolas, &quot;Liberation Mono&quot;, Menlo, monospace;background-color:#F0F0F0;padding:0 2px;\">c</code>"
    ), "{html}");

    let code = to_styled_html("```\nx < y\n```\n", &theme);
    assert!(
        code.contains("<pre style=\"margin:12px 0;padding:6px;background-color:#F0F0F0;"),
        "{code}"
    );
    assert!(
        code.contains("background-color:transparent;padding:0;\">x &lt; y\n</code>"),
        "{code}"
    );

    let blocks = to_styled_html("# H\n\n> q\n\n- a\n\n---\n", &theme);
    assert!(blocks.contains("<h1 style=\"margin:0 0 12px 0;font-size:20pt;font-weight:700;line-height:1.25;\">H</h1>"), "{blocks}");
    assert!(blocks.contains("<blockquote style=\"margin:12px 0;padding:0 0 0 12px;border-left:4px solid #7AD7F0;\">"), "{blocks}");
    assert!(
        blocks.contains("<ul style=\"margin:0 0 12px 0;padding-left:24px;\">"),
        "{blocks}"
    );
    assert!(
        blocks.contains("<li style=\"margin:0 0 3px 0;\">"),
        "{blocks}"
    );
    assert!(
        blocks.contains("<hr style=\"border:none;border-top:1px solid #F5F5F5;margin:12px 0;\" />")
            || blocks.contains(
                "<hr style=\"border:none;border-top:1px solid #F5F5F5;margin:12px 0;\"/>"
            ),
        "{blocks}"
    );
}

#[test]
fn rust_only_existing_style_gains_a_separator_and_malformed_html_stays_text() {
    let theme = HtmlStyleTheme::default();
    let html = style_html_fragment(
        "<p style='color: red'>x</p><p style=\"a:b; \">y</p>",
        &theme,
    );
    assert!(
        html.contains("<p style=\"color: red;margin:0 0 12px 0;\">x</p>"),
        "{html}"
    );
    assert!(
        html.contains("<p style=\"a:b; margin:0 0 12px 0;\">y</p>"),
        "{html}"
    );

    // A lone `<`, an unclosed tag and an unclosed quote are text; nothing is lost.
    for fragment in ["a < b", "x <b", "<a href=\"y>z", "</>", "<!-- c", "<p>open"] {
        let html = style_html_fragment(fragment, &theme);
        let inner = &html[html.find("\">").expect("root") + 2..html.len() - "</div>".len()];
        if fragment == "<p>open" {
            assert_eq!(inner, "<p style=\"margin:0 0 12px 0;\">open");
        } else {
            assert_eq!(inner, fragment);
        }
    }

    // Raw-text elements hold their content as text; an unopened end tag closes nothing.
    let html = style_html_fragment(
        "<script>if (a<b) {}</p></script></tr><tbody><tr></tr><tr></tr></tbody>",
        &theme,
    );
    assert!(
        html.contains("<script>if (a<b) {}</p></script></tr>"),
        "{html}"
    );
    assert!(
        html.ends_with(
            "<tbody><tr></tr><tr style=\"background-color:#ECECEC;\"></tr></tbody></div>"
        ),
        "{html}"
    );

    // Unclosed list items are siblings, so `code` inside `pre` still sees its parent.
    let html = style_html_fragment("<ul><li>a<li>b</ul><pre><code>x</code></pre>", &theme);
    assert_eq!(html.matches("<li style=").count(), 2);
    assert!(
        html.contains("background-color:transparent;padding:0;\">x</code>"),
        "{html}"
    );
}
