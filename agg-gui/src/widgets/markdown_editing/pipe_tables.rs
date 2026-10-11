//! Finds pipe tables the way Markdig's `PipeTableParser` finds them, for the
//! rich editor's reader ([`super::markdown_parser`], [`super::code_and_tables`])
//! and the viewer (`widgets::markdown::parse`), so a file C#'s MarkdigAgg read
//! as a table reads as one here, and one it read as a paragraph stays one.
//!
//! pulldown-cmark follows GFM, which differs from Markdig in three ways that
//! decide whether there is a table at all:
//!
//! * GFM splits a cell at a pipe inside a code span (`` `c|d` ``); Markdig
//!   keeps the code span whole.
//! * GFM wants the header row to have exactly as many cells as the delimiter
//!   row; Markdig takes a table whatever the two counts (its columns are the
//!   header's, widened by any wider body row).
//! * GFM takes any line after the delimiter row as a body row; Markdig reads
//!   the whole paragraph as text when a line after the header has no pipe.
//!
//! [`find`] gives Markdig's verdict on a block's source; [`markdig_source`]
//! rewrites a whole document's tables so pulldown-cmark's events match it.

use std::borrow::Cow;
use std::ops::Range;

use pulldown_cmark::{Alignment, Event, Parser, Tag};

use super::parse_tree;

/// A pipe table found in a block's source.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FoundTable {
    /// The index of the header line among the source's lines; lines before it
    /// are a paragraph of their own.
    pub header_line: usize,
    /// The delimiter row's alignments, one per delimiter cell.
    pub alignments: Vec<Alignment>,
}

/// The source's lines, each without its line break (and `\r`).
pub(crate) fn lines_of(source: &str) -> Vec<&str> {
    source
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect()
}

/// Markdig's verdict on a paragraph's (or a GFM table's) source: the table it
/// holds, or `None` when Markdig reads it as text.  A table starts at the
/// first line holding a pipe that is followed by a delimiter row, and every
/// line after the header must hold a pipe.
pub(crate) fn find(source: &str) -> Option<FoundTable> {
    let lines = lines_of(source.trim_end_matches(['\n', '\r']));
    for header_line in 0..lines.len().saturating_sub(1) {
        if pipe_positions(lines[header_line]).is_empty() {
            continue;
        }
        let Some(alignments) = delimiter_alignments(lines[header_line + 1]) else {
            continue;
        };
        if lines[header_line + 2..]
            .iter()
            .any(|line| pipe_positions(line).is_empty())
        {
            return None;
        }
        return Some(FoundTable {
            header_line,
            alignments,
        });
    }
    None
}

/// The alignments of a delimiter row (`| :--- | ---: |`), or `None` when the
/// line is not one: it needs a pipe, and every cell between pipes is dashes
/// with an optional colon at either end.
pub(crate) fn delimiter_alignments(line: &str) -> Option<Vec<Alignment>> {
    let row = line.trim();
    if !row.contains('|') {
        return None;
    }
    let row = row.strip_prefix('|').unwrap_or(row);
    let row = row.strip_suffix('|').unwrap_or(row);
    row.split('|')
        .map(|cell| {
            let cell = cell.trim();
            let left = cell.starts_with(':');
            let right = cell.len() > 1 && cell.ends_with(':');
            let dashes = &cell[usize::from(left)..cell.len() - usize::from(right)];
            if dashes.is_empty() || !dashes.bytes().all(|b| b == b'-') {
                return None;
            }
            Some(match (left, right) {
                (true, true) => Alignment::Center,
                (true, false) => Alignment::Left,
                (false, true) => Alignment::Right,
                (false, false) => Alignment::None,
            })
        })
        .collect()
}

/// Where a row (already trimmed and without its leading pipe) has a cell
/// separator: each pipe outside a code span and not escaped.
fn pipe_positions(row: &str) -> Vec<usize> {
    let bytes = row.as_bytes();
    let mut pipes = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'\\' => at += 2,
            b'`' => {
                let run = bytes[at..].iter().take_while(|&&b| b == b'`').count();
                let ticks = &row[at..at + run];
                at += run;
                // A code span runs to the next backtick run of the same length; without one the
                // backticks are text.
                let mut search = at;
                while let Some(found) = row[search..].find(ticks).map(|i| i + search) {
                    let close_run = bytes[found..].iter().take_while(|&&b| b == b'`').count();
                    if close_run == run {
                        at = found + run;
                        break;
                    }
                    search = found + close_run;
                }
            }
            b'|' => {
                pipes.push(at);
                at += 1;
            }
            _ => at += 1,
        }
    }
    pipes
}

/// A row's cells as written, split at each pipe outside a code span and not
/// escaped; the outer pipes are optional.
pub(crate) fn split_row(line: &str) -> Vec<&str> {
    let row = line.trim();
    let row = row.strip_prefix('|').unwrap_or(row);
    let mut cells = Vec::new();
    let mut cell_start = 0;
    for pipe in pipe_positions(row) {
        cells.push(&row[cell_start..pipe]);
        cell_start = pipe + 1;
    }
    if !row[cell_start..].trim().is_empty() || cells.is_empty() {
        cells.push(&row[cell_start..]);
    }
    cells
}

/// `markdown` with its pipe tables rewritten so pulldown-cmark reads exactly
/// the tables Markdig reads: a table Markdig finds is written out with its
/// cells split Markdig's way (pipes inside code spans escaped, which GFM
/// unescapes) and its delimiter row as wide as its widest row; a GFM table
/// Markdig reads as text has its delimiter row escaped, so it stays a
/// paragraph.  Borrowed unchanged when nothing differs.
pub(crate) fn markdig_source(markdown: &str) -> Cow<'_, str> {
    let mut edits: Vec<(Range<usize>, String)> = Vec::new();
    let mut covered = 0;
    for (event, range) in Parser::new_ext(markdown, parse_tree::options()).into_offset_iter() {
        let is_table = matches!(event, Event::Start(Tag::Table(_)));
        if !(is_table || matches!(event, Event::Start(Tag::Paragraph))) || range.start < covered {
            continue;
        }
        covered = range.end;
        let source = &markdown[range.clone()];
        let found = find(source);
        let rewritten = match found {
            Some(found) => rewrite_table(source, &found),
            None if is_table => escape_delimiter_row(source),
            None => continue,
        };
        if rewritten != source {
            edits.push((range, rewritten));
        }
    }
    if edits.is_empty() {
        return Cow::Borrowed(markdown);
    }
    let mut out = markdown.to_string();
    for (range, text) in edits.into_iter().rev() {
        out.replace_range(range, &text);
    }
    Cow::Owned(out)
}

/// The block with the table's lines written GFM's way, each keeping its
/// indentation and line ending.
fn rewrite_table(source: &str, found: &FoundTable) -> String {
    let mut lines: Vec<String> = source.split('\n').map(str::to_string).collect();
    let content_lines = lines_of(source.trim_end_matches(['\n', '\r'])).len();
    let rows: Vec<usize> = (found.header_line..content_lines)
        .filter(|&i| i != found.header_line + 1)
        .collect();
    let columns = rows
        .iter()
        .map(|&i| split_row(&lines[i]).len())
        .max()
        .unwrap_or(0);
    for &i in &rows {
        let mut cells: Vec<String> = split_row(&lines[i])
            .iter()
            .map(|cell| escape_code_pipes(cell.trim()))
            .collect();
        if i == found.header_line {
            cells.resize(columns, String::new());
        }
        lines[i] = rewrite_line(&lines[i], &format!("| {} |", cells.join(" | ")));
    }
    let delimiter: Vec<&str> = (0..columns)
        .map(|c| match found.alignments.get(c) {
            Some(Alignment::Left) => ":---",
            Some(Alignment::Center) => ":---:",
            Some(Alignment::Right) => "---:",
            _ => "---",
        })
        .collect();
    let delimiter_line = found.header_line + 1;
    lines[delimiter_line] = rewrite_line(
        &lines[delimiter_line],
        &format!("| {} |", delimiter.join(" | ")),
    );
    lines.join("\n")
}

/// `content` in place of `line`'s, keeping its indentation and `\r`.
fn rewrite_line(line: &str, content: &str) -> String {
    let indent = &line[..line.len() - line.trim_start().len()];
    let cr = if line.ends_with('\r') { "\r" } else { "" };
    format!("{indent}{content}{cr}")
}

/// A cell with every pipe left in it (one inside a code span, since the cell
/// was split at the rest) escaped, as GFM wants it.
pub(crate) fn escape_code_pipes(cell: &str) -> String {
    let mut out = String::with_capacity(cell.len());
    let mut escaped = false;
    for c in cell.chars() {
        if c == '|' && !escaped {
            out.push('\\');
        }
        escaped = c == '\\' && !escaped;
        out.push(c);
    }
    out
}

/// The block with a backslash before its delimiter row (line two), which
/// GFM then no longer reads as one; the escape is a punctuation escape, so
/// the text shows as written.
fn escape_delimiter_row(source: &str) -> String {
    let mut lines: Vec<String> = source.split('\n').map(str::to_string).collect();
    if let Some(line) = lines.get_mut(1) {
        let indent = line.len() - line.trim_start().len();
        line.insert(indent, '\\');
    }
    lines.join("\n")
}
