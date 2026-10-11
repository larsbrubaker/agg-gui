//! The editing operations on a [`RichDocument`], a port of agg-sharp's
//! `RichEditOperations.cs`.  Holds the document's line-ending rule
//! ([`blank_line`]), which [`super::markdown_writer`] writes regenerated
//! separators with.

use super::document::RichDocument;

/// The gap given to a block an edit creates: one blank line, in the document's
/// line ending (the first line break found in the frontmatter, the blocks or
/// the trailing text).  Inside a group the writer regenerates separators
/// anyway; outside one this is what a new paragraph needs.
pub(crate) fn blank_line(document: &RichDocument) -> &'static str {
    let samples = std::iter::once(document.frontmatter.as_str())
        .chain(document.blocks.iter().flat_map(|block| {
            [
                block.separator_before.as_str(),
                block.original_source.as_str(),
            ]
        }))
        .chain(std::iter::once(document.trailing_text.as_str()));
    for sample in samples {
        if let Some(line_break) = sample.find('\n') {
            return if line_break > 0 && sample.as_bytes()[line_break - 1] == b'\r' {
                "\r\n\r\n"
            } else {
                "\n\n"
            };
        }
    }

    "\n\n"
}
