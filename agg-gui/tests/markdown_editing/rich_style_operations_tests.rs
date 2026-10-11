//! agg-sharp's `RichStyleOperationsTests`
//! (`Tests/Agg.Tests/MarkdigAgg/RichStyleOperationsTests.cs`), ported 1:1:
//! toggling bold/italic/code/strike over ranges, the toolbar's style state,
//! pending styles at a caret, and setting and removing links.

use agg_gui::widgets::markdown_editing::edit_operations;
use agg_gui::widgets::markdown_editing::markdown_parser::parse;
use agg_gui::widgets::markdown_editing::markdown_writer::write;
use agg_gui::widgets::markdown_editing::style_operations as style_ops;
use agg_gui::widgets::markdown_editing::{
    DocPosition, RichBlock, RichBlockKind, RichDocument, RichInline, RichInlineStyle,
    RichPendingStyle, RichSelection, RichStyleCoverage, RichStyleState,
};

fn range(block: usize, from: usize, to: usize) -> RichSelection {
    RichSelection::new(DocPosition::new(block, from), DocPosition::new(block, to))
}

fn caret(block: usize, offset: usize) -> RichSelection {
    RichSelection::at(DocPosition::new(block, offset))
}

/// A block's runs as "text" or "text:flags" joined by '|', flags B I C S and @url for a link. Asserting on
/// the model rather than written markdown keeps these tests independent of the inline writer.
fn runs(block: &RichBlock) -> String {
    runs_of(&block.inlines)
}

/// As [`runs`] for any inline list (a table cell's); an atom shows as "[atom]".
fn runs_of(inlines: &[RichInline]) -> String {
    inlines
        .iter()
        .map(|inline| {
            let code = inline.as_run().is_some_and(|run| run.code);
            let mut flags = String::new();
            flags += if inline.bold { "B" } else { "" };
            flags += if inline.italic { "I" } else { "" };
            flags += if code { "C" } else { "" };
            flags += if inline.strike { "S" } else { "" };
            if let Some(url) = &inline.link_url {
                flags += "@";
                flags += url;
            }
            let text = inline.text().unwrap_or("[atom]");
            if flags.is_empty() {
                text.to_string()
            } else {
                format!("{text}:{flags}")
            }
        })
        .collect::<Vec<_>>()
        .join("|")
}

/// Writes the edited document and reads it back, so a test can assert the styles survive in markdown.
fn round_trip(document: &RichDocument) -> (String, RichDocument) {
    let markdown = write(document);
    let reread = parse(&markdown);
    (markdown, reread)
}

fn state(document: &RichDocument, selection: RichSelection) -> RichStyleState {
    style_ops::style_at(document, selection, RichPendingStyle::default()).unwrap()
}

#[test]
fn toggle_bold_mid_word_splits_the_run() {
    let mut document = parse("abcdef\n");
    let selection = range(0, 2, 4);

    let result = style_ops::toggle_style(&mut document, selection, RichInlineStyle::BOLD).unwrap();

    assert_eq!(result, selection);
    assert!(document.blocks[0].dirty);
    assert_eq!(runs(&document.blocks[0]), "ab|cd:B|ef");
}

#[test]
fn toggle_removes_when_all_have_it_and_merges_back() {
    let mut document = parse("ab**cd**ef\n");

    style_ops::toggle_style(&mut document, range(0, 2, 4), RichInlineStyle::BOLD).unwrap();

    // One plain run again, not three that happen to share a style.
    assert_eq!(document.blocks[0].inlines.len(), 1);
    assert_eq!(runs(&document.blocks[0]), "abcdef");
}

#[test]
fn mixed_selection_applies_to_all_without_fragments() {
    let mut document = parse("ab**cd**ef\n");

    style_ops::toggle_style(&mut document, range(0, 0, 6), RichInlineStyle::BOLD).unwrap();

    assert_eq!(document.blocks[0].inlines.len(), 1);
    assert_eq!(runs(&document.blocks[0]), "abcdef:B");
}

#[test]
fn across_paragraphs_skips_raw_block() {
    let mut document = parse("one\n\n---\n\ntwo\n");
    assert_eq!(document.blocks[1].kind, RichBlockKind::Raw);
    let selection = RichSelection::new(DocPosition::new(0, 1), DocPosition::new(2, 2));

    style_ops::toggle_style(&mut document, selection, RichInlineStyle::ITALIC).unwrap();

    assert!(!document.blocks[1].dirty);
    assert_eq!(
        format!(
            "{}/{}",
            runs(&document.blocks[0]),
            runs(&document.blocks[2])
        ),
        "o|ne:I/tw:I|o"
    );
}

#[test]
fn inside_one_table_cell() {
    let mut document = parse("| a | b |\n|---|---|\n| cell | x |\n");
    assert_eq!(document.blocks[0].kind, RichBlockKind::Table);
    let selection = RichSelection::new(
        DocPosition::in_cell(0, 0, 1, 0),
        DocPosition::in_cell(0, 4, 1, 0),
    );

    style_ops::toggle_style(&mut document, selection, RichInlineStyle::STRIKE).unwrap();

    let table = &document.blocks[0];
    let cell = &table.table_rows[1][0].inlines;
    assert_eq!(cell.len(), 1);
    assert!(cell[0].strike);
    assert_eq!(cell[0].text(), Some("cell"));
    assert!(table.dirty);
}

#[test]
fn style_at_reports_all_none_and_mixed() {
    let document = parse("ab***cd***ef\n");

    let mixed = state(&document, range(0, 0, 4));
    assert_eq!(
        mixed.coverage(RichInlineStyle::BOLD),
        RichStyleCoverage::Mixed
    );

    let all = state(&document, range(0, 2, 4));
    assert_eq!(all.coverage(RichInlineStyle::BOLD), RichStyleCoverage::All);
    assert_eq!(
        all.coverage(RichInlineStyle::ITALIC),
        RichStyleCoverage::All
    );
    assert_eq!(all.coverage(RichInlineStyle::CODE), RichStyleCoverage::None);

    let none = state(&document, range(0, 4, 6));
    assert_eq!(
        none.coverage(RichInlineStyle::BOLD),
        RichStyleCoverage::None
    );
    assert!(!document.blocks[0].dirty);
}

#[test]
fn style_at_caret_follows_typing_rule_and_link_range() {
    let document = parse("**ab** [link](http://x) c\n");

    let inside_bold = state(&document, caret(0, 2));
    assert_eq!(
        inside_bold.coverage(RichInlineStyle::BOLD),
        RichStyleCoverage::All
    );

    let inside_link = state(&document, caret(0, 5));
    assert_eq!(inside_link.link_url.as_deref(), Some("http://x"));

    // A caret just after a link shows it (so the link button edits it), though typing there does not
    // extend it.
    let after_link = state(&document, caret(0, 7));
    assert_eq!(after_link.link_url.as_deref(), Some("http://x"));
    let typing = style_ops::typing_style(&document, DocPosition::new(0, 7)).unwrap();
    assert_eq!(typing.link_url, None);

    let whole_link = state(&document, range(0, 3, 7));
    assert_eq!(whole_link.link_url.as_deref(), Some("http://x"));
    let part_link = state(&document, range(0, 2, 7));
    assert_eq!(part_link.link_url, None);
}

#[test]
fn pending_style_makes_typed_text_bold() {
    let mut document = parse("ab\n");
    let pending = RichPendingStyle::default().toggle(RichInlineStyle::BOLD);
    let position = DocPosition::new(0, 2);

    let shown = style_ops::style_at(&document, RichSelection::at(position), pending).unwrap();
    assert_eq!(
        shown.coverage(RichInlineStyle::BOLD),
        RichStyleCoverage::All
    );

    let style = pending.apply_pending(&style_ops::typing_style(&document, position).unwrap());
    edit_operations::insert_text(&mut document, position, "cd", Some(&style)).unwrap();

    assert_eq!(runs(&document.blocks[0]), "ab|cd:B");
    assert!(pending.toggle(RichInlineStyle::BOLD).is_empty());
}

#[test]
fn set_and_remove_link_on_range() {
    let mut document = parse("see here now\n");

    style_ops::set_link(&mut document, range(0, 4, 8), "http://a", None).unwrap();
    assert_eq!(runs(&document.blocks[0]), "see |here:@http://a| now");

    style_ops::remove_link(&mut document, range(0, 4, 8)).unwrap();
    assert_eq!(document.blocks[0].inlines.len(), 1);
    assert_eq!(runs(&document.blocks[0]), "see here now");
}

#[test]
fn collapsed_caret_link_ops() {
    let mut document = parse("go [a **b** c][ref] end\n\n[ref]: http://old\n");

    // Set on a caret inside a link retargets the whole link and makes it inline.
    style_ops::set_link(&mut document, caret(0, 4), "http://new", None).unwrap();
    let links: Vec<&RichInline> = document.blocks[0]
        .inlines
        .iter()
        .filter(|r| r.is_run() && r.link_url.is_some())
        .collect();
    assert_eq!(links.len(), 3);
    assert!(links
        .iter()
        .all(|r| r.link_url.as_deref() == Some("http://new") && r.link_label.is_none()));

    // Remove on a caret inside a link removes the whole link, whatever runs it spans.
    style_ops::remove_link(&mut document, caret(0, 4)).unwrap();
    assert!(!document.blocks[0]
        .inlines
        .iter()
        .any(|r| r.is_run() && r.link_url.is_some()));

    // Set on a caret with no link types the url as linked text.
    let mut plain = parse("x\n");
    let result = style_ops::set_link(&mut plain, caret(0, 1), "http://u", None).unwrap();
    assert_eq!(result, caret(0, 9));
    assert!(plain.blocks[0].dirty);
    assert_eq!(runs(&plain.blocks[0]), "x|http://u:@http://u");
}

#[test]
fn bold_mid_word_survives_write_and_read() {
    let mut document = parse("abcdef\n");

    style_ops::toggle_style(&mut document, range(0, 2, 4), RichInlineStyle::BOLD).unwrap();

    let (_, reread) = round_trip(&document);
    assert_eq!(runs(&reread.blocks[0]), "ab|cd:B|ef");
}

#[test]
fn bold_over_an_image_writes_one_bold_span() {
    let mut document = parse("a ![i](p.png) b\n");

    let before = state(&document, range(0, 0, 5));
    assert_eq!(
        before.coverage(RichInlineStyle::BOLD),
        RichStyleCoverage::None
    );
    style_ops::toggle_style(&mut document, range(0, 0, 5), RichInlineStyle::BOLD).unwrap();
    assert_eq!(runs(&document.blocks[0]), "a :B|[atom]:B| b:B");
    let after = state(&document, range(0, 0, 5));
    assert_eq!(
        after.coverage(RichInlineStyle::BOLD),
        RichStyleCoverage::All
    );

    let (markdown, reread) = round_trip(&document);
    assert_eq!(markdown, "**a ![i](p.png) b**\n");
    assert_eq!(runs(&reread.blocks[0]), "a :B|[atom]:B| b:B");

    // Toggling again over all-bold text and image removes it from both.
    style_ops::toggle_style(&mut document, range(0, 0, 5), RichInlineStyle::BOLD).unwrap();
    assert_eq!(runs(&document.blocks[0]), "a |[atom]| b");
}

#[test]
fn code_leaves_an_image_alone() {
    let mut document = parse("![i](p.png)\n");

    style_ops::toggle_style(&mut document, range(0, 0, 1), RichInlineStyle::CODE).unwrap();

    assert_eq!(runs(&document.blocks[0]), "[atom]");
    assert!(!document.blocks[0].dirty);
    let shown = state(&document, range(0, 0, 1));
    assert_eq!(
        shown.coverage(RichInlineStyle::CODE),
        RichStyleCoverage::None
    );
}

#[test]
fn link_over_styled_text_survives_write_and_read() {
    let mut document = parse("x **bold** y\n");

    style_ops::set_link(&mut document, range(0, 0, 8), "http://a", None).unwrap();

    let (_, reread) = round_trip(&document);
    assert_eq!(
        runs(&reread.blocks[0]),
        "x :@http://a|bold:B@http://a| y:@http://a"
    );
}

#[test]
fn link_over_an_image_keeps_one_link() {
    let mut document = parse("a ![i](p.png) b\n");

    style_ops::set_link(&mut document, range(0, 0, 5), "http://a", None).unwrap();

    let (markdown, mut reread) = round_trip(&document);
    assert_eq!(markdown, "[a ![i](p.png) b](http://a)\n");
    assert_eq!(
        runs(&reread.blocks[0]),
        "a :@http://a|[atom]:@http://a| b:@http://a"
    );
    let whole = state(&reread, range(0, 0, 5));
    assert_eq!(whole.link_url.as_deref(), Some("http://a"));

    // A caret in the link removes all of it, image included.
    style_ops::remove_link(&mut reread, caret(0, 1)).unwrap();
    assert_eq!(runs(&reread.blocks[0]), "a |[atom]| b");
}

#[test]
fn bold_across_table_cells_styles_each_cell_in_reading_order() {
    let mut document = parse("| a | b |\n|---|---|\n| cell | x |\n| y | z |\n");
    let selection = RichSelection::new(
        DocPosition::in_cell(0, 2, 1, 0),
        DocPosition::in_cell(0, 1, 2, 0),
    );

    style_ops::toggle_style(&mut document, selection, RichInlineStyle::BOLD).unwrap();

    let (_, reread) = round_trip(&document);
    let rows = &reread.blocks[0].table_rows;
    assert_eq!(runs_of(&rows[0][1].inlines), "b");
    assert_eq!(runs_of(&rows[1][0].inlines), "ce|ll:B");
    assert_eq!(runs_of(&rows[1][1].inlines), "x:B");
    assert_eq!(runs_of(&rows[2][0].inlines), "y:B");
    assert_eq!(runs_of(&rows[2][1].inlines), "z");

    let shown = state(&document, selection);
    assert_eq!(
        shown.coverage(RichInlineStyle::BOLD),
        RichStyleCoverage::All
    );
}

#[test]
fn link_across_table_cells_links_each_cell() {
    let mut document = parse("| a | b |\n|---|---|\n| c | d |\n");
    let selection = RichSelection::new(
        DocPosition::in_cell(0, 0, 1, 0),
        DocPosition::in_cell(0, 1, 1, 1),
    );

    style_ops::set_link(&mut document, selection, "http://a", None).unwrap();

    let (_, reread) = round_trip(&document);
    let row = &reread.blocks[0].table_rows[1];
    assert_eq!(
        format!("{}/{}", runs_of(&row[0].inlines), runs_of(&row[1].inlines)),
        "c:@http://a/d:@http://a"
    );
}

#[test]
fn empty_document() {
    let mut document = parse("");
    assert_eq!(document.blocks.len(), 0);

    assert_eq!(state(&document, caret(0, 0)), RichStyleState::default());
    let typing = style_ops::typing_style(&document, DocPosition::new(0, 0)).unwrap();
    assert_eq!(runs_of(&[typing.with_text("t")]), "t");
    assert_eq!(
        style_ops::remove_link(&mut document, caret(0, 0)).unwrap(),
        caret(0, 0)
    );
    assert_eq!(document.blocks.len(), 0);

    let result = style_ops::set_link(&mut document, caret(0, 0), "http://u", None).unwrap();
    assert_eq!(result, caret(0, 8));
    assert_eq!(runs(&document.blocks[0]), "http://u:@http://u");
}

#[test]
fn caret_at_either_edge_of_link_shows_it() {
    let document = parse("[link](http://x \"T\") c\n");

    let at_end = state(&document, caret(0, 4));
    assert_eq!(at_end.link_url.as_deref(), Some("http://x"));
    assert_eq!(at_end.link_title.as_deref(), Some("T"));
    assert_eq!(
        state(&document, caret(0, 0)).link_url.as_deref(),
        Some("http://x")
    );
    assert_eq!(state(&document, caret(0, 6)).link_url, None);
}

#[test]
fn retarget_from_caret_keeps_title() {
    let mut document = parse("[a](http://old \"T\")\n");

    style_ops::set_link(&mut document, caret(0, 1), "http://new", None).unwrap();

    let inlines = &document.blocks[0].inlines;
    assert_eq!(inlines.len(), 1);
    assert_eq!(inlines[0].link_url.as_deref(), Some("http://new"));
    assert_eq!(inlines[0].link_title.as_deref(), Some("T"));
}

#[test]
fn caret_link_display_text_is_trimmed_one_line() {
    let mut document = parse("x\n");
    let url = "  http://u\n\tz ";

    let result = style_ops::set_link(&mut document, caret(0, 1), url, None).unwrap();

    assert_eq!(result, caret(0, 12));
    assert_eq!(document.blocks.len(), 1);
    let link = &document.blocks[0].inlines[1];
    assert_eq!(link.text(), Some("http://u  z"));
    assert_eq!(link.link_url.as_deref(), Some(url));
}

#[test]
fn caret_link_inside_code_is_not_code() {
    let mut document = parse("`abc`\n");

    style_ops::set_link(&mut document, caret(0, 1), "http://u", None).unwrap();

    assert_eq!(runs(&document.blocks[0]), "a:C|http://u:@http://u|bc:C");
}

#[test]
fn no_change_leaves_block_clean() {
    let mut document = parse("ab cd\n");

    style_ops::remove_link(&mut document, range(0, 0, 5)).unwrap();

    assert!(!document.blocks[0].dirty);
    assert_eq!(runs(&document.blocks[0]), "ab cd");
}
