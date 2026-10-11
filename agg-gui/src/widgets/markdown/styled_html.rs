//! Markdown as self-styled HTML, for the clipboard — a port of agg-sharp's
//! `AggMarkdownDocument.ToStyledHtml` / `StyleHtmlFragment` / `CssColor`
//! (`MarkdigAgg/AggMarkdownDocument.cs`).
//!
//! A paste target (mail, a document) styles bare HTML its own way, so a copy
//! carries its look inline: the fragment is wrapped in a `<div>` with the
//! body font, size and colours, and each element gets the CSS the viewer
//! draws it with (heading sizes, list indents, quote bars, code shading,
//! table borders and stripes, link colour).  Colours with alpha are blended
//! over the theme background ([`css_color`]), since not every paste target
//! reads `rgba()`.
//!
//! [`to_styled_html`] styles the viewer's own HTML of the markdown
//! (`html_form.rs`, what C# gets from Markdig's HTML renderer);
//! `html_rewriter.rs` walks the HTML and appends the CSS chosen here.
//!
//! Fixed from C#: a table cell's alignment was read from an `align`
//! attribute, which Markdig never writes (it writes
//! `style="text-align: ..."`), and a header cell then fell back to
//! `text-align:left` appended after its own alignment, so a centred or
//! right-aligned column's header pasted left-aligned.  Here the cell's own
//! `text-align` is read when there is no `align` attribute
//! (`rust_only_aligned_header_keeps_its_alignment`).

use super::html_form::raw_html;
use super::html_rewriter::{rewrite_start_tags, StartTag};
use crate::color::Color;

/// C# `TableBorderAlpha`: the text colour's alpha for table borders.
const TABLE_BORDER_ALPHA: u8 = 150;
/// C# `ZebraStripeAlpha`: the text colour's alpha for striped body rows.
const ZEBRA_STRIPE_ALPHA: u8 = 24;

const MONO_FONTS: &str = "Consolas, \"Liberation Mono\", Menlo, monospace";

/// The parts of C#'s `ThemeConfig` styled HTML reads.  [`Default`] is a
/// fresh C# `new ThemeConfig()`: light, 11 pt, which is what the rich editor
/// copies with, so a copy from a dark theme pastes readable on white.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HtmlStyleTheme {
    /// Body text size in points (C# `DefaultFontSize`).
    pub default_font_size: f64,
    /// C# `TextColor`.
    pub text_color: Color,
    /// C# `BackgroundColor`.
    pub background_color: Color,
    /// Links and quote bars (C# `PrimaryAccentColor`).
    pub primary_accent_color: Color,
    /// Code shading (C# `MinimalShade`).
    pub minimal_shade: Color,
    /// Rules (C# `TabBarBackground`).
    pub tab_bar_background: Color,
}

impl Default for HtmlStyleTheme {
    fn default() -> Self {
        Self {
            default_font_size: 11.0,
            text_color: Color::from_rgb8(0x33, 0x33, 0x33),
            background_color: Color::from_rgb8(0xFF, 0xFF, 0xFF),
            primary_accent_color: Color::from_rgb8(0x7A, 0xD7, 0xF0),
            minimal_shade: Color::from_rgba8(0, 0, 0, 0x0F),
            tab_bar_background: Color::from_rgb8(0xF5, 0xF5, 0xF5),
        }
    }
}

/// `markdown` as HTML styled for the clipboard (C# `ToStyledHtml`); empty
/// for empty markdown.
pub fn to_styled_html(markdown: &str, theme: &HtmlStyleTheme) -> String {
    if markdown.is_empty() {
        return String::new();
    }
    style_html_fragment(&raw_html(markdown), theme)
}

/// `html_fragment` wrapped in a `<div>` with the theme's body style, every
/// element given its inline CSS (C# `StyleHtmlFragment`); empty for a blank
/// fragment.
pub fn style_html_fragment(html_fragment: &str, theme: &HtmlStyleTheme) -> String {
    if html_fragment.trim().is_empty() {
        return String::new();
    }
    let background = theme.background_color;
    let on_background = |color: Color| css_color(color, Some(background));
    let root_style = format!(
        "font-family:Arial, Helvetica, sans-serif;font-size:{}pt;line-height:1.45;color:{};background-color:{};",
        theme.default_font_size,
        on_background(theme.text_color),
        css_color(background, None),
    );
    let accent = on_background(theme.primary_accent_color);
    let shade = on_background(theme.minimal_shade);
    let rule = on_background(theme.tab_bar_background);
    let border = on_background(with_alpha8(theme.text_color, TABLE_BORDER_ALPHA));
    let stripe = on_background(with_alpha8(theme.text_color, ZEBRA_STRIPE_ALPHA));

    let body = rewrite_start_tags(html_fragment, "div", |tag| {
        element_style(tag, &accent, &shade, &rule, &border, &stripe)
    });
    let mut out = String::with_capacity(body.len() + root_style.len() + 24);
    out.push_str("<div style=\"");
    out.push_str(&root_style.replace('"', "&quot;"));
    out.push_str("\">");
    out.push_str(&body);
    out.push_str("</div>");
    out
}

/// The CSS one element gets, `None` for elements left as they are.
fn element_style(
    tag: &StartTag<'_>,
    accent: &str,
    shade: &str,
    rule: &str,
    border: &str,
    stripe: &str,
) -> Option<String> {
    let css = match tag.name.as_str() {
        "h1" => "margin:0 0 12px 0;font-size:20pt;font-weight:700;line-height:1.25;".into(),
        "h2" => "margin:0 0 12px 0;font-size:17pt;font-weight:700;line-height:1.3;".into(),
        "h3" => "margin:0 0 12px 0;font-size:15pt;font-weight:700;line-height:1.3;".into(),
        "h4" => "margin:0 0 12px 0;font-size:13pt;font-weight:700;line-height:1.35;".into(),
        "h5" => "margin:0 0 12px 0;font-size:12pt;font-weight:700;line-height:1.35;".into(),
        "h6" => "margin:0 0 12px 0;font-size:11pt;font-weight:700;line-height:1.35;".into(),
        "p" => "margin:0 0 12px 0;".into(),
        "ul" | "ol" => "margin:0 0 12px 0;padding-left:24px;".into(),
        "li" => "margin:0 0 3px 0;".into(),
        "blockquote" => format!("margin:12px 0;padding:0 0 0 12px;border-left:4px solid {accent};"),
        "pre" => format!(
            "margin:12px 0;padding:6px;background-color:{shade};font-family:{MONO_FONTS};font-size:10pt;white-space:pre-wrap;"
        ),
        "code" if tag.parent == Some("pre") => format!(
            "font-family:{MONO_FONTS};font-size:10pt;background-color:transparent;padding:0;"
        ),
        "code" => format!("font-family:{MONO_FONTS};background-color:{shade};padding:0 2px;"),
        "hr" => format!("border:none;border-top:1px solid {rule};margin:12px 0;"),
        "table" => "margin:12px 0;border-collapse:collapse;border-spacing:0;".into(),
        // C# IsStripedBodyRow: every second row of a tbody.
        "tr" if tag.parent == Some("tbody") && tag.index_among_same_name % 2 == 1 => {
            format!("background-color:{stripe};")
        }
        "th" => format!(
            "padding:6px 8px;border:1px solid {border};font-weight:700;{}",
            table_cell_text_align(tag, Some("left"))
        ),
        "td" => format!(
            "padding:6px 8px;border:1px solid {border};{}",
            table_cell_text_align(tag, None)
        ),
        "a" => format!("color:{accent};text-decoration:underline;"),
        "img" => "max-width:100%;height:auto;".into(),
        "strong" => "font-weight:700;".into(),
        "em" => "font-style:italic;".into(),
        "del" => "text-decoration:line-through;".into(),
        _ => return None,
    };
    Some(css)
}

/// C# `TableCellTextAlign`: `text-align:<align>;` from the cell's `align`
/// attribute, else (fixed from C#, see the module doc) the `text-align` of
/// its own style, else `default_align`; empty when none of these says.
fn table_cell_text_align(tag: &StartTag<'_>, default_align: Option<&str>) -> String {
    let align = tag
        .attribute("align")
        .map(str::to_string)
        .or_else(|| tag.attribute("style").and_then(style_text_align))
        .or_else(|| default_align.map(str::to_string));
    match align {
        Some(align) if !align.trim().is_empty() => {
            format!("text-align:{};", align.trim().to_lowercase())
        }
        _ => String::new(),
    }
}

/// The value of the last `text-align` declaration in a `style` attribute.
fn style_text_align(style: &str) -> Option<String> {
    style
        .split(';')
        .filter_map(|declaration| declaration.split_once(':'))
        .filter(|(property, _)| property.trim().eq_ignore_ascii_case("text-align"))
        .map(|(_, value)| value.trim().to_string())
        .next_back()
}

/// C# `new Color(color, alpha)`: the colour with its alpha replaced.
fn with_alpha8(color: Color, alpha: u8) -> Color {
    color.with_alpha(f32::from(alpha) / 255.0)
}

/// A colour channel as the byte C#'s 8-bit `Color` holds.
fn channel8(value: f32) -> u8 {
    // Clamped first, so the cast never saturates.
    (value * 255.0).round().clamp(0.0, 255.0) as u8
}

/// C# `Rgba8Math.Lerp` (C++ AGG `rgba8::lerp`): `p` moved toward `q` by
/// `a`/255, rounded.
fn lerp8(p: u8, q: u8, a: u8) -> u8 {
    let (p, q, a) = (i32::from(p), i32::from(q), i32::from(a));
    let t = (q - p) * a + 128 - i32::from(p > q);
    // Always within 0..=255: the step never passes `q`.
    (p + (((t >> 8) + t) >> 8)) as u8
}

/// `color` as an opaque CSS hex colour, `#RRGGBB` (C# `CssColor`).  A colour
/// with alpha is blended over `background` (white when `None`) the way C#'s
/// `ThemeConfig.ResolveColor2` does (its `BlenderBGRA`).
pub fn css_color(color: Color, background: Option<Color>) -> String {
    let background = background.unwrap_or_else(Color::white);
    let [r, g, b, a] = [color.r, color.g, color.b, color.a].map(channel8);
    let (r, g, b) = if a < 255 {
        let [br, bg, bb] = [background.r, background.g, background.b].map(channel8);
        (lerp8(br, r, a), lerp8(bg, g, a), lerp8(bb, b, a))
    } else {
        (r, g, b)
    };
    format!("#{r:02X}{g:02X}{b:02X}")
}
