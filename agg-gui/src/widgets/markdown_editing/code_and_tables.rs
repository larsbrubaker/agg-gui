//! Reads code blocks and pipe tables into their [`RichBlock`] models.  Ports
//! agg-sharp's `RichCodeAndTables.cs`.  A block this cannot model is left
//! untouched, so it stays the Raw block the parser made with its exact source.
//!
//! Markdig keeps a pipe inside a code span in its cell (`` `c|d` ``) where
//! pulldown-cmark, following GFM, splits the cell there and drops cells past
//! the header's count.  So whether a block is a table at all is Markdig's
//! verdict ([`super::pipe_tables`]), a table's rows are split from its source
//! the way Markdig splits them, and each cell's text is read as inline
//! markdown on its own ([`super::parser_inlines`]), exactly as a paragraph is.

use pulldown_cmark::Alignment;

use super::block::{RichAlignment, RichBlock, RichBlockKind, RichTableCell};
use super::parse_tree::{self, Node, NodeKind};
use super::parser_inlines::InlineReader;
use super::pipe_tables;

/// Where a code block's source starts: the start of its line, so the fence's
/// indentation is part of the block.  Markdig strips up to that much
/// indentation from every content line, so a rewrite that indented only the
/// opening fence (leaving the indent in the separator) would change the code.
/// An indented block's span starts at its line too in Markdig, where
/// pulldown-cmark's starts after the indentation.
pub(super) fn code_start(code: &Node, body: &str, start: usize, cursor: usize) -> usize {
    if !matches!(code.kind, NodeKind::CodeBlock { .. }) {
        return start;
    }
    let bytes = body.as_bytes();
    let mut line_start = start;
    while line_start > cursor && (bytes[line_start - 1] == b' ' || bytes[line_start - 1] == b'\t') {
        line_start -= 1;
    }
    if line_start == 0 || bytes[line_start - 1] == b'\n' {
        line_start
    } else {
        start
    }
}

/// Fills a code block's text, language, info string and fence from
/// `body[start..end]`, its source.
pub(super) fn read_code(code: &Node, body: &str, start: usize, end: usize, result: &mut RichBlock) {
    let mut info = String::new();
    let mut fence = String::new();
    if code.kind == (NodeKind::CodeBlock { fenced: true }) {
        let source = &body[start..end];
        let fence_start = source.len() - source.trim_start().len();
        let fence_char = source[fence_start..].chars().next().unwrap_or('`');
        let count = source[fence_start..]
            .chars()
            .take_while(|&c| c == fence_char)
            .count();
        fence = fence_char.to_string().repeat(count);
        // The info string as written, not the decoded one: entities and escapes stay as the
        // author typed them, and words after the language ("```cs title=x") are kept for a rewrite.
        let info_start = fence_start + count;
        let line_end = source[info_start..]
            .find('\n')
            .map_or(source.len(), |i| i + info_start);
        info = source[info_start..line_end].trim().to_string();
    }
    // Splitting on whitespace: the language is the info string's first word.
    let language = info.split([' ', '\t']).next().unwrap_or("").to_string();

    // The parser has already removed the fence's (or the indented block's) indentation from each
    // line; content past the block's end (blank lines closing an unclosed fence) is separator.
    let mut text = String::new();
    for child in &code.children {
        if let NodeKind::Text(piece) = &child.kind {
            if child.range.start < end {
                text.push_str(piece);
            }
        }
    }
    let text = text.strip_suffix('\n').unwrap_or(&text);
    let lines: Vec<&str> = if text.is_empty() && code.children.is_empty() {
        Vec::new()
    } else {
        text.split('\n')
            .map(|line| line.strip_suffix('\r').unwrap_or(line))
            .collect()
    };
    let mut code_text = lines.join("\n");
    // Blank lines after the last content line are the separator's, as in Markdig.
    while code.kind == (NodeKind::CodeBlock { fenced: false }) && code_text.ends_with('\n') {
        code_text.pop();
    }

    result.kind = RichBlockKind::CodeBlock;
    result.code_text = code_text;
    result.code_language = language;
    result.code_info = info;
    result.code_fence = fence;
}

/// Fills a pipe table's rows and column alignments from its source, or
/// leaves it Raw: rows wider than the header and cells holding anything but
/// one paragraph are not tables the rich view edits.  The caller has checked
/// that Markdig reads the source as a table starting at its first line
/// ([`pipe_tables::find`]).
pub(super) fn read_table(
    alignments: &[Alignment],
    result: &mut RichBlock,
    soft_breaks_are_hard: bool,
) {
    let lines = pipe_tables::lines_of(&result.original_source);
    // The header, then the delimiter row, then the body rows.
    if lines.len() < 2 {
        return;
    }
    let mut rows = Vec::new();
    let mut columns = 0;
    for (index, line) in lines.iter().enumerate() {
        if index == 1 {
            continue;
        }
        let mut cells = Vec::new();
        for cell in pipe_tables::split_row(line) {
            let Some(cell) = read_cell(cell.trim(), soft_breaks_are_hard) else {
                return;
            };
            cells.push(cell);
        }
        if index == 0 {
            columns = cells.len();
            if columns == 0 || columns > alignments.len() {
                return;
            }
        } else if cells.len() > columns {
            return;
        }
        cells.resize(columns, RichTableCell::default());
        rows.push(cells);
    }

    result.kind = RichBlockKind::Table;
    result.table_rows = rows;
    result.column_alignments = alignments[..columns]
        .iter()
        .map(|alignment| match alignment {
            Alignment::Left => Some(RichAlignment::Left),
            Alignment::Center => Some(RichAlignment::Center),
            Alignment::Right => Some(RichAlignment::Right),
            Alignment::None => None,
        })
        .collect();
}

/// A cell's inlines (an empty cell when written empty), or `None` when its
/// text is not one paragraph.
fn read_cell(text: &str, soft_breaks_are_hard: bool) -> Option<RichTableCell> {
    if text.is_empty() {
        return Some(RichTableCell::default());
    }
    let tree = parse_tree::parse(text);
    let [paragraph] = tree.children.as_slice() else {
        return None;
    };
    if !paragraph.is_paragraph() {
        return None;
    }
    let reader = InlineReader {
        source: text,
        soft_breaks_are_hard,
    };
    Some(RichTableCell {
        inlines: reader.inlines_of(paragraph),
    })
}

/// True for the first line of a grid table (`+---+---+`), a table the viewer
/// reads and the rich view does not edit.
pub(super) fn is_grid_table(source: &str) -> bool {
    let first = source.lines().next().unwrap_or("").trim_end();
    first.len() > 2
        && first.starts_with('+')
        && first.ends_with('+')
        && first[1..first.len() - 1].split('+').all(|column| {
            !column.is_empty() && column.chars().all(|c| matches!(c, '-' | '=' | ':'))
        })
}
