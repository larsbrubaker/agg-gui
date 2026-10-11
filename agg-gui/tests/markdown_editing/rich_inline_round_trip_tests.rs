//! agg-sharp's `RichInlineRoundTripTests`
//! (`Tests/Agg.Tests/MarkdigAgg/RichInlineRoundTripTests.cs`), ported 1:1
//! against `markdown_editing::inline_writer` and `markdown_parser`.
//!
//! The writer's promise, checked as a property: inlines built the way the
//! editor builds them (styled runs, code, atoms, with and without spaces
//! between) write to markdown that reads back as the same text with the same
//! style on every character.  Whitespace styles are not compared: the writer
//! moves whitespace outside delimiters on purpose.

use agg_gui::widgets::markdown_editing::{inline_writer, markdown_parser};
use agg_gui::widgets::markdown_editing::{
    InlineAtomKind, RichInline, RichInlineContent, RichInlineContext,
};

fn run_with(text: &str, bold: bool, italic: bool, strike: bool, code: bool) -> RichInline {
    let mut run = RichInline::run(text);
    run.bold = bold;
    run.italic = italic;
    run.strike = strike;
    if code {
        run = run.code();
    }
    run
}

fn image(bold: bool, italic: bool, strike: bool) -> RichInline {
    let mut atom = RichInline::atom(InlineAtomKind::Image, "![i](a.png)");
    atom.bold = bold;
    atom.italic = italic;
    atom.strike = strike;
    atom
}

#[test]
fn reviewed_edges_read_back() {
    let plain = |text: &str| run_with(text, false, false, false, false);
    let cases = vec![
        vec![
            plain("a"),
            run_with("b", true, false, true, false),
            plain("c"),
        ],
        vec![run_with("b", true, false, false, true), plain("c")],
        vec![
            plain("ab"),
            run_with("q.", true, false, false, false),
            plain("cd"),
        ],
        vec![image(true, false, false), plain("c")],
        vec![
            plain("a"),
            run_with("\"b\"", true, false, false, false),
            plain("c"),
        ],
        vec![
            plain("x "),
            run_with("y", false, true, false, false),
            run_with("z", true, true, false, false),
        ],
    ];
    for inlines in cases {
        assert_eq!(read_back_failure(&inlines), None);
    }
}

/// Every sequence of three pieces (letters, punctuation, code, an image) in
/// every style combination, touching or separated by a space.
#[test]
fn every_three_run_permutation_reads_back() {
    let mut failures = Vec::new();
    let pieces = ["a", "q.", "code", "image"];
    for separator in 0..2 {
        for first in 0..32 {
            for second in 0..32 {
                for third in 0..32 {
                    let mut inlines = Vec::new();
                    let picks = [first, second, third];
                    for (k, pick) in picks.iter().enumerate() {
                        if k > 0 && separator == 1 {
                            inlines.push(RichInline::run(" "));
                        }

                        inlines.push(piece(pieces[pick / 8], pick % 8));
                    }

                    if let Some(failure) = read_back_failure(&inlines) {
                        failures.push(failure);
                    }
                }
            }
        }
    }

    let shown: Vec<String> = failures.into_iter().take(20).collect();
    assert_eq!(shown.join("\n"), "");
}

fn piece(piece: &str, style: usize) -> RichInline {
    let (bold, italic, strike) = (style & 1 != 0, style & 2 != 0, style & 4 != 0);
    match piece {
        "code" => run_with("c", bold, italic, strike, true),
        "image" => image(bold, italic, strike),
        _ => run_with(piece, bold, italic, strike, false),
    }
}

/// Whitespace hoisting once restarted its scan from the first token after
/// every move, so a long paragraph of spans with spaces at their edges took
/// quadratic time (about 2 s for 8,000 spans in Debug).  Counted in scan steps
/// rather than milliseconds, so a loaded machine cannot fail it: the scan stays
/// a small multiple of the token count (each move costs one step back), where a
/// restarting scan grows with its square.
#[test]
fn many_edge_whitespace_spans_hoist_in_linear_steps() {
    let mut inlines = Vec::new();
    for _ in 0..2000 {
        inlines.push(RichInline::run("a"));
        inlines.push(RichInline::run(" b ").bold());
    }

    let mut markdown = String::new();
    let (token_count, hoist_steps) =
        inline_writer::write_counted(&inlines, &mut markdown, RichInlineContext::Paragraph);

    assert_eq!(markdown, "a **b** ".repeat(2000));
    assert!(hoist_steps <= 4 * token_count);
}

/// None when the written markdown reads back as the inlines; otherwise what
/// was written and how it differs.
fn read_back_failure(inlines: &[RichInline]) -> Option<String> {
    let markdown = inline_writer::write(inlines, RichInlineContext::Paragraph);
    let blocks = markdown_parser::parse(&markdown).blocks;
    if blocks.len() != 1 || !blocks[0].is_text_block() {
        return Some(format!(
            "{} -> {markdown} -> {} blocks",
            describe(inlines),
            blocks.len()
        ));
    }

    let wanted = describe(inlines);
    let found = describe(&blocks[0].inlines);
    (wanted != found).then(|| format!("{wanted} -> {markdown} -> {found}"))
}

/// The text with each non-space character's style: "a[B]" is a bold a, "`c`"
/// code, "{img}" the image.
fn describe(inlines: &[RichInline]) -> String {
    let mut text = String::new();
    for inline in inlines {
        let style = format!(
            "{}{}{}",
            if inline.bold { "B" } else { "" },
            if inline.italic { "I" } else { "" },
            if inline.strike { "S" } else { "" }
        );
        match &inline.content {
            RichInlineContent::Atom(atom) => {
                text.push('{');
                text.push_str(&atom.raw_markdown);
                text.push('}');
                if !style.is_empty() {
                    text.push_str(&format!("[{style}]"));
                }
            }
            RichInlineContent::Run(run) => {
                for c in run.text.chars() {
                    if run.code {
                        text.push_str(&format!("`{c}`"));
                    } else {
                        text.push(c);
                    }

                    if !c.is_whitespace() && !style.is_empty() {
                        text.push_str(&format!("[{style}]"));
                    }
                }
            }
        }
    }

    text
}

/// Markdig applies the rule of three to what is left of a delimiter run, so
/// agg-sharp writes `**a***a**a***` here; pulldown-cmark applies it to the run
/// as written and leaves `**` as literal text.  The read-back sees other
/// characters and uses the tags, which read back exactly.
#[test]
fn rust_only_rule_of_three_mismatch_writes_tags() {
    let inlines = [
        run_with("a", true, false, false, false),
        run_with("a", false, true, false, false),
        run_with("a", true, true, false, false),
    ];
    assert_eq!(
        inline_writer::write(&inlines, RichInlineContext::Paragraph),
        "<strong>a</strong><em>a<strong>a</strong></em>"
    );
    assert_eq!(read_back_failure(&inlines), None);
}
