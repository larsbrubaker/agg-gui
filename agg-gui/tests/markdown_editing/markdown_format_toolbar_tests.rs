//! agg-sharp's `MarkdownFormatToolbarTests.cs`, ported 1:1 against
//! `MarkdownFormatToolbar` over the widget-free `RichDocumentCommands`.
//!
//! The toolbar does not hold its commands in Rust (see `format_toolbar`'s
//! header): C#'s `InvokeClick()` is `click(kind, &mut commands)`, moving the
//! caret is `set_selection` followed by the `sync` a host runs after the
//! commands' handlers fire, and C#'s `Commands = other` is `attach`.  C#
//! counted `FocusText` calls in an override; `RichDocumentCommands` counts
//! them itself (`focus_requests`).  Layout is in agg-gui's logical units, so
//! the room-above-and-below test expects 6 units at every device scale where
//! C# expected `6 * DeviceScale` device pixels.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::event::{Event, Key, Modifiers};
use agg_gui::text::Font;
use agg_gui::widgets::markdown_editing::{
    markdown_parser, markdown_writer, with_scheme, DocPosition, MarkdownFormatButtonKind as K,
    MarkdownFormatGroup, MarkdownFormatToolbar, RichAlignment, RichBlockKind, RichDocumentCommands,
    RichEditCommands, RichInlineStyle, RichSelection, RichStyleCoverage,
};
use agg_gui::{Rect, Size, Widget};

use super::rich_markdown_edit_widget_tests::fonts;

struct Fixture {
    toolbar: MarkdownFormatToolbar,
    commands: RichDocumentCommands,
    changes: Rc<Cell<usize>>,
}

impl Fixture {
    fn click(&mut self, kind: K) {
        self.toolbar.click(kind, &mut self.commands).unwrap();
    }

    // Moves the caret as a click would; the host then lets the toolbar follow.
    fn select(&mut self, selection: RichSelection) {
        self.commands.set_selection(selection);
        self.toolbar.sync(&mut self.commands).unwrap();
    }

    fn pressed(&self, kind: K) -> bool {
        self.toolbar.button(kind).is_pressed()
    }

    fn enabled(&self, kind: K) -> bool {
        self.toolbar.button(kind).enabled
    }

    fn markdown(&self) -> String {
        markdown_writer::write(self.commands.document())
    }
}

fn icons() -> Arc<Font> {
    Arc::new(Font::from_slice(agg_gui::fonts::FONT_AWESOME_4_7).expect("font awesome"))
}

fn make(markdown: &str, selection: RichSelection) -> Fixture {
    let mut commands = RichDocumentCommands::new(markdown_parser::parse(markdown), selection);
    let changes = Rc::new(Cell::new(0));
    let counter = Rc::clone(&changes);
    commands.on_document_changed(Box::new(move || counter.set(counter.get() + 1)));
    let mut toolbar = MarkdownFormatToolbar::new(fonts().regular, icons(), false);
    toolbar.attach(&mut commands);
    Fixture {
        toolbar,
        commands,
        changes,
    }
}

fn range(block: usize, from: usize, to: usize) -> RichSelection {
    RichSelection::new(DocPosition::new(block, from), DocPosition::new(block, to))
}

fn caret(block: usize, offset: usize) -> RichSelection {
    RichSelection::at(DocPosition::new(block, offset))
}

#[test]
fn bold_over_a_selection_bolds_it() {
    let mut f = make("Hello world\n", range(0, 0, 5));
    assert!(!f.pressed(K::Bold));

    f.click(K::Bold);

    assert_eq!(f.markdown(), "**Hello** world\n");
    assert!(f.pressed(K::Bold));
}

#[test]
fn bold_at_a_caret_flips_the_pending_style_only() {
    let mut f = make("Hello world\n", caret(0, 3));

    f.click(K::Bold);

    assert_eq!(f.commands.pending_style().flipped, RichInlineStyle::BOLD);
    assert!(f.pressed(K::Bold));
    assert_eq!(f.markdown(), "Hello world\n");

    // Moving the caret drops the pending style, and the button follows.
    f.select(caret(0, 1));
    assert!(!f.pressed(K::Bold));
}

#[test]
fn style_buttons_show_all_mixed_and_none() {
    let mut f = make("**ab**cd\n", range(0, 0, 2));
    assert_eq!(f.toolbar.button(K::Bold).coverage, RichStyleCoverage::All);

    f.select(range(0, 0, 4));
    assert_eq!(f.toolbar.button(K::Bold).coverage, RichStyleCoverage::Mixed);

    f.select(range(0, 2, 4));
    assert_eq!(f.toolbar.button(K::Bold).coverage, RichStyleCoverage::None);
}

#[test]
fn size_list_sets_heading_two() {
    let mut f = make("Hello world\n", caret(0, 2));
    assert_eq!(f.toolbar.size_index(), Some(0));

    f.toolbar.set_size_index(2, &mut f.commands).unwrap();

    assert_eq!(f.markdown(), "## Hello world\n");
}

#[test]
fn size_list_follows_the_selection() {
    let mut f = make("# Title\n\nBody\n", caret(1, 1));
    assert_eq!(f.toolbar.size_index(), Some(0));

    f.select(caret(0, 2));
    assert_eq!(f.toolbar.size_index(), Some(1));

    // Over a heading and a paragraph the size is mixed: blank, and the document is untouched.
    f.select(RichSelection::new(
        DocPosition::new(0, 1),
        DocPosition::new(1, 2),
    ));
    assert_eq!(f.toolbar.size_index(), None);
    assert_eq!(f.markdown(), "# Title\n\nBody\n");
}

#[test]
fn alignment_is_disabled_in_a_list() {
    let mut f = make("- one\n- two\n\nplain\n", caret(0, 1));
    assert!(!f.enabled(K::AlignCenter));
    assert!(f.pressed(K::BulletList));

    f.select(caret(2, 1));
    assert!(f.enabled(K::AlignCenter));
    assert!(f.pressed(K::AlignLeft));

    f.click(K::AlignCenter);
    assert!(f.pressed(K::AlignCenter));
    assert_eq!(
        f.commands.document().blocks[2].alignment,
        RichAlignment::Center
    );
}

#[test]
fn numbered_list_toggles_on_and_off() {
    let mut f = make("one\n", caret(0, 1));

    f.click(K::NumberedList);
    assert_eq!(f.markdown(), "1. one\n");
    assert!(f.pressed(K::NumberedList));
    assert!(!f.enabled(K::AlignLeft));

    f.click(K::NumberedList);
    assert_eq!(f.markdown(), "one\n");
}

#[test]
fn the_link_row_sets_and_removes_a_url() {
    let mut f = make("Hello world\n", range(0, 0, 5));

    f.click(K::Link);
    assert!(f.toolbar.link_editor().is_visible());
    assert_eq!(f.toolbar.link_editor().url(), "");

    f.toolbar.link_editor().set_url("https://example.com");
    f.toolbar.accept_link_editor(&mut f.commands).unwrap();
    assert!(!f.toolbar.link_editor().is_visible());
    assert_eq!(f.markdown(), "[Hello](https://example.com) world\n");
    assert!(f.pressed(K::Link));

    // Reopening edits that link; Remove keeps the text.
    f.click(K::Link);
    assert_eq!(f.toolbar.link_editor().url(), "https://example.com");
    f.toolbar.remove_link_editor(&mut f.commands).unwrap();
    assert_eq!(f.markdown(), "Hello world\n");
}

#[test]
fn a_host_dialog_replaces_the_link_row() {
    let mut f = make("[Hi](https://a.b) there\n", caret(0, 1));
    let requested = Rc::new(RefCell::new(None::<String>));
    let seen = Rc::clone(&requested);
    f.toolbar.on_link_requested(Box::new(move |url| {
        *seen.borrow_mut() = url.map(str::to_string)
    }));

    f.click(K::Link);

    assert_eq!(requested.borrow().as_deref(), Some("https://a.b"));
    assert!(!f.toolbar.link_editor().is_visible());
}

#[test]
fn table_inserts_a_two_by_two_table() {
    let mut f = make("Hello\n", caret(0, 5));

    f.click(K::Table);

    let table = &f.commands.document().blocks[1];
    assert_eq!(table.kind, RichBlockKind::Table);
    assert_eq!(table.table_rows.len(), 2);
    assert_eq!(table.table_rows[0].len(), 2);
    assert_eq!(
        f.commands.selection().caret,
        DocPosition::in_cell(1, 0, 0, 0)
    );
}

#[test]
fn code_block_round_trips() {
    let mut f = make(
        "one\n\n**two**\n\nafter\n",
        RichSelection::new(DocPosition::new(0, 1), DocPosition::new(1, 2)),
    );

    f.click(K::CodeBlock);

    assert_eq!(f.commands.document().blocks.len(), 2);
    assert_eq!(f.commands.document().blocks[0].code_text, "one\ntwo");
    assert_eq!(f.markdown(), "```\none\ntwo\n```\n\nafter\n");
    assert_eq!(
        f.commands.selection(),
        RichSelection::new(DocPosition::new(0, 1), DocPosition::new(0, 6))
    );
    assert!(f.pressed(K::CodeBlock));
    assert!(!f.enabled(K::Bold));

    f.click(K::CodeBlock);

    assert_eq!(f.markdown(), "one\n\ntwo\n\nafter\n");
    assert_eq!(
        f.commands.selection(),
        RichSelection::new(DocPosition::new(0, 1), DocPosition::new(1, 2))
    );
}

#[test]
fn code_block_from_a_list_item_leaves_the_list() {
    let mut f = make("- one\n- two\n- three\n", caret(1, 0));

    f.click(K::CodeBlock);

    assert_eq!(f.markdown(), "- one\n\n```\ntwo\n```\n\n- three\n");
}

#[test]
fn code_block_over_all_blocks_refreshes_without_crashing() {
    let mut f = make(
        "one\n\ntwo\n",
        RichSelection::new(DocPosition::new(0, 1), DocPosition::new(1, 2)),
    );

    f.click(K::CodeBlock);

    assert_eq!(f.markdown(), "```\none\ntwo\n```\n");
    assert!(f.pressed(K::CodeBlock));
}

#[test]
fn every_toolbar_action_gives_the_text_its_focus_back() {
    let mut f = make("Hello world\n", range(0, 0, 5));

    f.click(K::Bold);
    assert_eq!(f.commands.focus_requests(), 1);

    f.select(caret(0, 2));
    f.click(K::Italic);
    assert_eq!(f.commands.focus_requests(), 2);

    f.click(K::Link);
    f.toolbar.cancel_link_editor(&mut f.commands).unwrap();
    assert_eq!(f.commands.focus_requests(), 3);
}

#[test]
fn a_click_that_changes_nothing_reports_no_change() {
    let mut f = make("Hello\n", caret(0, 2));

    f.click(K::AlignLeft);
    assert_eq!(f.changes.get(), 0);

    f.click(K::AlignRight);
    assert_eq!(f.changes.get(), 1);
}

#[test]
fn code_block_is_disabled_where_it_cannot_apply() {
    let mut f = make("before\n\n| a | b |\n| - | - |\n| c | d |\n", caret(0, 1));
    assert!(f.enabled(K::CodeBlock));

    f.select(RichSelection::at(DocPosition::in_cell(1, 0, 1, 1)));
    assert!(!f.enabled(K::CodeBlock));

    f.select(RichSelection::new(
        DocPosition::new(0, 1),
        DocPosition::in_cell(1, 1, 0, 0),
    ));
    assert!(!f.enabled(K::CodeBlock));
}

#[test]
fn code_block_to_paragraphs_drops_blank_lines() {
    let mut f = make("```\na\n\nb\n```\n", caret(0, 2));

    f.click(K::CodeBlock);

    assert_eq!(f.commands.document().blocks.len(), 2);
    assert_eq!(f.markdown(), "a\n\nb\n");
    assert_eq!(f.commands.selection().caret, DocPosition::new(1, 0));
}

#[test]
fn link_row_escape_cancels_and_adds_a_scheme() {
    let mut f = make("Hello world\n", range(0, 0, 5));

    f.click(K::Link);
    let escape = Event::KeyDown {
        key: Key::Escape,
        modifiers: Modifiers::default(),
    };
    f.toolbar.children_mut()[1].on_event(&escape);
    f.toolbar.sync(&mut f.commands).unwrap();
    assert!(!f.toolbar.link_editor().is_visible());
    assert_eq!(f.markdown(), "Hello world\n");

    f.click(K::Link);
    f.toolbar.link_editor().set_url("example.com");
    f.toolbar.accept_link_editor(&mut f.commands).unwrap();
    assert_eq!(f.markdown(), "[Hello](https://example.com) world\n");

    assert_eq!(with_scheme("Fillets.md"), "Fillets.md");
    assert_eq!(with_scheme("mailto:a@b.c"), "mailto:a@b.c");
    assert_eq!(with_scheme("docs/page"), "docs/page");
}

#[test]
fn link_row_closes_when_the_caret_moves_or_the_document_is_swapped() {
    let mut f = make("Hello world\n", range(0, 0, 5));

    f.click(K::Link);
    f.select(caret(0, 8));
    assert!(!f.toolbar.link_editor().is_visible());

    f.click(K::Link);
    let mut other =
        RichDocumentCommands::new(markdown_parser::parse("Other\n"), RichSelection::default());
    f.toolbar.attach(&mut other);
    assert!(!f.toolbar.link_editor().is_visible());
}

#[test]
fn size_list_names_deep_headings() {
    let mut f = make("#### Deep\n\nBody\n", caret(0, 1));
    assert_eq!(f.toolbar.size_label(), "Heading 4");

    f.select(caret(1, 1));
    assert_eq!(f.toolbar.size_label(), "Normal");
    assert_eq!(f.toolbar.size_items().len(), 4);

    f.toolbar.set_size_index(3, &mut f.commands).unwrap();
    assert_eq!(f.markdown(), "#### Deep\n\n### Body\n");
}

/// Every widget under `widget` in paint order, with its box in the
/// coordinates of `widget`.
fn walk<'a>(widget: &'a dyn Widget, origin: (f64, f64), out: &mut Vec<(&'a dyn Widget, Rect)>) {
    for child in widget.children() {
        let b = child.bounds();
        let at = (origin.0 + b.x, origin.1 + b.y);
        out.push((child.as_ref(), Rect::new(at.0, at.1, b.width, b.height)));
        walk(child.as_ref(), at, out);
    }
}

fn laid_out(f: &mut Fixture) {
    f.toolbar.layout(Size::new(1200.0, 200.0));
    let height = f.toolbar.layout(Size::new(1200.0, 200.0)).height;
    f.toolbar.set_bounds(Rect::new(0.0, 0.0, 1200.0, height));
}

/// Related buttons share one outlined group, in reading order; the size list
/// stands alone between the first two, and each group and the list are items
/// of the wrapping strip.
#[test]
fn the_groups_hold_their_buttons_in_order() {
    let mut f = make("Hello world\n", caret(0, 1));
    let expected: [&[K]; 4] = [
        &[K::Bold, K::Italic, K::Strike, K::Code],
        &[K::AlignLeft, K::AlignCenter, K::AlignRight],
        &[K::BulletList, K::NumberedList, K::Quote],
        &[K::Link, K::CodeBlock, K::Table],
    ];
    laid_out(&mut f);

    let strip = &f.toolbar.children()[0];
    assert_eq!(strip.type_name(), "FlowLeftRightWithWrapping");
    // The strip's direct children are its rows; the items are the rows' children.
    let items: Vec<&dyn Widget> = strip
        .children()
        .iter()
        .flat_map(|row| row.children().iter().map(|c| c.as_ref()))
        .collect();
    assert_eq!(strip.children().len(), 1, "wide enough for one row");
    let groups: Vec<&MarkdownFormatGroup> = items
        .iter()
        .filter_map(|w| w.as_any()?.downcast_ref::<MarkdownFormatGroup>())
        .collect();
    assert_eq!(groups.len(), expected.len());
    for (group, kinds) in groups.iter().zip(expected) {
        assert_eq!(group.buttons(), kinds.to_vec());
        assert_eq!(group.children().len(), kinds.len());
    }

    let names: Vec<&str> = items.iter().map(|w| w.type_name()).collect();
    assert_eq!(names[0], "MarkdownFormatGroup");
    assert_eq!(names[1], "MarkdownSizeList");
}

/// The groups do not sit tight under whatever is above the toolbar: at least
/// six units above them, and as many below.  Checked at 1x and 2x.
#[test]
fn the_groups_have_room_above_and_below_them() {
    for scale in [1.0, 2.0] {
        agg_gui::device_scale::set_device_scale(scale);
        let mut f = make("Hello world\n", caret(0, 1));
        laid_out(&mut f);
        let height = f.toolbar.bounds().height;
        let mut all = Vec::new();
        walk(&f.toolbar, (0.0, 0.0), &mut all);
        let groups: Vec<Rect> = all
            .iter()
            .filter(|(w, _)| w.type_name() == "MarkdownFormatGroup")
            .map(|(_, r)| *r)
            .collect();
        assert_eq!(groups.len(), 4);
        for bounds in groups {
            let above = height - (bounds.y + bounds.height);
            let below = bounds.y;
            assert!(above >= 6.0 - 0.5, "{above} units above the groups");
            assert!((below - above).abs() <= 0.5, "{below} below, {above} above");
        }
        agg_gui::device_scale::set_device_scale(1.0);
    }
}
