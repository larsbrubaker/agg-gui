//! pulldown-cmark's event stream folded into a tree of nodes with their source
//! ranges, the shape [`super::markdown_parser`] walks the way agg-sharp's
//! `RichMarkdownParser` walks Markdig's syntax tree.
//!
//! pulldown-cmark writes a tight list item's text straight into the item, with
//! no paragraph around it; the tree gives that run of inlines an implicit
//! paragraph node so every item reads like Markdig's (a `ParagraphBlock`
//! first), and remembers which paragraphs were written so a list knows it is
//! loose.

use std::ops::Range;

use pulldown_cmark::{Alignment, CodeBlockKind, Event, LinkType, Options, Parser, Tag};

/// What a [`Node`] is.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum NodeKind {
    /// The document root.
    Root,
    Paragraph,
    Heading(u8),
    /// A list: its start number when ordered, and whether it is loose.
    List {
        start: Option<u64>,
        loose: bool,
    },
    Item,
    Quote,
    /// A code block, fenced or indented.
    CodeBlock {
        fenced: bool,
    },
    HtmlBlock,
    Table(Vec<Alignment>),
    TableHead,
    TableRow,
    TableCell,
    /// Any other block (thematic break, footnote definition, metadata).
    OtherBlock,
    /// Inline text with its decoded content.
    Text(String),
    /// A code span with its content.
    Code(String),
    InlineHtml,
    SoftBreak,
    HardBreak,
    Emphasis,
    Strong,
    Strikethrough,
    Link {
        link_type: LinkType,
        url: String,
        title: String,
        id: String,
    },
    Image,
    TaskMarker,
    /// Any other inline (footnote reference, math).
    OtherInline,
}

/// One node: its kind, its source range and its children in source order.
#[derive(Clone, Debug)]
pub(super) struct Node {
    pub kind: NodeKind,
    pub range: Range<usize>,
    pub children: Vec<Node>,
    /// True for the paragraph made around a tight list item's text.
    pub implicit: bool,
}

impl Node {
    fn new(kind: NodeKind, range: Range<usize>) -> Self {
        Self {
            kind,
            range,
            children: Vec::new(),
            implicit: false,
        }
    }

    /// True for the inline kinds.
    pub fn is_inline(&self) -> bool {
        matches!(
            self.kind,
            NodeKind::Text(_)
                | NodeKind::Code(_)
                | NodeKind::InlineHtml
                | NodeKind::SoftBreak
                | NodeKind::HardBreak
                | NodeKind::Emphasis
                | NodeKind::Strong
                | NodeKind::Strikethrough
                | NodeKind::Link { .. }
                | NodeKind::Image
                | NodeKind::TaskMarker
                | NodeKind::OtherInline
        )
    }

    /// True for a paragraph, written or implicit.
    pub fn is_paragraph(&self) -> bool {
        self.kind == NodeKind::Paragraph
    }

    /// The source the node's inline children cover: from the first one's
    /// start to the furthest end.  Markdig's paragraph spans run the same way,
    /// from the first text to the last.
    ///
    /// pulldown-cmark's range for an escaped character leaves out its
    /// backslash, so a backslash just before the first inline is taken in.
    pub fn inline_extent(&self, source: &str) -> Range<usize> {
        let mut start = self
            .children
            .first()
            .map_or(self.range.start, |c| c.range.start);
        if start > 0 && source.as_bytes()[start - 1] == b'\\' {
            start -= 1;
        }
        let end = self
            .children
            .iter()
            .map(|c| c.range.end)
            .max()
            .unwrap_or(self.range.end);
        start..end.max(start)
    }
}

/// The options matching the viewer's: tables, strikethrough and task lists.
pub(super) fn options() -> Options {
    Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS
}

/// Parses `source` into its node tree.
pub(super) fn parse(source: &str) -> Node {
    let mut stack = vec![Node::new(NodeKind::Root, 0..source.len())];
    for (event, range) in Parser::new_ext(source, options()).into_offset_iter() {
        match event {
            Event::Start(tag) => stack.push(Node::new(kind_of(tag), range)),
            Event::End(_) => {
                let Some(mut node) = stack.pop() else { break };
                finish(&mut node, source);
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(node);
                }
            }
            other => {
                let kind = match other {
                    Event::Text(text) => NodeKind::Text(text.to_string()),
                    Event::Code(code) => NodeKind::Code(code.to_string()),
                    Event::Html(_) => NodeKind::OtherBlock,
                    Event::InlineHtml(_) => NodeKind::InlineHtml,
                    Event::SoftBreak => NodeKind::SoftBreak,
                    Event::HardBreak => NodeKind::HardBreak,
                    Event::Rule => NodeKind::OtherBlock,
                    Event::TaskListMarker(_) => NodeKind::TaskMarker,
                    _ => NodeKind::OtherInline,
                };
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(Node::new(kind, range));
                }
            }
        }
    }
    let mut root = stack.swap_remove(0);
    finish(&mut root, source);
    root
}

fn kind_of(tag: Tag) -> NodeKind {
    match tag {
        Tag::Paragraph => NodeKind::Paragraph,
        Tag::Heading { level, .. } => NodeKind::Heading(level as u8),
        Tag::BlockQuote(_) => NodeKind::Quote,
        Tag::CodeBlock(kind) => NodeKind::CodeBlock {
            fenced: matches!(kind, CodeBlockKind::Fenced(_)),
        },
        Tag::HtmlBlock => NodeKind::HtmlBlock,
        Tag::List(start) => NodeKind::List {
            start,
            loose: false,
        },
        Tag::Item => NodeKind::Item,
        Tag::Table(alignments) => NodeKind::Table(alignments),
        Tag::TableHead => NodeKind::TableHead,
        Tag::TableRow => NodeKind::TableRow,
        Tag::TableCell => NodeKind::TableCell,
        Tag::Emphasis => NodeKind::Emphasis,
        Tag::Strong => NodeKind::Strong,
        Tag::Strikethrough => NodeKind::Strikethrough,
        Tag::Link {
            link_type,
            dest_url,
            title,
            id,
        } => NodeKind::Link {
            link_type,
            url: dest_url.to_string(),
            title: title.to_string(),
            id: id.to_string(),
        },
        Tag::Image { .. } => NodeKind::Image,
        _ => NodeKind::OtherBlock,
    }
}

/// Wraps a tight item's inline runs in implicit paragraphs, gives paragraphs
/// their inline extent, and marks a list loose when an item wrote one.
fn finish(node: &mut Node, source: &str) {
    match node.kind {
        NodeKind::Item => {
            let children = std::mem::take(&mut node.children);
            for child in children {
                if child.is_inline() {
                    match node.children.last_mut() {
                        Some(last) if last.implicit => last.children.push(child),
                        _ => {
                            let mut paragraph = Node::new(NodeKind::Paragraph, child.range.clone());
                            paragraph.implicit = true;
                            paragraph.children.push(child);
                            node.children.push(paragraph);
                        }
                    }
                } else {
                    node.children.push(child);
                }
            }
            for child in node.children.iter_mut().filter(|c| c.implicit) {
                child.range = child.inline_extent(source);
            }
        }
        NodeKind::Paragraph if !node.children.is_empty() => node.range = node.inline_extent(source),
        NodeKind::List { ref mut loose, .. } => {
            *loose = node.children.iter().any(|item| {
                item.children
                    .iter()
                    .any(|child| child.is_paragraph() && !child.implicit)
            });
        }
        _ => {}
    }
}
