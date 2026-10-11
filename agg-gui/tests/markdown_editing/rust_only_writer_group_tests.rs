//! Rust-only checks of `markdown_editing::markdown_writer`'s group paths
//! (verbatim copy, regenerated lists, quotes and alignment wrappers, source
//! offsets) on hand-built documents.  agg-sharp's `RichMarkdownWriterGroupTests`
//! build the same documents through the reader and the edit operations, so
//! they port with those; the expected markdown here is theirs.

use agg_gui::widgets::markdown_editing::markdown_writer;
use agg_gui::widgets::markdown_editing::{
    shared_group, RichAlignGroup, RichAlignment, RichBlock, RichBlockKind, RichDocument,
    RichInline, RichListGroup, RichListInfo, RichQuoteGroup,
};

/// A clean paragraph as the reader leaves it.
fn paragraph(text: &str, separator: &str) -> RichBlock {
    let mut block = RichBlock::new(RichBlockKind::Paragraph);
    block.inlines = vec![RichInline::run(text)];
    block.original_source = text.to_string();
    block.separator_before = separator.to_string();
    block
}

fn document(blocks: Vec<RichBlock>, trailing: &str) -> RichDocument {
    let mut document = RichDocument::new();
    document.blocks = blocks;
    document.trailing_text = trailing.to_string();
    document
}

#[test]
fn rust_only_unedited_document_writes_its_bytes() {
    let mut doc = document(vec![paragraph("A *b*", ""), paragraph("C", "\n\n\n")], "\n");
    doc.frontmatter = "---\nt: 1\n---\n".to_string();
    assert_eq!(
        markdown_writer::write(&doc),
        "---\nt: 1\n---\nA *b*\n\n\nC\n"
    );
}

#[test]
fn rust_only_bullets_on_two_paragraphs_write_one_tight_list() {
    // RichMarkdownWriterGroupTests.BulletsOnTwoParagraphsWriteOneTightList's document.
    let group = shared_group(RichListGroup::default());
    let mut blocks = vec![paragraph("A", ""), paragraph("B", "\n\n")];
    for block in &mut blocks {
        block.kind = RichBlockKind::ListItem;
        block.list = Some(RichListInfo::default());
        block.list_group = Some(group.clone());
        block.dirty = true;
    }
    assert_eq!(
        markdown_writer::write(&document(blocks, "\n")),
        "- A\n- B\n"
    );
}

#[test]
fn rust_only_numbered_list_keeps_its_start_and_nests_a_bullet() {
    // RichMarkdownWriterGroupTests.NumberedListKeepsItsStartAndNestsABullet's document.
    let group = shared_group(RichListGroup {
        original_member_count: 3,
    });
    let numbered = RichListInfo {
        ordered: true,
        marker: '.',
        start_number: 3,
        ..RichListInfo::default()
    };
    let bullet = RichListInfo {
        depth: 1,
        ..RichListInfo::default()
    };
    let mut blocks = vec![
        paragraph("a", ""),
        paragraph("b", "\n"),
        paragraph("c", "\n"),
    ];
    for (index, block) in blocks.iter_mut().enumerate() {
        block.kind = RichBlockKind::ListItem;
        block.original_source = format!("{}. {}", index + 3, block.original_source);
        block.list = Some(if index == 1 {
            bullet.clone()
        } else {
            numbered.clone()
        });
        block.list_group = Some(group.clone());
    }
    blocks[1].dirty = true;
    assert_eq!(
        markdown_writer::write(&document(blocks, "\n")),
        "3. a\n   - b\n4. c\n"
    );
}

#[test]
fn rust_only_crlf_quote_with_two_paragraphs() {
    // RichMarkdownWriterGroupTests.CrlfDocumentKeepsItsLineEnding's document.
    let group = shared_group(RichQuoteGroup::default());
    let mut blocks = vec![paragraph("A", ""), paragraph("B", "\r\n\r\n")];
    for block in &mut blocks {
        block.kind = RichBlockKind::Quote;
        block.quote_group = Some(group.clone());
        block.dirty = true;
    }
    assert_eq!(
        markdown_writer::write(&document(blocks, "\r\n")),
        "> A\r\n>\r\n> B\r\n"
    );
}

#[test]
fn rust_only_centered_heading_and_paragraph_share_one_wrapper() {
    // RichMarkdownWriterGroupTests.CenteredHeadingAndParagraphShareOneWrapper's document.
    let mut heading = paragraph("T", "");
    heading.kind = RichBlockKind::Heading;
    heading.heading_level = 1;
    heading.original_source = "# T".to_string();
    let mut blocks = vec![heading, paragraph("P", "\n\n")];
    for block in &mut blocks {
        block.alignment = RichAlignment::Center;
        block.dirty = true;
    }
    assert_eq!(
        markdown_writer::write(&document(blocks, "\n")),
        "<div align=\"center\">\n\n# T\n\nP\n\n</div>\n"
    );
}

#[test]
fn rust_only_unchanged_wrapper_keeps_its_tags_and_a_dropped_member_regenerates() {
    let group = shared_group(RichAlignGroup {
        original_member_count: 2,
        alignment: RichAlignment::Center,
        open_source: "<div align=\"center\">\n\n".to_string(),
        close_source: "\n\n</div>".to_string(),
        one_line_source: None,
    });
    let mut blocks = vec![
        paragraph("A", ""),
        paragraph("B", "\n\n"),
        paragraph("C", "\n\n"),
    ];
    for block in &mut blocks[..2] {
        block.alignment = RichAlignment::Center;
        block.align_group = Some(group.clone());
    }
    let mut doc = document(blocks, "\n");
    let source = "<div align=\"center\">\n\nA\n\nB\n\n</div>\n\nC\n";
    assert_eq!(markdown_writer::write(&doc), source);
    assert_eq!(
        markdown_writer::source_offset_of(&doc, 1),
        source.find('B').unwrap()
    );
    assert_eq!(
        markdown_writer::source_offset_of(&doc, 2),
        source.find('C').unwrap()
    );

    // RichMarkdownWriterGroupTests.DeletingTheFirstCenteredBlockLeavesNoOrphanTag's result.
    doc.blocks.remove(0);
    doc.blocks[0].separator_before = String::new();
    assert_eq!(
        markdown_writer::write(&doc),
        "<div align=\"center\">\n\nB\n\n</div>\n\nC\n"
    );
}
