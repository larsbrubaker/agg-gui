//! Turns a [`RichDocument`] back into markdown.  Ports agg-sharp's
//! `RichMarkdownWriter.cs`.
//!
//! Unedited blocks and unchanged groups (see [`super::groups`]) are copied
//! verbatim, so a document loaded and saved with no edits is byte-identical; a
//! changed group is regenerated whole by [`super::group_writer`].  Edited text
//! goes through [`super::inline_writer`].

use super::block::{RichAlignment, RichBlock, RichBlockKind, RichListInfo};
use super::document::RichDocument;
use super::edit_operations;
use super::group_writer;
use super::inline_writer::{self, RichInlineContext};

/// The document as markdown.
pub fn write(document: &RichDocument) -> String {
    write_tracked(document, None).0
}

/// Where block `block_index` starts in [`write`]'s output, in characters: the
/// written lengths of everything before it (frontmatter, earlier blocks,
/// separators, an alignment wrapper's opening), so a Raw block double-clicked
/// in the rich view can be found in the Markdown tab.  A block inside a
/// regenerated group is found by its source within that group's text, or at
/// the group's start when the group rewrote it.
pub fn source_offset_of(document: &RichDocument, block_index: usize) -> usize {
    let (markdown, byte_offset) = write_tracked(document, Some(block_index));
    markdown[..byte_offset].chars().count()
}

/// The markdown and the byte offset where `tracked_block` starts in it (0 when
/// none is tracked).
fn write_tracked(document: &RichDocument, tracked_block: Option<usize>) -> (String, usize) {
    let mut tracked_offset = 0;
    let blocks = &document.blocks;
    let mut markdown = document.frontmatter.clone();
    let blank_line = edit_operations::blank_line(document);
    let newline = &blank_line[..blank_line.len() / 2];
    let unchanged = group_writer::unchanged_groups(blocks);

    // After a regenerated unit the next block needs a blank line before it; previous_list is the
    // top-level style of a list just written, so a regenerated list next to it can keep the two
    // apart, and list_content_column is where that list's last top-level item's text starts: a
    // block indented that far after it would be read into that item.
    let mut blank_line_next = false;
    let mut previous_list: Option<RichListInfo> = None;
    let mut list_content_column: Option<usize> = None;
    let mut i = 0;
    while i < blocks.len() {
        let block = &blocks[i];
        if let Some(unit_end) = group_writer::regenerated_unit_end(blocks, i, &unchanged) {
            markdown.push_str(&group_writer::leading_separator(
                &block.separator_before,
                newline,
                i == 0,
            ));
            let unit = group_writer::write_unit(
                blocks,
                i,
                unit_end,
                newline,
                &mut previous_list,
                &mut list_content_column,
            );
            if let Some(tracked) = tracked_block.filter(|t| (i..=unit_end).contains(t)) {
                let at = unit.find(&blocks[tracked].original_source).unwrap_or(0);
                tracked_offset = markdown.len() + at;
            }

            markdown.push_str(&unit);
            blank_line_next = true;
            i = unit_end + 1;
            continue;
        }

        // An unchanged wrapper's bytes go around its first and last members; a changed one at
        // Left drops.
        let align = block
            .align_group
            .as_ref()
            .filter(|_| group_writer::contains(&unchanged, &block.align_group));
        let mut text = if block.dirty {
            write_block(block, newline)
        } else {
            block.original_source.clone()
        };
        if let Some(one_line) = align.and_then(|group| group.borrow().one_line_source.clone()) {
            // An intact one-line element: its line holds the tags and the text (its Open/Close
            // sources are empty).
            text = one_line;
        }
        if text.is_empty() && align.is_none() {
            // An empty paragraph writes nothing, gap included, so the lists or quotes around it
            // stay apart exactly as if it were not there.
            if tracked_block == Some(i) {
                tracked_offset = markdown.len();
            }

            i += 1;
            continue;
        }

        let mut separator = if blank_line_next {
            group_writer::with_blank_line(&block.separator_before, newline)
        } else {
            block.separator_before.clone()
        };
        if block.kind != RichBlockKind::ListItem
            && list_content_column.is_some_and(|c| group_writer::column(&separator, &text) >= c)
        {
            // Only an edit can put an indented block right after a list (the parser would have
            // read it into the list): start it at the margin, and fence indented code, whose
            // indentation is its syntax.
            separator.truncate(separator.rfind('\n').map_or(0, |at| at + 1));
            if block.kind == RichBlockKind::CodeBlock {
                text = write_code(block, newline, true);
            }
        }

        markdown.push_str(&separator);
        blank_line_next = false;
        let same_align = |other: &RichBlock| matches!((align, &other.align_group), (Some(a), Some(b)) if std::rc::Rc::ptr_eq(a, b));
        if let Some(group) = align {
            if i == 0 || !same_align(&blocks[i - 1]) {
                markdown.push_str(&group.borrow().open_source);
            }
        }

        if tracked_block == Some(i) {
            tracked_offset = markdown.len();
        }

        markdown.push_str(&text);
        if let Some(group) = align {
            if i == blocks.len() - 1 || !same_align(&blocks[i + 1]) {
                markdown.push_str(&group.borrow().close_source);
            }
        }

        if block.kind != RichBlockKind::ListItem {
            previous_list = None;
            list_content_column = None;
        } else if block.list.as_ref().is_some_and(|list| list.depth == 0) {
            previous_list = block.list.clone();
            list_content_column = Some(group_writer::original_content_column(block));
        }

        i += 1;
    }

    markdown.push_str(&document.trailing_text);
    (markdown, tracked_offset)
}

/// Markdown for an edited block, or for a member of a regenerated alignment
/// wrapper.  Raw blocks are never edited in the rich view, so their source
/// stands.  List items and quotes are always written as part of their whole
/// group ([`super::group_writer`]); the writer never sends one here.
pub(crate) fn write_block(block: &RichBlock, newline: &str) -> String {
    match block.kind {
        RichBlockKind::Raw => block.original_source.clone(),
        RichBlockKind::Paragraph => {
            inline_writer::write(&block.inlines, RichInlineContext::Paragraph)
        }
        RichBlockKind::Heading => write_heading(block),
        RichBlockKind::CodeBlock => write_code(block, newline, false),
        RichBlockKind::Table => write_table(block, newline),
        // agg-sharp throws here ("A {kind} block is written with its group"); the writer's
        // unit detection makes it unreachable, so the item's text stands rather than a panic.
        RichBlockKind::ListItem | RichBlockKind::Quote => {
            inline_writer::write(&block.inlines, RichInlineContext::Paragraph)
        }
    }
}

/// An edited heading.  ATX (`## text`) keeps it one line whatever the text.
/// Hard breaks already write as spaces; when an atom's own markdown holds a
/// line break, levels 1 and 2 use a setext underline, which allows several
/// lines, and deeper levels get the break as a space (an attribute value or
/// alt text in that atom then holds a space instead of a newline, which
/// renders the same).
fn write_heading(block: &RichBlock) -> String {
    let level = block.heading_level.clamp(1, 6);
    let content = inline_writer::write(&block.inlines, RichInlineContext::Heading)
        .trim_end()
        .to_string();
    if content.contains('\n') && level <= 2 {
        let line_ending = if content.contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        };
        let underline = if level == 1 { "===" } else { "---" };
        return format!("{content}{line_ending}{underline}");
    }

    let mut content = content.replace("\r\n", " ").replace(['\n', '\r'], " ");

    // A trailing "#" run after a space would read as the optional closing sequence and vanish,
    // unless the writer already escaped it (as it does at the start of the text).
    let chars: Vec<char> = content.chars().collect();
    let mut backslashes = 0;
    while backslashes + 1 < chars.len() && chars[chars.len() - 2 - backslashes] == '\\' {
        backslashes += 1;
    }

    if content.ends_with('#') && backslashes % 2 == 0 {
        content.pop();
        content.push_str("\\#");
    }

    let hashes = "#".repeat(level as usize);
    if content.is_empty() {
        hashes
    } else {
        format!("{hashes} {content}")
    }
}

/// An edited code block, fenced with the author's fence and info string as
/// written, or indented when it was (`code_fence` ""). A fence closes at a run
/// of its character at least as long, so the fence grows past the longest such
/// run in the code; indented code cannot start or end with a blank line or be
/// empty, so those write fenced, as does `fenced` code (indentation would read
/// as list content).
fn write_code(block: &RichBlock, newline: &str, fenced: bool) -> String {
    let normalized = block.code_text.replace("\r\n", "\n");
    let lines: Vec<&str> = normalized.split('\n').collect();
    let can_indent = !block.code_text.is_empty()
        && !lines[0].trim().is_empty()
        && !lines[lines.len() - 1].trim().is_empty();
    if block.code_fence.is_empty() && can_indent && !fenced {
        return lines
            .iter()
            .map(|line| format!("    {line}"))
            .collect::<Vec<_>>()
            .join(newline);
    }

    let fence_char = block.code_fence.chars().next().unwrap_or('`');
    let fence_length = block.code_fence.chars().count().max(3);
    let code: Vec<char> = block.code_text.chars().collect();
    let mut longest_run = 0;
    let mut i = 0;
    while i < code.len() {
        let start = i;
        while i < code.len() && code[i] == fence_char {
            i += 1;
        }

        longest_run = longest_run.max(i - start);
        i = i.max(start + 1);
    }

    let fence = fence_char
        .to_string()
        .repeat(fence_length.max(longest_run + 1));
    let mut markdown = fence.clone();
    markdown.push_str(&info_as_written(block));
    markdown.push_str(newline);
    if !block.code_text.is_empty() {
        markdown.push_str(&lines.join(newline));
        markdown.push_str(newline);
    }

    markdown.push_str(&fence);
    markdown
}

/// The code's info string with the author's spacing after the fence (`~~~ c#`
/// stays `~~~ c#`) while it still matches [`RichBlock::code_info`]; the parser
/// keeps only the trimmed info.
fn info_as_written(block: &RichBlock) -> String {
    let first_line = block.original_source.trim_start_matches([' ', '\t']);
    let first_line = first_line.split('\n').next().unwrap_or("").trim_end();
    if !block.code_fence.is_empty() {
        if let Some(info) = first_line.strip_prefix(block.code_fence.as_str()) {
            if info.trim() == block.code_info {
                return info.to_string();
            }
        }
    }

    block.code_info.clone()
}

/// An edited table as a pipe table: header row, delimiter row from the column
/// alignments, then the body.  Cells are one line each, with '|' escaped
/// outside code.
fn write_table(block: &RichBlock, newline: &str) -> String {
    let mut lines = Vec::new();
    let columns = block.table_rows.first().map_or(0, Vec::len);
    for (row, cells) in block.table_rows.iter().enumerate() {
        // An HTML atom's own line break would end the row; a space renders the same inside a tag.
        let cells: Vec<String> = cells
            .iter()
            .map(|cell| {
                inline_writer::write(&cell.inlines, RichInlineContext::TableCell)
                    .replace("\r\n", " ")
                    .replace(['\n', '\r'], " ")
                    .trim()
                    .to_string()
            })
            .collect();
        lines.push(format!("| {} |", cells.join(" | ")));
        if row == 0 {
            let delimiters: Vec<&str> = (0..columns)
                .map(
                    |column| match block.column_alignments.get(column).copied().flatten() {
                        Some(RichAlignment::Left) => ":---",
                        Some(RichAlignment::Center) => ":---:",
                        Some(RichAlignment::Right) => "---:",
                        None => "---",
                    },
                )
                .collect();
            lines.push(format!("| {} |", delimiters.join(" | ")));
        }
    }

    lines.join(newline)
}
