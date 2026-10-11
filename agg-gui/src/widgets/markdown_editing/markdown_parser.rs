//! Reads markdown into a [`RichDocument`] whose blocks partition the source
//! exactly, so [`super::markdown_writer::write`] of an unedited document gives
//! back the same bytes.  Ports agg-sharp's `RichMarkdownParser.cs`.
//!
//! Paragraphs, headings, single-paragraph list items, plain-paragraph quotes,
//! `<div align>` groups ([`super::align_groups`]), code blocks and pipe tables
//! ([`super::code_and_tables`], found as Markdig finds them by
//! [`super::pipe_tables`]) are modelled; every other block is Raw (shown
//! rendered, edited in the Markdown tab).
//!
//! agg-sharp builds the partition from Markdig's precise spans; this port
//! builds it from pulldown-cmark's source ranges ([`super::parse_tree`]),
//! trimmed to Markdig's: a block ends at its last content line (the line break
//! and blank lines after it are separator), a paragraph runs from its first
//! inline to its last, and link reference definitions, which neither parser
//! puts in a block, fall into the gaps and become Raw blocks of whole lines.

use std::ops::Range;

use super::align_groups;
use super::block::{RichBlock, RichBlockKind, RichListInfo};
use super::code_and_tables;
use super::document::RichDocument;
use super::group_writer::EMPTY_ITEM_COMMENT;
use super::groups::{shared, RichListGroup, RichQuoteGroup};
use super::parse_tree::{self, Node, NodeKind};
use super::parser_inlines::InlineReader;
use super::pipe_tables;

/// Reads `markdown` into a document.
pub fn parse(markdown: &str) -> RichDocument {
    parse_with(markdown, false)
}

/// [`parse`], optionally keeping each soft line break as a hard break: how
/// pasted plain text keeps its lines, where markdown would join them into one.
pub fn parse_with(markdown: &str, soft_breaks_are_hard: bool) -> RichDocument {
    let frontmatter = frontmatter_of(markdown).to_string();
    // Parse only the body so hidden frontmatter never reads as a thematic break or a setext heading.
    let body = &markdown[frontmatter.len()..];
    let mut parser = Reader {
        body,
        reader: InlineReader {
            source: body,
            soft_breaks_are_hard,
        },
        document: RichDocument::new(),
        pending_separator: String::new(),
        html_blocks: Vec::new(),
    };
    parser.document.frontmatter = frontmatter;
    parser.run();
    let Reader {
        mut document,
        html_blocks,
        ..
    } = parser;
    align_groups::fold(&mut document.blocks, html_blocks);
    document
}

/// One modelled block cut out of a list or quote: the source range it owns
/// and the paragraph it reads.
struct Segment<'n> {
    start: usize,
    end: usize,
    paragraph: Option<&'n Node>,
    kind: RichBlockKind,
    list: Option<RichListInfo>,
}

struct Reader<'a> {
    body: &'a str,
    reader: InlineReader<'a>,
    document: RichDocument,
    // Whitespace seen since the last block; it becomes the next block's separator (or the trailing text).
    pending_separator: String,
    // Per block, true for one read from an HTML block: only these can be an alignment wrapper's
    // tags (an indented code block holding "<div align>" text must stay code).
    html_blocks: Vec<bool>,
}

impl<'a> Reader<'a> {
    fn run(&mut self) {
        let tree = parse_tree::parse(self.body);
        let mut cursor = 0;
        for block in &tree.children {
            // A list or quote of plain paragraphs becomes one block per paragraph; anything richer
            // is one Raw block.
            let segments = match block.kind {
                NodeKind::List { .. } => self.list_segments(block, cursor),
                NodeKind::Quote => self.quote_segments(block, cursor),
                _ => None,
            };
            if let Some(segments) = segments {
                cursor = self.add_segments(cursor, &segments);
                continue;
            }

            let mut range = self.content_range(block);
            range.start = range.start.max(cursor);
            if range.end <= range.start {
                continue;
            }
            if matches!(block.kind, NodeKind::CodeBlock { .. }) {
                range.start = code_and_tables::code_start(block, self.body, range.start, cursor);
            }
            self.add_gap(cursor, range.start);
            if block.is_paragraph() {
                range.start = self.add_paragraph_before_table(block, range.clone());
            }
            let built = self.build_block(Some(block), range.start, range.end);
            self.document.blocks.push(built);
            cursor = range.end;
        }
        self.add_gap(cursor, self.body.len());
        self.document.trailing_text = std::mem::take(&mut self.pending_separator);
    }

    /// Where a paragraph's table starts when Markdig finds one below its
    /// first line (`foo` above a table whose rows GFM refuses): the lines
    /// above it are added as a paragraph of their own, the line break after
    /// them as separator.  `range.start` when there is no such table.
    fn add_paragraph_before_table(&mut self, paragraph: &Node, range: Range<usize>) -> usize {
        let source = &self.body[range.clone()];
        let Some(found) = pipe_tables::find(source).filter(|found| found.header_line > 0) else {
            return range.start;
        };
        let table_start = range.start
            + source
                .match_indices('\n')
                .nth(found.header_line - 1)
                .map_or(0, |(i, _)| i + 1);
        let mut above = paragraph.clone();
        above.children.retain(|child| child.range.end < table_start);
        while above
            .children
            .last()
            .is_some_and(|c| matches!(c.kind, NodeKind::SoftBreak | NodeKind::HardBreak))
        {
            above.children.pop();
        }
        if above.children.is_empty() {
            return range.start;
        }
        above.range = above.inline_extent(self.body);
        let built = self.build_block(Some(&above), above.range.start, above.range.end);
        self.document.blocks.push(built);
        self.add_gap(above.range.end, table_start);
        table_start
    }

    /// A block's source as Markdig spans it: a paragraph from its first inline
    /// to its last, anything else without the line break and blank lines after
    /// its last content line.
    fn content_range(&self, block: &Node) -> Range<usize> {
        if block.is_paragraph() {
            return block.range.clone();
        }
        block.range.start..trim_trailing_blank_lines(self.body, block.range.clone())
    }

    /// One ListItem segment per item, nested lists flattened in source order
    /// with their depth, or `None` when any item holds more than one paragraph
    /// or anything but a paragraph and nested lists.  `None` makes the whole
    /// top-level list one Raw block: splitting a list around an unmodelled
    /// item would let an edit to its neighbours renumber or re-indent markdown
    /// the rich view cannot show.
    fn list_segments<'n>(&self, list: &'n Node, cursor: usize) -> Option<Vec<Segment<'n>>> {
        let mut segments = Vec::new();
        if !self.add_list_segments(list, 0, &mut segments)
            || segments.is_empty()
            || segments[0].start < cursor
        {
            return None;
        }
        self.gaps_are_whitespace(&segments).then_some(segments)
    }

    fn add_list_segments<'n>(
        &self,
        list: &'n Node,
        depth: usize,
        segments: &mut Vec<Segment<'n>>,
    ) -> bool {
        let NodeKind::List { start, loose } = list.kind else {
            return false;
        };
        let ordered = start.is_some();
        let Ok(start_number) = i32::try_from(start.unwrap_or(1)) else {
            return false;
        };
        let bytes = self.body.as_bytes();
        for item in &list.children {
            if item.kind != NodeKind::Item {
                return false;
            }
            // The item starts at its marker; indentation before it is whitespace for the separator.
            let item_start = self.skip_spaces(item.range.start);
            let mut marker_end = item_start;
            while marker_end < bytes.len() && bytes[marker_end].is_ascii_digit() {
                marker_end += 1;
            }
            let marker = bytes.get(marker_end).map_or('-', |&b| b as char);
            marker_end += 1;

            let mut paragraph = None;
            let item_end;
            let mut nested_from = 1;
            if self.is_empty_item(item, item_start) {
                // An empty item (what Enter leaves) owns its marker line; any nested lists follow as usual.
                item_end = self.line_end(item_start);
                nested_from = usize::from(
                    !item
                        .children
                        .first()
                        .is_some_and(|c| matches!(c.kind, NodeKind::List { .. })),
                );
            } else {
                let Some(first) = item.children.first().filter(|c| c.is_paragraph()) else {
                    return false;
                };
                // Reference definitions at the top of the item would sit in its source but not its inlines.
                let paragraph_start = first.range.start;
                if item_start >= paragraph_start
                    || marker_end > paragraph_start
                    || !self.body[marker_end..paragraph_start].trim().is_empty()
                {
                    return false;
                }
                paragraph = Some(first);
                item_end = first.range.end;
            }

            segments.push(Segment {
                start: item_start,
                end: item_end,
                paragraph,
                kind: RichBlockKind::ListItem,
                list: Some(RichListInfo {
                    ordered,
                    depth,
                    marker,
                    start_number,
                    loose,
                }),
            });

            for nested in item.children.iter().skip(nested_from) {
                if !matches!(nested.kind, NodeKind::List { .. })
                    || !self.add_list_segments(nested, depth + 1, segments)
                {
                    return false;
                }
            }
        }
        true
    }

    /// One Quote segment per paragraph of a blockquote whose children are all
    /// paragraphs, or `None` (Raw) for nested quotes, lists, code and the like.
    /// Each segment's source keeps its `>` prefixes.  The `>` line between two
    /// paragraphs starts the second segment, leaving a bare line break as the
    /// separator, and `>` lines closing the quote end the last segment.
    fn quote_segments<'n>(&self, quote: &'n Node, cursor: usize) -> Option<Vec<Segment<'n>>> {
        let quote_start = self.skip_spaces(quote.range.start);
        let quote_end = trim_trailing_blank_lines(self.body, quote.range.clone());
        if quote.children.is_empty()
            || quote_start < cursor
            || self.body.as_bytes().get(quote_start) != Some(&b'>')
        {
            return None;
        }

        let mut segments: Vec<Segment<'n>> = Vec::new();
        let mut start = quote_start;
        for paragraph in &quote.children {
            if !paragraph.is_paragraph() {
                return None;
            }
            let paragraph_start = paragraph.range.start;
            if let Some(last) = segments.last() {
                let line_break = self.body[last.end..].find('\n').map(|i| i + last.end)?;
                if line_break >= paragraph_start {
                    return None;
                }
                start = line_break + 1;
            }
            if start > paragraph_start || !self.only_quote_markers(start, paragraph_start) {
                return None;
            }
            segments.push(Segment {
                start,
                end: paragraph.range.end,
                paragraph: Some(paragraph),
                kind: RichBlockKind::Quote,
                list: None,
            });
        }

        let last_end = segments.last()?.end;
        if quote_end > last_end {
            if !self.only_quote_markers(last_end, quote_end) {
                return None;
            }
            if let Some(marker) = self.body[last_end..quote_end].rfind('>') {
                if let Some(last) = segments.last_mut() {
                    last.end = last_end + marker + 1;
                }
            }
        }
        self.gaps_are_whitespace(&segments).then_some(segments)
    }

    /// Adds the segments' blocks: the text before the first is an ordinary
    /// gap, the whitespace between them separators.  Returns where the last
    /// one ends.
    fn add_segments(&mut self, cursor: usize, segments: &[Segment]) -> usize {
        self.add_gap(cursor, segments[0].start);
        // The segments of one call are one top-level list or one blockquote.
        let count = segments.len();
        let list_group = (segments[0].kind == RichBlockKind::ListItem).then(|| {
            shared(RichListGroup {
                original_member_count: count,
            })
        });
        let quote_group = (segments[0].kind == RichBlockKind::Quote).then(|| {
            shared(RichQuoteGroup {
                original_member_count: count,
            })
        });
        for (i, segment) in segments.iter().enumerate() {
            if i > 0 {
                let previous_end = segments[i - 1].end;
                self.pending_separator
                    .push_str(&self.body[previous_end..segment.start]);
            }
            let mut block = self.build_block(segment.paragraph, segment.start, segment.end);
            block.kind = segment.kind;
            block.list = segment.list.clone();
            block.list_group = list_group.clone();
            block.quote_group = quote_group.clone();
            self.document.blocks.push(block);
        }
        segments[count - 1].end
    }

    fn gaps_are_whitespace(&self, segments: &[Segment]) -> bool {
        segments.windows(2).all(|pair| {
            let (gap_start, gap_end) = (pair[0].end, pair[1].start);
            gap_end >= gap_start && self.body[gap_start..gap_end].trim().is_empty()
        })
    }

    fn only_quote_markers(&self, start: usize, end: usize) -> bool {
        self.body[start..end]
            .chars()
            .all(|c| c == '>' || c.is_whitespace())
    }

    fn skip_spaces(&self, mut position: usize) -> usize {
        let bytes = self.body.as_bytes();
        while position < bytes.len() && (bytes[position] == b' ' || bytes[position] == b'\t') {
            position += 1;
        }
        position
    }

    /// The end of the line holding `position`, before its line break.
    fn line_end(&self, position: usize) -> usize {
        let end = self.body[position..]
            .find('\n')
            .map_or(self.body.len(), |i| i + position);
        if end > position && self.body.as_bytes()[end - 1] == b'\r' {
            end - 1
        } else {
            end
        }
    }

    /// True for an item with no text of its own: a bare marker, a marker whose
    /// content starts on the next line with a nested list, or the writer's form
    /// for an empty first item of a nested list, a marker followed by
    /// [`EMPTY_ITEM_COMMENT`] (a bare marker there would read as its parent's
    /// text).
    fn is_empty_item(&self, item: &Node, item_start: usize) -> bool {
        let Some(first) = item.children.first() else {
            return true;
        };
        let line_end = self.line_end(item_start);
        if matches!(first.kind, NodeKind::List { .. }) {
            return first.range.start > line_end;
        }
        let html_end = trim_trailing_blank_lines(self.body, first.range.clone());
        first.kind == NodeKind::HtmlBlock
            && (item.children.len() == 1 || matches!(item.children[1].kind, NodeKind::List { .. }))
            && first.range.start < line_end
            && self.body[item_start..line_end]
                .trim_end()
                .ends_with(&format!(" {EMPTY_ITEM_COMMENT}"))
            && self.body[first.range.start..html_end].trim() == EMPTY_ITEM_COMMENT
    }

    /// Text between blocks.  Whitespace becomes a separator; anything else
    /// left outside every block (link reference definitions) becomes a Raw
    /// block so it stays visible and survives edits around it.
    fn add_gap(&mut self, start: usize, end: usize) {
        let gap = &self.body[start..end];
        let Some(content_start) = gap.find(|c: char| !c.is_whitespace()) else {
            self.pending_separator.push_str(gap);
            return;
        };
        // Indentation on the content's first line belongs to the content, not the separator.
        let line_start = gap[..content_start].rfind('\n').map_or(0, |i| i + 1);
        let content_end = gap.trim_end().len();
        let mut block = RichBlock::new(RichBlockKind::Raw);
        block.separator_before = std::mem::take(&mut self.pending_separator) + &gap[..line_start];
        block.original_source = gap[line_start..content_end].to_string();
        self.document.blocks.push(block);
        self.html_blocks.push(false);
        self.pending_separator = gap[content_end..].to_string();
    }

    fn build_block(&mut self, node: Option<&Node>, start: usize, end: usize) -> RichBlock {
        let mut result = RichBlock::new(RichBlockKind::Raw);
        result.separator_before = std::mem::take(&mut self.pending_separator);
        result.original_source = self.body[start..end].to_string();
        self.html_blocks
            .push(node.is_some_and(|n| n.kind == NodeKind::HtmlBlock));
        // An empty list item has no paragraph; add_segments makes it a ListItem with no inlines.
        let Some(node) = node else {
            return result;
        };
        match node.kind {
            // A grid table, which the viewer reads and pulldown-cmark takes for a paragraph,
            // stays Raw.
            NodeKind::Paragraph if code_and_tables::is_grid_table(&result.original_source) => {}
            NodeKind::Heading(level) => {
                result.inlines = self.reader.inlines_of(node);
                result.kind = RichBlockKind::Heading;
                result.heading_level = level;
            }
            NodeKind::CodeBlock { .. } => {
                code_and_tables::read_code(node, self.body, start, end, &mut result)
            }
            // A pipe table is Markdig's verdict on the source, whichever block pulldown-cmark
            // read: a paragraph GFM refused (a pipe in a code span, header and delimiter rows
            // of different widths) can be a table, and a GFM table can be text.
            NodeKind::Paragraph | NodeKind::Table(_) => {
                if let Some(found) = pipe_tables::find(&result.original_source)
                    .filter(|found| found.header_line == 0)
                {
                    code_and_tables::read_table(
                        &found.alignments,
                        &mut result,
                        self.reader.soft_breaks_are_hard,
                    );
                } else if node.is_paragraph() {
                    result.inlines = self.reader.inlines_of(node);
                    result.kind = RichBlockKind::Paragraph;
                }
            }
            _ => {}
        }
        result
    }
}

/// `range`'s end moved back over its last line break and any blank lines
/// before that, to the end of its last content line.
pub(super) fn trim_trailing_blank_lines(body: &str, range: Range<usize>) -> usize {
    let text = &body[range.clone()];
    let mut end = text.len();
    loop {
        let content_end = text[..end].trim_end_matches(['\n', '\r']).len();
        let line_start = text[..content_end].rfind('\n').map_or(0, |i| i + 1);
        if line_start == 0 || !text[line_start..content_end].trim().is_empty() {
            return range.start + content_end;
        }
        end = line_start;
    }
}

/// The frontmatter by the viewer's convention: a first line of `---` through
/// the next `---` line and its newline; `""` when there is none.
fn frontmatter_of(markdown: &str) -> &str {
    let Some(first_line_end) = markdown.find('\n') else {
        return "";
    };
    if markdown[..first_line_end].trim() != "---" {
        return "";
    }
    let mut line_start = first_line_end + 1;
    while line_start < markdown.len() {
        let line_end = markdown[line_start..].find('\n').map(|i| i + line_start);
        let content_end = line_end.unwrap_or(markdown.len());
        let next = line_end.map_or(markdown.len(), |e| e + 1);
        if markdown[line_start..content_end].trim() == "---" {
            return &markdown[..next];
        }
        line_start = next;
    }
    ""
}
