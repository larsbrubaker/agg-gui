//! Grid tables for `MarkdownView`, read as Markdig's `GridTableParser` reads
//! them (C#'s MarkdigAgg pipeline has `UseGridTables`), so a file that showed
//! a grid table in MarkdigAgg shows the same table here.
//!
//! pulldown-cmark has no grid tables; it reads one as a paragraph.  Like
//! `markdown_editing::pipe_tables::markdig_source`, [`markdig_grid_source`]
//! rewrites the source before pulldown-cmark reads it: each grid table becomes
//! a pipe table holding the same cells, set apart by blank lines (a grid table
//! interrupts a paragraph and ends at the first line that starts with neither
//! `+` nor `|`, as in Markdig).
//!
//! Markdig's reading, checked against Markdig itself:
//! * the first line (`+---+:--:+`) sets the columns, at its `+` positions, and
//!   their alignment from the `:`s;
//! * a `|` line adds a line to each cell of the current row; a cell starts at
//!   each column whose position holds a `|`, so a missing `|` spans a cell
//!   over the next column; a line whose cells start at other columns than the
//!   row's begins a new row; the last cell ends at the right edge when the
//!   line has a `|` there, else at the line's closing `|`;
//! * a `+` line ends the row, and the first `+===+` line makes the rows above
//!   it header rows.  A table without one has no header row.
//!
//! The viewer's cells are one line of text, so a cell's lines are joined with
//! a space (Markdig's soft breaks), and a spanning cell takes its first column
//! (MarkdigAgg's `AggTableRenderer` ignores column spans too).  A table's
//! header is its first row; [`GridSource::headerless`] lists the tables that
//! have none, by the byte offset of the pipe table in the rewritten source.

use std::borrow::Cow;
use std::ops::Range;

use pulldown_cmark::{Event, Options, Parser, Tag};

use crate::widgets::markdown_editing::pipe_tables::escape_code_pipes;

/// The rewritten source and where its headerless tables start.
pub(super) struct GridSource<'a> {
    pub source: Cow<'a, str>,
    /// The byte offsets, in `source`, of tables without a header row.
    pub headerless: Vec<usize>,
}

/// `markdown` with every grid table in a top-level paragraph rewritten as a
/// pipe table; borrowed unchanged when there is none.
pub(super) fn markdig_grid_source(markdown: &str, options: Options) -> GridSource<'_> {
    let mut edits: Vec<(Range<usize>, Vec<Segment>)> = Vec::new();
    for (event, range) in Parser::new_ext(markdown, options).into_offset_iter() {
        let at_line_start = range.start == 0 || markdown[..range.start].ends_with('\n');
        if !matches!(event, Event::Start(Tag::Paragraph)) || !at_line_start {
            continue;
        }
        let segments = segments(&markdown[range.clone()]);
        if segments.iter().any(|s| matches!(s, Segment::Table { .. })) {
            edits.push((range, segments));
        }
    }
    if edits.is_empty() {
        return GridSource {
            source: Cow::Borrowed(markdown),
            headerless: Vec::new(),
        };
    }
    let mut out = String::with_capacity(markdown.len() + 64);
    let mut headerless = Vec::new();
    let mut copied = 0;
    for (range, segments) in edits {
        out.push_str(&markdown[copied..range.start]);
        for segment in segments {
            match segment {
                Segment::Text(text) => out.push_str(&text),
                Segment::Table { text, header } => {
                    if !out.is_empty() && !out.ends_with("\n\n") {
                        out.push_str(if out.ends_with('\n') { "\n" } else { "\n\n" });
                    }
                    if !header {
                        headerless.push(out.len());
                    }
                    out.push_str(&text);
                    out.push('\n');
                }
            }
        }
        copied = range.end;
    }
    out.push_str(&markdown[copied..]);
    GridSource {
        source: Cow::Owned(out),
        headerless,
    }
}

/// A paragraph's source split into its text and its grid tables.
enum Segment {
    Text(String),
    /// A grid table as a pipe table, ending in a blank line.
    Table {
        text: String,
        header: bool,
    },
}

fn segments(source: &str) -> Vec<Segment> {
    let lines: Vec<&str> = source.split_inclusive('\n').collect();
    let mut segments = Vec::new();
    let mut text = String::new();
    let mut i = 0;
    while i < lines.len() {
        let line = trim_line_end(lines[i]);
        let indent = line.len() - line.trim_start_matches(' ').len();
        if indent >= 4 || !is_grid_first_line(&line[indent..]) {
            text.push_str(lines[i]);
            i += 1;
            continue;
        }
        let mut end = i + 1;
        while end < lines.len() {
            let rest = trim_line_end(lines[end]).get(indent..).unwrap_or("");
            if !(rest.starts_with('+') || rest.starts_with('|')) {
                break;
            }
            end += 1;
        }
        let table_lines: Vec<&str> = lines[i..end]
            .iter()
            .map(|line| &trim_line_end(line)[indent..])
            .collect();
        if !text.is_empty() {
            segments.push(Segment::Text(std::mem::take(&mut text)));
        }
        let (text, header) = pipe_table(&table_lines);
        segments.push(Segment::Table {
            text: format!("{text}\n"),
            header,
        });
        i = end;
    }
    if !text.is_empty() {
        segments.push(Segment::Text(text));
    }
    segments
}

fn trim_line_end(line: &str) -> &str {
    line.trim_end_matches(['\n', '\r'])
}

/// True for a grid table's first line: `+`-separated runs of `-` and `:`.
fn is_grid_first_line(line: &str) -> bool {
    let line = line.trim_end();
    line.len() > 2
        && line.starts_with('+')
        && line.ends_with('+')
        && line[1..line.len() - 1]
            .split('+')
            .all(|column| !column.is_empty() && column.chars().all(|c| matches!(c, '-' | ':')))
}

/// One row of a grid table: the columns its cells start at, and each cell's lines.
struct Row {
    starts: Vec<usize>,
    cells: Vec<Vec<String>>,
}

/// Where the last cell of a row line ends (Markdig's `GridTableParser`): at
/// the table's right edge when the line has a `|` (or `+`) there; otherwise
/// the cell runs on, to the line's closing `|` or its end.
fn last_cell_end(chars: &[char], right_edge: usize) -> usize {
    match chars.get(right_edge) {
        Some('|' | '+') => right_edge,
        _ if chars.last() == Some(&'|') => chars.len() - 1,
        _ => chars.len(),
    }
}

/// The pipe table holding a grid table's cells, and whether it has a header row.
fn pipe_table(lines: &[&str]) -> (String, bool) {
    let first: Vec<char> = lines[0].trim_end().chars().collect();
    let plus: Vec<usize> = (0..first.len()).filter(|&i| first[i] == '+').collect();
    let columns = plus.len() - 1;
    let mut rows: Vec<Row> = Vec::new();
    let mut header_rows = 0;
    let mut current: Option<Row> = None;
    for line in &lines[1..] {
        let chars: Vec<char> = line.chars().collect();
        if chars.first() == Some(&'+') {
            rows.extend(current.take());
            if header_rows == 0 && chars.contains(&'=') {
                header_rows = rows.len();
            }
            continue;
        }
        let starts: Vec<usize> = (0..columns)
            .filter(|&k| chars.get(plus[k]) == Some(&'|'))
            .collect();
        if current.as_ref().is_some_and(|row| row.starts != starts) {
            rows.extend(current.take());
        }
        let row = current.get_or_insert_with(|| Row {
            cells: vec![Vec::new(); starts.len()],
            starts: starts.clone(),
        });
        for (n, &k) in starts.iter().enumerate() {
            let next = starts.get(n + 1).copied().unwrap_or(columns);
            let from = (plus[k] + 1).min(chars.len());
            let to = if next < columns {
                plus[next]
            } else {
                last_cell_end(&chars, plus[columns])
            }
            .min(chars.len())
            .max(from);
            row.cells[n].push(chars[from..to].iter().collect());
        }
    }
    rows.extend(current);

    let cells_of = |row: Option<&Row>| -> Vec<String> {
        let mut cells = vec![String::new(); columns];
        if let Some(row) = row {
            for (n, &k) in row.starts.iter().enumerate() {
                let text: Vec<&str> = row.cells[n]
                    .iter()
                    .map(|line| line.trim())
                    .filter(|line| !line.is_empty())
                    .collect();
                cells[k] = escape_code_pipes(&text.join(" "));
            }
        }
        cells
    };
    let line_of = |cells: Vec<String>| format!("| {} |", cells.join(" | "));
    let mut out = vec![line_of(cells_of(rows.first()))];
    let delimiter: Vec<String> = plus
        .windows(2)
        .map(|pair| {
            let segment: String = first[pair[0] + 1..pair[1]].iter().collect();
            match (segment.starts_with(':'), segment.ends_with(':')) {
                (true, true) => ":---:",
                (true, false) => ":---",
                (false, true) => "---:",
                (false, false) => "---",
            }
            .to_string()
        })
        .collect();
    out.push(line_of(delimiter));
    for row in rows.iter().skip(1) {
        out.push(line_of(cells_of(Some(row))));
    }
    (out.join("\n"), header_rows > 0)
}
