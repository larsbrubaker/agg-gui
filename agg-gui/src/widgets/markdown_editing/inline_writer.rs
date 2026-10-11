//! Writes a list of inlines (a paragraph's, a heading's, a table cell's) as
//! markdown that reads back to the same inlines.  Ports agg-sharp's
//! `RichInlineWriter.cs`.
//!
//! Styles become the shortest properly nested delimiters, plain text is
//! escaped ([`super::escaping`]), atoms are written verbatim inside their own
//! style.  Every edited text block goes through here
//! ([`super::markdown_writer`], [`super::group_writer`]), so a rewrite never
//! changes what renders.
//!
//! The write ends with a read-back check (C#'s `ReadsBackAs`): it parses the
//! result with [`super::markdown_parser::parse`] and, when the styles differ
//! (cases the flanking pass below does not model: the rule of three, runs that
//! both open and close), writes every span as HTML tags.  Where the parse
//! reads other characters, agg-sharp trusts the flanking pass; here the tags
//! are used when they read back exactly, because pulldown-cmark applies
//! CommonMark's rule of three to a delimiter run as written where Markdig
//! applies it to what is left of the run (see `write_counted`).

use super::char_class::is_punctuation_or_symbol;
use super::escaping;
use super::inline::{InlineAtomKind, RichInline, RichInlineContent};
use super::inlines;
use super::markdown_parser;

/// Where a list of inlines is written, which changes what its markdown may
/// contain.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RichInlineContext {
    /// A paragraph, list item or quote line.
    #[default]
    Paragraph,
    /// A heading is one line, so a hard break writes as a space.
    Heading,
    /// A pipe-table cell: one line like a heading, and a '|' in plain text is
    /// escaped so it does not end the cell.
    TableCell,
}

/// Declaration order breaks ties when spans open together: earlier kinds go
/// outside.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum SpanKind {
    Link,
    Strike,
    Bold,
    Italic,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TokenKind {
    Open,
    Close,
    Text,
    Code,
    Atom,
}

/// A span a delimiter pair opens and closes.  Links are spans too, so emphasis
/// and links nest properly around each other; a link's identity is its url,
/// title and label together.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Span {
    kind: SpanKind,
    link_url: Option<String>,
    link_title: Option<String>,
    link_label: Option<String>,
}

impl Span {
    fn style(kind: SpanKind) -> Self {
        Span {
            kind,
            link_url: None,
            link_title: None,
            link_label: None,
        }
    }
}

/// One token of the written line.  `use_id` indexes the opened span's shared
/// "write as HTML" flag (C#'s `SpanUse`), the same for its Open and Close so
/// both write the same form; links have none.
#[derive(Clone, Debug)]
struct Token {
    kind: TokenKind,
    span: Option<Span>,
    text: String,
    use_id: Option<usize>,
}

impl Token {
    fn content(kind: TokenKind, text: String) -> Self {
        Token {
            kind,
            span: None,
            text,
            use_id: None,
        }
    }

    fn delimiter(kind: TokenKind, span: Span, use_id: Option<usize>) -> Self {
        Token {
            kind,
            span: Some(span),
            text: String::new(),
            use_id,
        }
    }

    fn is_emphasis(&self) -> bool {
        self.span
            .as_ref()
            .is_some_and(|span| span.kind != SpanKind::Link)
    }
}

/// The markdown for the inlines.
pub fn write(inlines: &[RichInline], context: RichInlineContext) -> String {
    let mut markdown = String::new();
    write_into(inlines, &mut markdown, context);
    markdown
}

/// Appends the markdown for the inlines to `markdown`, which holds whatever
/// precedes them on the line (or nothing): line-start escaping is decided from
/// it.
pub fn write_into(inlines: &[RichInline], markdown: &mut String, context: RichInlineContext) {
    write_counted(inlines, markdown, context);
}

/// [`write_into`], also reporting how many tokens the whitespace hoisting
/// started with and how many steps its scan took, so a test can hold the scan
/// linear by count rather than by the clock.  Returns `(token_count,
/// hoist_steps)`.
pub fn write_counted(
    inlines: &[RichInline],
    markdown: &mut String,
    context: RichInlineContext,
) -> (usize, usize) {
    let written = prepare(inlines, context);
    let segments: Vec<(RichInline, Vec<Span>)> = written
        .iter()
        .map(|inline| (inline.clone(), spans_of(inline)))
        .collect();
    let (mut tokens, mut uses) = tokenize(&segments);
    let token_count = tokens.len();
    let hoist_steps = hoist_whitespace(&mut tokens);

    let prefix = markdown.clone();
    let table_cell = context == RichInlineContext::TableCell;

    // Each pass turns the spans whose delimiters would not read as emphasis into HTML tags; a tag
    // changes its neighbours' flanking, so repeat until nothing changes (each pass converts at
    // least one span).
    let (mut output, mut ranges) = serialize(&tokens, &uses, &prefix, table_cell);
    while mark_unflanked(&tokens, &mut uses, &ranges, &output) {
        (output, ranges) = serialize(&tokens, &uses, &prefix, table_cell);
    }

    // Flanking is not the whole of CommonMark's delimiter matching (the rule of three, runs that
    // both open and close); if the result still reads back with different styles, every span
    // uses tags.
    match read_back(&output[prefix.len()..], &written) {
        ReadBack::Same => {}
        ReadBack::StylesDiffer => {
            uses.iter_mut().for_each(|flag| *flag = true);
            (output, _) = serialize(&tokens, &uses, &prefix, table_cell);
        }
        ReadBack::TextDiffers => {
            // agg-sharp trusts the flanking pass here, because Markdig applies the rule of three
            // to the delimiters a run has left; pulldown-cmark (like the CommonMark reference)
            // applies it to the run as written, so `**a***a**a***` leaves `**` as literal text.
            // The tags win when they read back exactly.
            let mut tagged = uses.clone();
            tagged.iter_mut().for_each(|flag| *flag = true);
            let (html, _) = serialize(&tokens, &tagged, &prefix, table_cell);
            if read_back(&html[prefix.len()..], &written) == ReadBack::Same {
                output = html;
            }
        }
    }

    markdown.push_str(&output[prefix.len()..]);
    (token_count, hoist_steps)
}

/// The inlines as they will be written: merged so adjacent same-style runs
/// never write as `**a****b**`, and with a heading's or table cell's hard
/// breaks made spaces.
fn prepare(source: &[RichInline], context: RichInlineContext) -> Vec<RichInline> {
    let is_hard_break = |inline: &RichInline| {
        inline
            .as_atom()
            .is_some_and(|atom| atom.kind == InlineAtomKind::HardBreak)
    };
    let mut inlines = source.to_vec();
    if context != RichInlineContext::Paragraph {
        for inline in inlines.iter_mut() {
            if is_hard_break(inline) {
                // The atom's style (bold, italic, strike, link) carries over to the space.
                inline.content = RichInlineContent::Run(super::inline::RichRun {
                    text: " ".to_string(),
                    code: false,
                });
            }
        }
    }

    // A hard break with nothing after it (Shift+Enter at a paragraph's end, not yet typed past)
    // means nothing in markdown: written, its backslash would read back as literal text.
    while inlines.last().is_some_and(is_hard_break) {
        inlines.pop();
    }

    inlines::merge_adjacent(&mut inlines);
    inlines
}

fn spans_of(inline: &RichInline) -> Vec<Span> {
    let mut spans = Vec::new();
    if inline.link_url.is_some() || inline.link_label.is_some() {
        spans.push(Span {
            kind: SpanKind::Link,
            link_url: inline.link_url.clone(),
            link_title: inline.link_title.clone(),
            link_label: inline.link_label.clone(),
        });
    }

    if inline.strike {
        spans.push(Span::style(SpanKind::Strike));
    }

    if inline.bold {
        spans.push(Span::style(SpanKind::Bold));
    }

    if inline.italic {
        spans.push(Span::style(SpanKind::Italic));
    }

    spans
}

/// Opens and closes spans with a stack so delimiters always nest.  A span that
/// ends while spans opened inside it continue forces those to close and
/// reopen; opening the longest-lasting span outermost keeps that rare.
/// Returns the tokens and each opened emphasis span's HTML flag.
fn tokenize(segments: &[(RichInline, Vec<Span>)]) -> (Vec<Token>, Vec<bool>) {
    let mut tokens = Vec::new();
    let mut uses = Vec::new();
    let mut stack: Vec<(Span, Option<usize>)> = Vec::new();
    for (i, (inline, wanted)) in segments.iter().enumerate() {
        // Close down to the deepest span this segment is not in; spans above it that continue
        // reopen below.
        if let Some(deepest) = stack.iter().position(|(span, _)| !wanted.contains(span)) {
            for (span, use_id) in stack[deepest..].iter().rev() {
                tokens.push(Token::delimiter(TokenKind::Close, span.clone(), *use_id));
            }

            stack.truncate(deepest);
        }

        let mut to_open: Vec<&Span> = wanted
            .iter()
            .filter(|span| !stack.iter().any(|(open, _)| open == *span))
            .collect();
        // A stable sort, as LINQ's OrderByDescending(...).ThenBy(...).
        to_open.sort_by(|a, b| {
            persistence(segments, i, b)
                .cmp(&persistence(segments, i, a))
                .then(a.kind.cmp(&b.kind))
        });
        for span in to_open {
            let use_id = (span.kind != SpanKind::Link).then(|| {
                uses.push(false);
                uses.len() - 1
            });
            tokens.push(Token::delimiter(TokenKind::Open, span.clone(), use_id));
            stack.push((span.clone(), use_id));
        }

        tokens.push(match &inline.content {
            RichInlineContent::Run(run) if run.code => {
                Token::content(TokenKind::Code, run.text.clone())
            }
            RichInlineContent::Run(run) => Token::content(TokenKind::Text, run.text.clone()),
            RichInlineContent::Atom(atom) => {
                Token::content(TokenKind::Atom, atom.raw_markdown.clone())
            }
        });
    }

    for (span, use_id) in stack.into_iter().rev() {
        tokens.push(Token::delimiter(TokenKind::Close, span, use_id));
    }

    (tokens, uses)
}

fn persistence(segments: &[(RichInline, Vec<Span>)], start: usize, span: &Span) -> usize {
    let mut end = start;
    while end < segments.len() && segments[end].1.contains(span) {
        end += 1;
    }

    end - start
}

/// CommonMark only reads a delimiter as emphasis when it hugs non-space text
/// (`** a**` is literal), so whitespace at a styled span's edge moves outside
/// its delimiters.  Link brackets have no such rule, and moving a space across
/// one would change the link's text, so whitespace stops at a link.  Returns
/// how many steps the scan took.
fn hoist_whitespace(tokens: &mut Vec<Token>) -> usize {
    // Each change only alters what the rules see at its own token and the one before it (a
    // removal or a hoisted space can newly expose an empty pair, an empty text or an edge space
    // to the token before), so the scan steps back one token and carries on: a linear scan with
    // no rescans from the start, and the same rules fire in the same order. (Each list insert or
    // removal still shifts the tokens after it.)
    let mut steps = 0;
    let mut i = 0;
    while i < tokens.len() {
        steps += 1;
        let mut changed = false;
        let kind = tokens[i].kind;
        let emphasis = tokens[i].is_emphasis();
        let next_kind = tokens.get(i + 1).map(|token| token.kind);
        if kind == TokenKind::Text && tokens[i].text.is_empty() {
            tokens.remove(i);
            changed = true;
        } else if kind == TokenKind::Open
            && next_kind == Some(TokenKind::Close)
            && tokens[i + 1].span == tokens[i].span
        {
            // A pair left around nothing (all its text was whitespace) disappears.
            tokens.drain(i..i + 2);
            changed = true;
        } else if emphasis && kind == TokenKind::Open && next_kind == Some(TokenKind::Text) {
            let next = &tokens[i + 1].text;
            let lead = next.len() - next.trim_start().len();
            if lead > 0 {
                let hoisted = next[..lead].to_string();
                tokens[i + 1].text.drain(..lead);
                tokens.insert(i, Token::content(TokenKind::Text, hoisted));
                changed = true;
            }
        } else if emphasis
            && kind == TokenKind::Close
            && i > 0
            && tokens[i - 1].kind == TokenKind::Text
        {
            let previous = &tokens[i - 1].text;
            let trimmed = previous.trim_end().len();
            if trimmed < previous.len() {
                let hoisted = previous[trimmed..].to_string();
                tokens[i - 1].text.truncate(trimmed);
                tokens.insert(i + 1, Token::content(TokenKind::Text, hoisted));
                changed = true;
            }
        }

        i = if changed { i.saturating_sub(1) } else { i + 1 };
    }

    steps
}

/// The markdown for the tokens after `prefix`, with where each token landed
/// (byte ranges into the returned string, which starts with `prefix`).
fn serialize(
    tokens: &[Token],
    uses: &[bool],
    prefix: &str,
    table_cell: bool,
) -> (String, Vec<(usize, usize)>) {
    let mut markdown = String::from(prefix);
    let mut ranges: Vec<(usize, usize)> = Vec::with_capacity(tokens.len());
    for token in tokens {
        let mut start = markdown.len();
        let span = token.span.as_ref();
        match (token.kind, span) {
            (TokenKind::Open, Some(span)) if span.kind == SpanKind::Link => {
                // "![" would turn the link into an image.
                if ends_with_unescaped(&markdown, '!') && markdown.len() > prefix.len() {
                    markdown.insert(markdown.len() - 1, '\\');
                    if let Some(last) = ranges.last_mut() {
                        if last.1 == start {
                            last.1 = start + 1;
                        }
                    }

                    start += 1;
                }

                markdown.push('[');
            }
            (TokenKind::Close, Some(span)) if span.kind == SpanKind::Link => {
                markdown.push_str(&escaping::link_tail(
                    span.link_url.as_deref(),
                    span.link_title.as_deref(),
                    span.link_label.as_deref(),
                ));
            }
            (TokenKind::Open | TokenKind::Close, Some(span)) => {
                let html = token.use_id.is_some_and(|id| uses[id]);
                markdown.push_str(&delimiter(span.kind, html, token.kind == TokenKind::Close));
            }
            (TokenKind::Text, _) => {
                escaping::append_escaped(&token.text, &mut markdown, table_cell)
            }
            (TokenKind::Code, _) => escaping::append_code(&token.text, &mut markdown),
            (TokenKind::Atom, _) => markdown.push_str(&token.text),
            // Open and Close tokens always carry their span.
            (TokenKind::Open | TokenKind::Close, None) => {}
        }

        ranges.push((start, markdown.len()));
    }

    (markdown, ranges)
}

/// '*' rather than '_' because only '*' makes emphasis inside a word (a*b*c).
/// The tags are the fallback for spans whose delimiters would not read as
/// emphasis; the parser reads them back as the same styles.
fn delimiter(kind: SpanKind, html: bool, close: bool) -> String {
    if !html {
        return match kind {
            SpanKind::Bold => "**",
            SpanKind::Italic => "*",
            _ => "~~",
        }
        .to_string();
    }

    let tag = match kind {
        SpanKind::Bold => "strong",
        SpanKind::Italic => "em",
        _ => "del",
    };
    if close {
        format!("</{tag}>")
    } else {
        format!("<{tag}>")
    }
}

/// Applies CommonMark's flanking rules to every delimiter run (adjacent
/// delimiters of one character read as one run): an opener must be
/// left-flanking and a closer right-flanking, or the delimiters are literal
/// (`a**"b"**c`, ``**`b`**c``).  Spans that fail switch to HTML tags.  Returns
/// true when any span switched.
fn mark_unflanked(
    tokens: &[Token],
    uses: &mut [bool],
    ranges: &[(usize, usize)],
    output: &str,
) -> bool {
    let is_markdown_delimiter =
        |token: &Token, uses: &[bool]| token.use_id.is_some_and(|id| !uses[id]);
    let delimiter_char = |token: &Token| match token.span.as_ref().map(|span| span.kind) {
        Some(SpanKind::Bold | SpanKind::Italic) => '*',
        _ => '~',
    };

    let mut changed = false;
    let mut i = 0;
    while i < tokens.len() {
        if !is_markdown_delimiter(&tokens[i], uses) {
            i += 1;
            continue;
        }

        let delimiter = delimiter_char(&tokens[i]);
        let mut last = i;
        while last + 1 < tokens.len()
            && is_markdown_delimiter(&tokens[last + 1], uses)
            && delimiter_char(&tokens[last + 1]) == delimiter
            && ranges[last + 1].0 == ranges[last].1
        {
            last += 1;
        }

        let start = ranges[i].0;
        let end = ranges[last].1;
        let before = output[..start].chars().next_back().unwrap_or('\n');
        let after = output[end..].chars().next().unwrap_or('\n');
        let left_flanking = !after.is_whitespace()
            && (!is_punctuation_or_symbol(after)
                || before.is_whitespace()
                || is_punctuation_or_symbol(before));
        let right_flanking = !before.is_whitespace()
            && (!is_punctuation_or_symbol(before)
                || after.is_whitespace()
                || is_punctuation_or_symbol(after));
        for token in &tokens[i..=last] {
            let opens = token.kind == TokenKind::Open;
            let flanks = if opens { left_flanking } else { right_flanking };
            if !flanks {
                if let Some(id) = token.use_id {
                    uses[id] = true;
                }
                changed = true;
            }
        }

        i = last + 1;
    }

    changed
}

fn ends_with_unescaped(markdown: &str, c: char) -> bool {
    let Some(stripped) = markdown.strip_suffix(c) else {
        return false;
    };

    let backslashes = stripped.chars().rev().take_while(|&b| b == '\\').count();
    backslashes % 2 == 0
}

/// How the markdown parses back compared with the inlines it was written from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReadBack {
    /// The same characters with the same styles, or not one text block.
    Same,
    /// The same characters, some with other styles.
    StylesDiffer,
    /// Other characters: a reference link whose definition is elsewhere, or
    /// delimiters that read as literal text.
    TextDiffers,
}

/// Compares the markdown's parse with the inlines character by character.
/// Whitespace is not compared (it moves out of delimiters on purpose).  When
/// the parse is not one text block there is nothing to compare, so this
/// trusts the flanking pass.
fn read_back(markdown: &str, expected: &[RichInline]) -> ReadBack {
    let document = markdown_parser::parse(markdown);
    if document.blocks.len() != 1 || !document.blocks[0].is_text_block() {
        return ReadBack::Same;
    }

    let wanted = styled_characters(expected);
    let found = styled_characters(&document.blocks[0].inlines);
    if !wanted
        .iter()
        .map(|item| &item.0)
        .eq(found.iter().map(|item| &item.0))
    {
        return ReadBack::TextDiffers;
    }

    if wanted == found {
        ReadBack::Same
    } else {
        ReadBack::StylesDiffer
    }
}

/// Each non-whitespace character (or atom) with its bold, italic and strike.
fn styled_characters(inlines: &[RichInline]) -> Vec<(String, bool, bool, bool)> {
    let mut items = Vec::new();
    for inline in inlines {
        let style = (inline.bold, inline.italic, inline.strike);
        match &inline.content {
            RichInlineContent::Atom(atom) => {
                items.push((
                    format!("atom:{}", atom.raw_markdown),
                    style.0,
                    style.1,
                    style.2,
                ));
            }
            RichInlineContent::Run(run) => {
                for c in run.text.chars().filter(|c| !c.is_whitespace()) {
                    let key = if run.code {
                        format!("`{c}")
                    } else {
                        c.to_string()
                    };
                    items.push((key, style.0, style.1, style.2));
                }
            }
        }
    }

    items
}
