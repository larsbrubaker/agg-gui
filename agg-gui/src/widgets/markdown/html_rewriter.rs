//! A small HTML start-tag rewriter: walks an HTML fragment, keeps every byte
//! of it except the start tags it is asked to restyle, and appends inline CSS
//! to those.  It stands in for the HtmlAgilityPack parse-and-serialise that
//! agg-sharp's `AggMarkdownDocument.StyleHtmlFragment` does
//! (`MarkdigAgg/AggMarkdownDocument.cs`); `styled_html.rs` holds the rules
//! that decide each element's CSS.
//!
//! Each start tag is handed to the caller with what those rules read: its
//! lower-case name, its parent element's name, how many element siblings of
//! the same name come before it, and its attributes.  The element stack is
//! kept the way an HTML reader keeps it: void elements (`<img>`, `<br>`,
//! `<hr>`, ...) and self-closed tags are never opened, an end tag closes back
//! to its matching open element (one with no open match is left as text),
//! the raw-text elements (`<script>`, `<style>`, `<textarea>`, `<title>`)
//! hold their content as text, and a `<p>`, `<li>`, `<tr>`, `<td>` or `<th>`
//! closes an open one of its kind first, as unclosed ones in pasted HTML
//! would be.  Comments, doctypes and processing instructions pass through.
//! A `<` that starts no tag, or a tag with no closing `>`, is text.
//!
//! A restyled attribute is written double-quoted, with `"` as `&quot;` (the
//! CSS this module appends quotes font names); every other attribute keeps
//! its original text.

/// What the styling rules see of one start tag.
pub(super) struct StartTag<'a> {
    /// The element name, lower case.
    pub name: String,
    /// The parent element's name, lower case (`None` at the top level).
    pub parent: Option<&'a str>,
    /// How many element siblings with this name come before it.
    pub index_among_same_name: usize,
    attributes: &'a [Attribute<'a>],
}

impl StartTag<'_> {
    /// An attribute's value as written (entities not decoded), `None` when
    /// the tag has no such attribute.  An attribute without a value reads as
    /// empty.  Names compare case-insensitively.
    pub fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|a| a.name.eq_ignore_ascii_case(name))
            .map(|a| a.value.unwrap_or(""))
    }
}

struct Attribute<'a> {
    name: &'a str,
    value: Option<&'a str>,
    /// The attribute's whole text, name to closing quote.
    raw: &'a str,
}

/// One open element.
struct Open {
    name: String,
    /// Element children seen so far, by name, for `index_among_same_name`.
    child_counts: Vec<(String, usize)>,
}

impl Open {
    fn new(name: String) -> Self {
        Self {
            name,
            child_counts: Vec::new(),
        }
    }

    /// Counts a child named `name`, returning how many came before it.
    fn count_child(&mut self, name: &str) -> usize {
        match self.child_counts.iter_mut().find(|(n, _)| n == name) {
            Some((_, count)) => {
                *count += 1;
                *count - 1
            }
            None => {
                self.child_counts.push((name.to_string(), 1));
                0
            }
        }
    }
}

const VOID_ELEMENTS: [&str; 14] = [
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];
const RAW_TEXT_ELEMENTS: [&str; 4] = ["script", "style", "textarea", "title"];
const SELF_CLOSING_SIBLINGS: [&str; 5] = ["p", "li", "tr", "td", "th"];

/// `html` with `style_for`'s CSS appended to the `style` attribute of each
/// start tag it returns CSS for (a tag without one gains it, last).  The
/// fragment's top-level elements see `root` as their parent.
pub(super) fn rewrite_start_tags(
    html: &str,
    root: &str,
    mut style_for: impl FnMut(&StartTag<'_>) -> Option<String>,
) -> String {
    let bytes = html.as_bytes();
    let mut out = String::with_capacity(html.len() * 2);
    let mut stack = vec![Open::new(root.to_ascii_lowercase())];
    let mut text_start = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        let rest = &html[i..];
        let end = if let Some(comment) = rest.strip_prefix("<!--") {
            comment.find("-->").map(|e| i + 4 + e + 3)
        } else if rest.starts_with("<!") || rest.starts_with("<?") {
            rest.find('>').map(|e| i + e + 1)
        } else if rest.starts_with("</") {
            end_tag(rest).map(|(name, len)| {
                close(&mut stack, &name);
                i + len
            })
        } else if rest.len() > 1 && bytes[i + 1].is_ascii_alphabetic() {
            match parse_start_tag(rest) {
                Some(tag) => {
                    out.push_str(&html[text_start..i]);
                    let len = tag.len;
                    let raw_end = write_start_tag(&tag, &mut stack, &mut style_for, &mut out, rest);
                    text_start = i + len + raw_end;
                    i = text_start;
                    continue;
                }
                None => None,
            }
        } else {
            None
        };
        match end {
            Some(end) => {
                out.push_str(&html[text_start..end]);
                text_start = end;
                i = end;
            }
            None => i += 1,
        }
    }
    out.push_str(&html[text_start..]);
    out
}

/// A parsed start tag: `<name attrs... >` or `/>`.
struct ParsedTag<'a> {
    name: &'a str,
    attributes: Vec<Attribute<'a>>,
    self_closed: bool,
    /// The whole tag's length in bytes.
    len: usize,
}

/// Writes one start tag, restyled when `style_for` asks, and updates the
/// stack.  Returns the length of a raw-text element's content (and its end
/// tag), which is copied as is, or 0.
fn write_start_tag(
    tag: &ParsedTag<'_>,
    stack: &mut Vec<Open>,
    style_for: &mut impl FnMut(&StartTag<'_>) -> Option<String>,
    out: &mut String,
    rest: &str,
) -> usize {
    let name = tag.name.to_ascii_lowercase();
    if SELF_CLOSING_SIBLINGS.contains(&name.as_str()) {
        let closes_open = match name.as_str() {
            "td" | "th" => stack
                .last()
                .is_some_and(|o| o.name == "td" || o.name == "th"),
            _ => stack.last().is_some_and(|o| o.name == name),
        };
        if closes_open && stack.len() > 1 {
            stack.pop();
        }
    }
    let Some(parent) = stack.last_mut() else {
        return 0;
    };
    let index_among_same_name = parent.count_child(&name);
    let style = {
        let context = StartTag {
            name: name.clone(),
            parent: Some(parent.name.as_str()),
            index_among_same_name,
            attributes: &tag.attributes,
        };
        style_for(&context)
    };

    out.push('<');
    out.push_str(tag.name);
    let mut styled = false;
    for attribute in &tag.attributes {
        out.push(' ');
        match &style {
            Some(css) if !styled && attribute.name.eq_ignore_ascii_case("style") => {
                styled = true;
                push_attribute(
                    out,
                    attribute.name,
                    &append_style(attribute.value.unwrap_or(""), css),
                );
            }
            _ => out.push_str(attribute.raw),
        }
    }
    if let (Some(css), false) = (&style, styled) {
        out.push(' ');
        push_attribute(out, "style", &append_style("", css));
    }
    out.push_str(if tag.self_closed { "/>" } else { ">" });

    let after = &rest[tag.len..];
    if RAW_TEXT_ELEMENTS.contains(&name.as_str()) && !tag.self_closed {
        // The content up to (and with) the matching end tag is text.
        let lower = after.to_ascii_lowercase();
        let close_tag = format!("</{name}");
        let raw_len = match lower.find(&close_tag) {
            Some(at) => after[at..].find('>').map_or(after.len(), |e| at + e + 1),
            None => after.len(),
        };
        out.push_str(&after[..raw_len]);
        return raw_len;
    }
    if !tag.self_closed && !VOID_ELEMENTS.contains(&name.as_str()) {
        stack.push(Open::new(name));
    }
    0
}

/// C# `AppendStyle`: `existing`, with a `;` added when it has text that does
/// not end in one, then `css`.
fn append_style(existing: &str, css: &str) -> String {
    if css.trim().is_empty() {
        return existing.to_string();
    }
    let mut value = existing.to_string();
    if !value.trim().is_empty() && !value.trim_end().ends_with(';') {
        value.push(';');
    }
    value.push_str(css);
    value
}

fn push_attribute(out: &mut String, name: &str, value: &str) {
    out.push_str(name);
    out.push_str("=\"");
    out.push_str(&value.replace('"', "&quot;"));
    out.push('"');
}

/// Closes back to the innermost open element named `name`; an end tag with
/// no open match closes nothing (the root never closes).
fn close(stack: &mut Vec<Open>, name: &str) {
    if let Some(at) = stack.iter().skip(1).rposition(|o| o.name == name) {
        stack.truncate(at + 1);
    }
}

/// An end tag `</name ...>`: its lower-case name and length.
fn end_tag(rest: &str) -> Option<(String, usize)> {
    let name_len = rest[2..]
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == ':'))
        .unwrap_or(rest.len() - 2);
    if name_len == 0 {
        return None;
    }
    let close = rest.find('>')?;
    Some((rest[2..2 + name_len].to_ascii_lowercase(), close + 1))
}

/// Parses the start tag `rest` begins with, `None` when it is not one (no
/// closing `>`).
fn parse_start_tag(rest: &str) -> Option<ParsedTag<'_>> {
    let bytes = rest.as_bytes();
    let name_end = rest[1..]
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == ':'))
        .map_or(rest.len(), |e| e + 1);
    let name = &rest[1..name_end];
    let mut attributes = Vec::new();
    let mut i = name_end;
    loop {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        match bytes.get(i)? {
            b'>' => {
                return Some(ParsedTag {
                    name,
                    attributes,
                    self_closed: false,
                    len: i + 1,
                })
            }
            b'/' if bytes.get(i + 1) == Some(&b'>') => {
                return Some(ParsedTag {
                    name,
                    attributes,
                    self_closed: true,
                    len: i + 2,
                })
            }
            b'/' => {
                i += 1;
                continue;
            }
            _ => {}
        }
        let start = i;
        while i < bytes.len()
            && !bytes[i].is_ascii_whitespace()
            && !matches!(bytes[i], b'=' | b'>' | b'/')
        {
            i += 1;
        }
        if i == start {
            // A stray `=`: skip it.
            i += 1;
            continue;
        }
        let attr_name = &rest[start..i];
        let mut j = i;
        while j < bytes.len() && bytes[j].is_ascii_whitespace() {
            j += 1;
        }
        let mut value = None;
        if bytes.get(j) == Some(&b'=') {
            j += 1;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            match bytes.get(j)? {
                quote @ (b'"' | b'\'') => {
                    let close = rest[j + 1..].find(*quote as char)? + j + 1;
                    value = Some(&rest[j + 1..close]);
                    j = close + 1;
                }
                _ => {
                    let value_start = j;
                    while j < bytes.len() && !bytes[j].is_ascii_whitespace() && bytes[j] != b'>' {
                        j += 1;
                    }
                    value = Some(&rest[value_start..j]);
                }
            }
            i = j;
        }
        attributes.push(Attribute {
            name: attr_name,
            value,
            raw: &rest[start..i],
        });
    }
}
