//! agg-sharp's `RichMarkdownEditWidgetTests.cs`, ported 1:1 against
//! `RichMarkdownEditWidget`.  C# put the editor in a double-buffered
//! container and drew it into the container's back buffer; here the widget is
//! laid out to the container's size and painted through `paint_subtree` into
//! a software framebuffer (Y-up, as C#'s back buffer), with agg-gui's
//! viewport set to it so the Raw hosts' viewer wraps to their width.

use std::sync::Arc;

use agg_gui::geometry::Point;
use agg_gui::text::Font;
use agg_gui::widget::{paint_subtree, set_current_viewport};
use agg_gui::widgets::markdown_editing::edit_operations::{insert_text, split_block};
use agg_gui::widgets::markdown_editing::markdown_parser;
use agg_gui::widgets::markdown_editing::{
    DocPosition, RichBlockKind, RichCaret, RichEditHistory, RichEditKind, RichLayout,
    RichLayoutFonts, RichMarkdownEditWidget, RichSelection,
};
use agg_gui::{Color, Framebuffer, GfxCtx, Rect, Size, Widget};

static REGULAR: &[u8] = include_bytes!("../../../demo/assets/Nunito_Regular.ttf");

pub(crate) fn fonts() -> RichLayoutFonts {
    RichLayoutFonts {
        regular: Arc::new(Font::from_slice(REGULAR).expect("demo font")),
        bold: None,
        mono: None,
    }
}

pub(crate) struct Container {
    pub editor: RichMarkdownEditWidget,
    pub fb: Framebuffer,
}

impl Container {
    pub fn new(markdown: &str, width: f64, height: f64) -> Self {
        let mut editor = RichMarkdownEditWidget::new(fonts(), None);
        editor.set_markdown(markdown).unwrap();
        let mut container = Self {
            editor,
            fb: Framebuffer::new(width as u32, height as u32),
        };
        container.perform_layout(width, height);
        container
    }

    pub fn perform_layout(&mut self, width: f64, height: f64) {
        set_current_viewport(Size::new(width, height));
        if self.fb.width() != width as u32 || self.fb.height() != height as u32 {
            self.fb = Framebuffer::new(width as u32, height as u32);
        }
        self.editor.layout(Size::new(width, height));
        self.editor.set_bounds(Rect::new(0.0, 0.0, width, height));
    }

    pub fn draw(&mut self) {
        let mut ctx = GfxCtx::new(&mut self.fb);
        ctx.clear(Color::white());
        paint_subtree(&mut self.editor, &mut ctx);
    }

    pub fn pixel(&self, x: i32, y: i32) -> [u8; 4] {
        let i = ((y as u32 * self.fb.width() + x as u32) * 4) as usize;
        let p = self.fb.pixels();
        [p[i], p[i + 1], p[i + 2], p[i + 3]]
    }
}

const WHITE: [u8; 4] = [255, 255, 255, 255];

fn make(markdown: &str) -> Container {
    Container::new(markdown, 400.0, 300.0)
}

fn paragraphs(count: usize) -> String {
    (0..count)
        .map(|i| format!("Paragraph number {i} with a few words.\n\n"))
        .collect()
}

fn text_layout(
    editor: &RichMarkdownEditWidget,
    index: usize,
) -> &agg_gui::widgets::markdown_editing::RichBlockLayout {
    match editor.block_layout(index) {
        Some(RichLayout::Text(layout)) => layout,
        _ => panic!("block {index} is not a text layout"),
    }
}

#[test]
fn draws_the_document() {
    let mut c = make("# Title\n\nSome **bold** text and `code`.\n\n- one\n- two\n\n> quoted\n");
    c.draw();

    let any_ink =
        (0..c.fb.height() as i32).any(|y| (0..c.fb.width() as i32).any(|x| c.pixel(x, y) != WHITE));
    assert!(any_ink);
}

#[test]
fn an_edit_re_lays_out_only_the_edited_block() {
    let mut c = make(&paragraphs(6));
    c.draw();
    let before = c.editor.layout_count();

    insert_text(
        c.editor.document_mut(),
        DocPosition::new(3, 0),
        "Edited ",
        None,
    )
    .unwrap();
    c.editor.relayout(3, 3).unwrap();
    c.draw();

    assert_eq!(c.editor.layout_count() - before, 1);
}

#[test]
fn a_width_change_re_lays_out_every_block() {
    let mut c = make(&paragraphs(6));
    c.draw();
    let before = c.editor.layout_count();

    c.perform_layout(250.0, 300.0);
    c.draw();

    assert_eq!(
        c.editor.layout_count() - before,
        c.editor.document().blocks.len()
    );
}

#[test]
fn an_untouched_document_reads_back_byte_identical() {
    let source = "---\ntitle: x\n---\n# Heading\n\nText with *style*  and ![img](a.png).\n\n1. one\n2. two\n\n> quote\n>\n> more\n\n```cs\ncode();\n```\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n***\n";
    let mut c = make(source);
    c.draw();

    assert_eq!(c.editor.markdown(), source);
}

#[test]
fn an_empty_document_is_one_empty_paragraph_that_writes_nothing() {
    let mut c = make("");
    c.editor.set_empty_hint(Some("No notes yet...".to_string()));
    c.draw();

    assert!(c.editor.is_empty());
    assert_eq!(c.editor.markdown(), "");
}

#[test]
fn a_multi_paragraph_quote_has_no_gap_between_its_paragraphs() {
    let mut c = make("> one\n>\n> two\n");
    c.draw();

    let editor = &c.editor;
    assert_eq!(editor.document().blocks.len(), 2);
    let (b0, b1) = (
        editor.block_bounds(0).unwrap(),
        editor.block_bounds(1).unwrap(),
    );
    assert_eq!(b0.bottom(), b1.top());

    // The bars of the two paragraphs meet, so they read as one bar.
    let first = text_layout(editor, 0);
    let second = text_layout(editor, 1);
    let first_bar_bottom = b0.top() - first.height + first.quote_bar.unwrap().bottom();
    let second_bar_top = b1.top() - second.height + second.quote_bar.unwrap().top();
    assert!((first_bar_bottom - second_bar_top).abs() <= 0.001);
}

#[test]
fn a_raw_block_is_hosted_as_a_child_at_its_block() {
    let mut c = make("Before\n\n***\n\nAfter\n");
    c.draw();

    let editor = &c.editor;
    assert_eq!(editor.document().blocks[1].kind, RichBlockKind::Raw);
    let host = editor.raw_block_widget(1).expect("a host");
    // Its parent is the editor: it is one of the editor's children.
    assert_eq!(editor.children().len(), 1);
    assert_eq!(editor.children()[0].type_name(), "RichRawHost");

    let bounds = editor.block_bounds(1).unwrap();
    let at = host.bounds_in_view();
    assert!(at.y >= bounds.bottom());
    assert!(at.y + at.height <= bounds.top() + 0.001);
    assert!(editor.raw_block_widget(0).is_none());
}

#[test]
fn a_table_draws_its_cells_and_carets_round_trip_through_the_widget() {
    let mut c = make("| a | b |\n|---|---|\n| one | two |\n| three | four |\n");
    c.draw();

    let editor = &c.editor;
    let Some(RichLayout::Table(table)) = editor.block_layout(0) else {
        panic!("not a table");
    };
    assert_eq!(table.rows.len(), 3);
    assert!(editor.raw_block_widget(0).is_none());

    // The cell "four" has ink inside its box once drawn.
    let origin = editor.block_origin(0).unwrap();
    let cell = table.cell(2, 1).unwrap().rect;
    let low = editor.view_to_editor(Point::new(cell.left(), cell.bottom() + origin));
    let high = editor.view_to_editor(Point::new(cell.right(), cell.top() + origin));
    let mut ink = 0;
    for y in (low.y as i32 + 2)..(high.y as i32 - 1) {
        for x in (low.x as i32 + 2)..(high.x as i32 - 1) {
            if c.pixel(x, y) != WHITE {
                ink += 1;
            }
        }
    }
    assert!(ink > 0);

    // A caret in a cell, placed in the widget by the block's origin, hit-tests back to the same caret.
    let caret = RichCaret {
        offset: 2,
        at_line_end: false,
        row: 2,
        column: 1,
    };
    let caret_width = editor.style().unwrap().caret_width();
    let caret_rect = table.caret_rect(caret, caret_width).unwrap();
    let in_view = Point::new(caret_rect.center().x, caret_rect.center().y + origin);
    assert!(editor.block_bounds(0).unwrap().contains(in_view));
    assert_eq!(
        table.hit_test(Point::new(in_view.x, in_view.y - origin)),
        caret
    );
}

const WRAPPING: &str = "Typing many words into a block so that it wraps onto several more lines ";

#[test]
fn growing_the_first_block_keeps_the_view_at_the_top() {
    let mut c = Container::new(&paragraphs(60), 400.0, 200.0);
    c.draw();

    for _ in 0..5 {
        insert_text(
            c.editor.document_mut(),
            DocPosition::new(0, 0),
            WRAPPING,
            None,
        )
        .unwrap();
        c.editor.relayout(0, 0).unwrap();
        c.draw();
    }

    assert_eq!(c.editor.scroll_offset_from_top(), 0.0);
    assert_eq!(c.editor.drawn_blocks().start, 0);
}

#[test]
fn a_block_above_the_view_growing_leaves_the_visible_text_still() {
    let mut c = Container::new(&paragraphs(60), 400.0, 200.0);
    c.draw();
    let half = c.editor.max_scroll_from_top() / 2.0;
    c.editor.set_scroll_offset_from_top(half);
    c.draw();
    let watched = c.editor.drawn_blocks().start + 1;
    let y_in_view = |editor: &RichMarkdownEditWidget| {
        editor
            .view_to_editor(Point::new(0.0, editor.block_bounds(watched).unwrap().top()))
            .y
    };
    let before = y_in_view(&c.editor);

    for _ in 0..5 {
        insert_text(
            c.editor.document_mut(),
            DocPosition::new(1, 0),
            WRAPPING,
            None,
        )
        .unwrap();
        c.editor.relayout(1, 1).unwrap();
        c.draw();
    }

    assert!((y_in_view(&c.editor) - before).abs() <= 0.5);
}

#[test]
fn adopting_an_undone_document_shows_it_and_clamps_the_selection() {
    let source = "First\n\nSecond\n";
    let mut c = make(source);
    c.draw();

    let mut history = RichEditHistory::with_default_steps();
    let selection = c.editor.selection();
    history.record(c.editor.document(), selection, RichEditKind::Other, None);
    let typed = insert_text(
        c.editor.document_mut(),
        DocPosition::new(1, 6),
        " and more",
        None,
    )
    .unwrap();
    c.editor.relayout(1, 1).unwrap();
    history.committed(typed, true);
    c.editor.select(typed);

    let (document, selection) = history
        .undo(c.editor.document(), c.editor.selection())
        .unwrap();
    c.editor.set_document(document, selection).unwrap();
    c.draw();
    assert_eq!(c.editor.markdown(), source);
    assert_eq!(
        c.editor.block_layout_id(1),
        Some(c.editor.document().blocks[1].id())
    );

    // A selection past the end of a shorter document is pulled back into it.
    c.editor.select(RichSelection::at(DocPosition::new(1, 6)));
    let selection = c.editor.selection();
    c.editor
        .set_document(markdown_parser::parse("Only\n"), selection)
        .unwrap();
    assert_eq!(c.editor.selection().caret, DocPosition::new(0, 4));
}

#[test]
fn a_theme_change_rebuilds_raw_block_widgets() {
    let mut c = make("Before\n\n***\n\nAfter\n");
    c.draw();
    let old_host = c.editor.raw_block_widget(1).unwrap();

    c.editor.set_theme(None).unwrap();
    c.draw();

    assert!(old_host.is_closed());
    let new_host = c.editor.raw_block_widget(1).expect("a new host");
    assert!(!new_host.ptr_eq(&old_host));
}

#[test]
fn a_new_document_closes_the_old_raw_widgets_even_with_no_width() {
    let mut c = make("Before\n\n***\n\nAfter\n");
    c.draw();
    let old_host = c.editor.raw_block_widget(1).unwrap();

    // Out of its container, with no width.
    c.editor.layout(Size::new(0.0, 300.0));
    c.editor.set_markdown("Plain\n").unwrap();

    assert!(old_host.is_closed());
}

#[test]
fn only_ordered_items_re_lay_out_when_an_insert_renumbers_them() {
    let mut bullets = make("- a\n- b\n- c\n- d\n");
    bullets.draw();
    let before = bullets.editor.layout_count();
    split_block(bullets.editor.document_mut(), DocPosition::new(0, 1)).unwrap();
    bullets.editor.relayout(0, 1).unwrap();

    // The split item and the new one; the bullets below show no number, so they keep their layouts.
    assert_eq!(bullets.editor.layout_count() - before, 2);

    let mut ordered = make("1. a\n2. b\n3. c\n4. d\n");
    ordered.draw();
    let before = ordered.editor.layout_count();
    split_block(ordered.editor.document_mut(), DocPosition::new(0, 1)).unwrap();
    ordered.editor.relayout(0, 1).unwrap();

    // Plus the three items below, which each moved down a number.
    assert_eq!(ordered.editor.layout_count() - before, 5);
}

#[test]
fn scrolling_changes_which_blocks_draw() {
    let mut c = Container::new(&paragraphs(60), 400.0, 200.0);
    c.draw();
    let top = c.editor.drawn_blocks();

    // C#'s Last is inclusive; a Range's end is one past it.
    assert_eq!(top.start, 0);
    assert!(top.end - 1 < 59);

    let max = c.editor.max_scroll_from_top();
    c.editor.set_scroll_offset_from_top(max);
    c.draw();
    let bottom = c.editor.drawn_blocks();

    assert!(bottom.start > top.end - 1);
    assert_eq!(bottom.end - 1, 59);
}
