//! agg-sharp's `RichInlineRoundTripTests`
//! (`Tests/Agg.Tests/MarkdigAgg/RichInlineRoundTripTests.cs`), ported 1:1
//! against `markdown_editing::inline_writer`.
//!
//! Waiting for the reader (`RichMarkdownParser`), and ported with it:
//! `ReviewedEdgesReadBack` and `EveryThreeRunPermutationReadsBack`.

use agg_gui::widgets::markdown_editing::inline_writer;
use agg_gui::widgets::markdown_editing::{RichInline, RichInlineContext};

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
