//! Writes changed groups (see [`super::groups`]) for
//! [`super::markdown_writer`]: a whole list, blockquote or alignment wrapper
//! rebuilt from the model — every member, clean ones too, and the separators
//! between them — since an original member's bytes hold indentation,
//! numbering and `>` joiners that only fit the group as it was parsed.  Ports
//! agg-sharp's `RichGroupWriter.cs`.
//!
//! C# keeps groups in a `HashSet<RichBlockGroup>` by reference; here a group's
//! identity is its `Rc` allocation's address ([`GroupSet`]).

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use super::block::{RichAlignment, RichBlock, RichBlockKind, RichListInfo};
use super::inline_writer::{self, RichInlineContext};
use super::markdown_writer;

/// Written after the marker of an empty item that opens a list or sublist.
/// Right below its parent's text a bare marker cannot start an item (an empty
/// item cannot interrupt a paragraph), so it would read as more of that text
/// or underline it into a heading; Markdig does not start a top-level list
/// with a bare marker either.  The comment renders as nothing and the parser
/// reads the item back as empty.
pub const EMPTY_ITEM_COMMENT: &str = "<!-- -->";

const BULLETS: [char; 3] = ['-', '*', '+'];

/// The groups still exactly as parsed, by `Rc` address.
pub(crate) type GroupSet = HashSet<usize>;

fn address<T>(group: &Rc<RefCell<T>>) -> usize {
    Rc::as_ptr(group) as *const () as usize
}

/// True when `group` is in `set`.
pub(crate) fn contains<T>(set: &GroupSet, group: &Option<Rc<RefCell<T>>>) -> bool {
    group
        .as_ref()
        .is_some_and(|group| set.contains(&address(group)))
}

fn same_group<T>(a: &Option<Rc<RefCell<T>>>, b: &Option<Rc<RefCell<T>>>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => Rc::ptr_eq(a, b),
        _ => false,
    }
}

/// A block's groups (align, quote, list) as (address, original member count,
/// whether the block still has the group's alignment).
fn groups_of(block: &RichBlock) -> Vec<(usize, usize, bool)> {
    let mut groups = Vec::new();
    if let Some(align) = &block.align_group {
        let group = align.borrow();
        groups.push((
            address(align),
            group.original_member_count,
            block.alignment == group.alignment,
        ));
    }

    if let Some(quote) = &block.quote_group {
        groups.push((address(quote), quote.borrow().original_member_count, true));
    }

    if let Some(list) = &block.list_group {
        groups.push((address(list), list.borrow().original_member_count, true));
    }

    groups
}

/// The groups still exactly as parsed, found in one pass over the blocks: one
/// contiguous run of `original_member_count` members, every member clean and,
/// for alignment, still at the group's alignment.
pub(crate) fn unchanged_groups(blocks: &[RichBlock]) -> GroupSet {
    // Per group: the index of its last member seen, its member count, whether it is still
    // intact, and its original member count.
    let mut seen: HashMap<usize, (isize, usize, bool, usize)> = HashMap::new();
    for (i, block) in blocks.iter().enumerate() {
        let i = i as isize;
        for (group, original, aligned) in groups_of(block) {
            let state = seen
                .get(&group)
                .copied()
                .unwrap_or((i - 1, 0, true, original));
            let intact = state.2 && state.0 == i - 1 && !block.dirty && aligned;
            seen.insert(group, (i, state.1 + 1, intact, original));
        }
    }

    seen.into_iter()
        .filter(|(_, (_, count, intact, original))| *intact && *original > 0 && count == original)
        .map(|(group, _)| group)
        .collect()
}

/// The last index of the regenerated unit starting at `first`, or `None` when
/// that block is written on its own (its original bytes or a dirty rewrite).
/// A unit is a changed list or quote, or a run of paragraphs and headings
/// sharing one non-left alignment outside any unchanged wrapper.  A changed
/// "left" wrapper simply drops: left is how unwrapped blocks render anyway.
pub(crate) fn regenerated_unit_end(
    blocks: &[RichBlock],
    first: usize,
    unchanged: &GroupSet,
) -> Option<usize> {
    let block = &blocks[first];
    let member: Box<dyn Fn(&RichBlock) -> bool> = if block.kind == RichBlockKind::ListItem
        && !contains(unchanged, &block.list_group)
    {
        // A ListItem with no group (the parser and the ops always give one) is a list of one.
        Box::new(|other: &RichBlock| {
            other.kind == RichBlockKind::ListItem
                && same_group(&block.list_group, &other.list_group)
        })
    } else if block.kind == RichBlockKind::Quote && !contains(unchanged, &block.quote_group) {
        Box::new(|other: &RichBlock| {
            other.kind == RichBlockKind::Quote && same_group(&block.quote_group, &other.quote_group)
        })
    } else if in_align_run(block, unchanged) {
        Box::new(|other: &RichBlock| {
            in_align_run(other, unchanged) && other.alignment == block.alignment
        })
    } else {
        return None;
    };

    let mut last = first;
    while last + 1 < blocks.len() && member(&blocks[last + 1]) {
        last += 1;
    }

    Some(last)
}

fn in_align_run(block: &RichBlock, unchanged: &GroupSet) -> bool {
    matches!(
        block.kind,
        RichBlockKind::Paragraph | RichBlockKind::Heading
    ) && block.alignment != RichAlignment::Left
        && !contains(unchanged, &block.align_group)
}

/// Writes the regenerated unit `blocks[first..=last]`.  `previous_list` is the
/// top-level style of a list written right before this one (`None` when the
/// block before is not a list item); it is updated to this unit's top-level
/// style when the unit is a list.  `list_content_column` becomes the column the
/// list's last top-level item's text starts at, or `None` for another unit.
pub(crate) fn write_unit(
    blocks: &[RichBlock],
    first: usize,
    last: usize,
    newline: &str,
    previous_list: &mut Option<RichListInfo>,
    list_content_column: &mut Option<usize>,
) -> String {
    let block = &blocks[first];
    if block.kind == RichBlockKind::ListItem {
        return write_list(
            blocks,
            first,
            last,
            newline,
            previous_list,
            list_content_column,
        );
    }

    *previous_list = None;
    *list_content_column = None;
    if block.kind == RichBlockKind::Quote {
        return write_quote(blocks, first, last, newline);
    }

    write_align_run(blocks, first, last, newline)
}

/// A separator that holds at least one blank line, so a regenerated unit can
/// never run into its neighbour (a paragraph would continue a list item or
/// quote lazily; two wrappers' tags would share one HTML block).  Indentation
/// after the last line break is kept: it belongs to the block that follows.
pub(crate) fn with_blank_line(separator: &str, newline: &str) -> String {
    if separator.matches('\n').count() >= 2 {
        return separator.to_string();
    }

    let line_start = separator.rfind('\n').map_or(0, |at| at + 1);
    format!("{newline}{newline}{}", &separator[line_start..])
}

/// The separator before a regenerated unit: its line breaks only (the unit
/// writes its own indentation), at least a blank line unless the unit opens
/// the document.
pub(crate) fn leading_separator(separator: &str, newline: &str, first_block: bool) -> String {
    let mut line_breaks = separator.matches('\n').count();
    if !first_block {
        line_breaks = line_breaks.max(2);
    }

    newline.repeat(line_breaks)
}

/// The column a block starts at: the indentation after the separator's last
/// line break plus any at the start of its own markdown (a code block's source
/// holds its indentation).  A tab advances to a multiple of 4.
pub(crate) fn column(separator: &str, text: &str) -> usize {
    let line_start = separator.rfind('\n').map_or(0, |at| at + 1);
    let mut column = 0;
    for c in separator[line_start..].chars().chain(text.chars()) {
        match c {
            ' ' => column += 1,
            '\t' => column += 4 - (column % 4),
            _ => break,
        }
    }

    column
}

/// Where a parsed (clean) list item's text starts: its indentation, its marker
/// and the 1-4 spaces after it (more than 4 means indented code inside the
/// item, which starts 1 space after the marker, as does an empty item).
pub(crate) fn original_content_column(item: &RichBlock) -> usize {
    let source: Vec<char> = item.original_source.chars().collect();
    let mut marker = source.iter().take_while(|c| c.is_ascii_digit()).count();
    marker = (marker + 1).min(source.len());
    let mut spaces = 0;
    while marker + spaces < source.len() && source[marker + spaces] == ' ' {
        spaces += 1;
    }

    let line_ends = marker + spaces >= source.len()
        || source[marker + spaces] == '\n'
        || source[marker + spaces] == '\r';
    if spaces == 0 || spaces > 4 || line_ends {
        spaces = 1;
    }

    column(&item.separator_before, "") + marker + spaces
}

/// One list level being written: where its markers start, its style (from its
/// first item) and the number its next item gets.
struct ListLevel {
    indent: usize,
    ordered: bool,
    marker: char,
    loose: bool,
    next: i32,
    /// The content column of this level's latest item: where a child list's
    /// markers go (CommonMark nests a list under an item by indenting it to the
    /// item's content).
    child_indent: usize,
}

/// A list rebuilt from each item's depth and style.  Each (sub)list takes its
/// marker, numbering start and spacing from its first item, so items that
/// disagree still read back as one list; numbers count up from that start.  A
/// change of kind at one depth starts a new sublist, as it does in markdown.
///
/// Spacing: items of a loose (sub)list ([`RichListInfo::loose`]) are a blank
/// line apart, all others a line break, whatever the gaps in the model hold —
/// edits give every new block a blank-line gap, so the gaps cannot tell.
///
/// Two lists side by side: a blank line alone would join them into one loose
/// list, so when this list has the same kind and marker as the one right
/// before it, it switches to another bullet ('-', '*', '+') or delimiter ('.',
/// ')'), which CommonMark reads as a new list.  Unlike an HTML comment between
/// them, nothing extra shows in the Markdown tab.
fn write_list(
    blocks: &[RichBlock],
    first: usize,
    last: usize,
    newline: &str,
    previous_list: &mut Option<RichListInfo>,
    list_content_column: &mut Option<usize>,
) -> String {
    let mut markdown = String::new();
    let mut levels: Vec<ListLevel> = Vec::new();
    let mut top_style: Option<RichListInfo> = None;
    for (k, block) in blocks.iter().enumerate().take(last + 1).skip(first) {
        let info = block.list.clone().unwrap_or_default();

        // The ops keep depths gapless; a skipped level has no markdown form, so it is clamped.
        let depth = info.depth.min(levels.len());
        let continues = depth < levels.len() && levels[depth].ordered == info.ordered;
        let content = inline_writer::write(&block.inlines, RichInlineContext::Paragraph)
            .replace("\r\n", "\n");
        if continues {
            levels.truncate(depth + 1);
        } else {
            levels.truncate(depth);
            let indent = if depth == 0 {
                0
            } else {
                levels[depth - 1].child_indent
            };
            let avoid = if depth == 0 && k == first {
                previous_list.as_ref()
            } else {
                None
            };
            levels.push(new_level(&info, indent, avoid));
            if depth == 0 {
                top_style = Some(RichListInfo {
                    ordered: info.ordered,
                    marker: levels[0].marker,
                    ..RichListInfo::default()
                });
            }
        }

        let level = &mut levels[depth];
        if k > first {
            // A nested list starting at a number other than 1 cannot interrupt its parent's
            // paragraph: right below it the line would continue that paragraph.
            let cannot_interrupt = !continues && depth > 0 && level.ordered && level.next != 1;
            markdown.push_str(newline);
            if (continues && level.loose) || cannot_interrupt {
                markdown.push_str(newline);
            }
        }

        let marker = if level.ordered {
            let number = level.next;
            level.next += 1;
            format!("{number}{}", level.marker)
        } else {
            level.marker.to_string()
        };
        level.child_indent = level.indent + marker.chars().count() + 1;
        let lines: Vec<&str> = content.split('\n').collect();
        markdown.push_str(&" ".repeat(level.indent));
        markdown.push_str(&marker);
        if !lines[0].is_empty() {
            markdown.push(' ');
            markdown.push_str(lines[0]);
        } else if !continues {
            markdown.push(' ');
            markdown.push_str(EMPTY_ITEM_COMMENT);
        }

        for line in &lines[1..] {
            markdown.push_str(newline);
            markdown.push_str(&" ".repeat(level.child_indent));
            markdown.push_str(line);
        }
    }

    *previous_list = top_style;
    *list_content_column = levels.first().map(|level| level.child_indent);
    markdown
}

fn new_level(info: &RichListInfo, indent: usize, avoid: Option<&RichListInfo>) -> ListLevel {
    let mut marker = if info.ordered {
        if info.marker == ')' {
            ')'
        } else {
            '.'
        }
    } else if BULLETS.contains(&info.marker) {
        info.marker
    } else {
        '-'
    };
    if let Some(avoid) = avoid {
        if avoid.ordered == info.ordered && avoid.marker == marker {
            marker = if info.ordered {
                if marker == '.' {
                    ')'
                } else {
                    '.'
                }
            } else {
                BULLETS
                    .into_iter()
                    .find(|&bullet| bullet != marker)
                    .unwrap_or('-')
            };
        }
    }

    ListLevel {
        indent,
        ordered: info.ordered,
        marker,
        loose: info.loose,
        next: info.start_number.max(0),
        child_indent: 0,
    }
}

/// One blockquote: every line prefixed `> ` (so hard breaks and wrapped lines
/// stay inside it) and paragraphs joined by a bare `>` line.
fn write_quote(blocks: &[RichBlock], first: usize, last: usize, newline: &str) -> String {
    let mut markdown = String::new();
    for (k, block) in blocks.iter().enumerate().take(last + 1).skip(first) {
        if k > first {
            markdown.push_str(newline);
            markdown.push('>');
            markdown.push_str(newline);
        }

        let content = inline_writer::write(&block.inlines, RichInlineContext::Paragraph)
            .replace("\r\n", "\n");
        for (index, line) in content.split('\n').enumerate() {
            if index > 0 {
                markdown.push_str(newline);
            }

            if line.is_empty() {
                markdown.push('>');
            } else {
                markdown.push_str("> ");
                markdown.push_str(line);
            }
        }
    }

    markdown
}

/// Paragraphs and headings sharing one alignment, in the viewer's storage
/// form: the `<div align>` tag, a blank line, the blocks a blank line apart, a
/// blank line, `</div>`.  The blank lines make each tag its own HTML block so
/// the markdown between them is still read as markdown.
fn write_align_run(blocks: &[RichBlock], first: usize, last: usize, newline: &str) -> String {
    let blank_line = format!("{newline}{newline}");
    let mut markdown = String::from("<div align=\"");
    markdown.push_str(if blocks[first].alignment == RichAlignment::Center {
        "center"
    } else {
        "right"
    });
    markdown.push_str("\">");
    for block in &blocks[first..=last] {
        // An empty paragraph writes nothing, as it does outside a wrapper.
        let written = markdown_writer::write_block(block, newline);
        if !written.is_empty() {
            markdown.push_str(&blank_line);
            markdown.push_str(&written);
        }
    }

    markdown.push_str(&blank_line);
    markdown.push_str("</div>");
    markdown
}
