//! agg-sharp's `RichEditorMouseTests.cs`, ported 1:1 over `RichEditorState`
//! (the edit widget's state; C# sent the events through a container to
//! `RichMarkdownEditWidget`, which forwards them there).  Points are computed
//! from the layouts as C# computed them (caret boxes placed by the block
//! origins) and given in editor coordinates; the click count is what agg-gui's
//! `current_click_count` hands the widget.
//!
//! `AModifierClickOnALinkInARawBlockRaisesLinkClicked` needs the Raw block's
//! rendered host widget and its link; it ports with the widget.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::event::{Modifiers, MouseButton};
use agg_gui::geometry::Point;
use agg_gui::text::Font;
use agg_gui::widgets::markdown_editing::block_layout::RichBlockGeometry;
use agg_gui::widgets::markdown_editing::edit_operations::{insert_text, whole_block};
use agg_gui::widgets::markdown_editing::markdown_writer::source_offset_of;
use agg_gui::widgets::markdown_editing::{
    DocPosition, RichCaret, RichEditorState, RichLayout, RichLayoutFonts, RichLayoutStyle,
    RichSelection,
};

static REGULAR: &[u8] = include_bytes!("../../../demo/assets/Nunito_Regular.ttf");

fn style() -> RichLayoutStyle {
    RichLayoutStyle::new(
        RichLayoutFonts {
            regular: Arc::new(Font::from_slice(REGULAR).expect("demo font")),
            bold: None,
            mono: None,
        },
        Some(1.0),
    )
}

struct Harness {
    editor: RichEditorState,
    held: Modifiers,
}

impl Harness {
    // A point in the editor for a caret, at the middle of its caret box.
    fn at(&self, block: usize, caret: RichCaret) -> Point {
        let view = self.editor.view();
        let caret_width = self.editor.style().unwrap().caret_width();
        let rect = view
            .layout_of(block)
            .unwrap()
            .caret_rect(caret, caret_width)
            .unwrap();
        let center = rect.center();
        self.to_editor(Point::new(
            center.x,
            center.y + view.origin_of(block).unwrap(),
        ))
    }

    fn to_editor(&self, in_view: Point) -> Point {
        self.editor.from_view(in_view)
    }

    fn block_center(&self, block: usize) -> Point {
        self.to_editor(self.editor.view().block_bounds(block).unwrap().center())
    }

    fn down(&mut self, point: Point, clicks: u32) {
        self.editor
            .mouse_down(point, MouseButton::Left, clicks, self.held)
            .unwrap();
    }

    fn move_to(&mut self, point: Point) {
        self.editor.mouse_move(point);
    }

    fn up(&mut self, _point: Point) {
        self.editor.mouse_up();
    }

    fn click(&mut self, point: Point) {
        self.click_n(point, 1);
    }

    fn click_n(&mut self, point: Point, clicks: u32) {
        self.down(point, clicks);
        self.up(point);
    }
}

fn make(markdown: &str) -> Harness {
    let mut editor = RichEditorState::new(Box::new(style));
    editor.set_markdown(markdown).unwrap();
    editor.set_viewport(400.0, 300.0).unwrap();
    Harness {
        editor,
        held: Modifiers::default(),
    }
}

fn at(offset: usize) -> RichSelection {
    RichSelection::at(DocPosition::new(0, offset))
}

fn span(a: (usize, usize), b: (usize, usize)) -> RichSelection {
    RichSelection::new(DocPosition::new(a.0, a.1), DocPosition::new(b.0, b.1))
}

fn counter() -> (Rc<Cell<usize>>, Box<dyn FnMut()>) {
    let count = Rc::new(Cell::new(0));
    let inner = count.clone();
    (count, Box::new(move || inner.set(inner.get() + 1)))
}

fn urls(h: &mut Harness) -> Rc<RefCell<Vec<String>>> {
    let urls = Rc::new(RefCell::new(Vec::new()));
    let inner = urls.clone();
    h.editor.on_link_clicked(Box::new(move |url| {
        inner.borrow_mut().push(url.to_string())
    }));
    urls
}

fn activated(h: &mut Harness) -> Rc<Cell<Option<usize>>> {
    let activated = Rc::new(Cell::new(None));
    let inner = activated.clone();
    h.editor
        .on_raw_block_activated(Box::new(move |offset| inner.set(Some(offset))));
    activated
}

#[test]
fn a_click_places_the_caret_and_tells_the_editor() {
    let mut h = make("Hello world here\n");
    let (user_changes, handler) = counter();
    h.editor.on_selection_changed_by_user(handler);
    let (focus, handler) = counter();
    h.editor.on_focus_requested(handler);

    h.click(h.at(0, RichCaret::new(6)));

    assert_eq!(h.editor.selection(), at(6));
    assert_eq!(focus.get(), 1, "the press asks for focus");
    assert_eq!(user_changes.get(), 1);
}

#[test]
fn a_click_right_of_a_word_broken_line_keeps_the_caret_at_the_line_end() {
    let mut h = make(&format!("{}\n", "W".repeat(120)));
    let end = match h.editor.view().layout_of(0) {
        Some(RichLayout::Text(layout)) => layout.lines[1].start,
        other => panic!("no text layout: {other:?}"),
    };

    h.click(h.at(0, RichCaret::at_line_end(end)));

    assert_eq!(h.editor.selection(), at(end));
    assert!(h.editor.caret_at_line_end());
}

#[test]
fn a_drag_selects_across_two_blocks() {
    let mut h = make("First block\n\nSecond block\n");
    let end = h.at(1, RichCaret::new(3));

    h.down(h.at(0, RichCaret::new(2)), 1);
    h.move_to(end);
    h.up(end);

    assert_eq!(h.editor.selection(), span((0, 2), (1, 3)));
}

#[test]
fn a_shift_click_extends_from_the_anchor() {
    let mut h = make("First block\n\nSecond block\n");
    h.click(h.at(0, RichCaret::new(2)));

    h.held.shift = true;
    h.click(h.at(1, RichCaret::new(1)));

    assert_eq!(h.editor.selection(), span((0, 2), (1, 1)));
}

#[test]
fn a_double_click_selects_a_word() {
    let mut h = make("Hello world here\n");

    h.click_n(h.at(0, RichCaret::new(8)), 2);

    assert_eq!(h.editor.selection(), span((0, 6), (0, 11)));
}

#[test]
fn a_double_click_selects_a_word_in_code() {
    let mut h = make("```\nvar some_name = 1;\n```\n");

    h.click_n(h.at(0, RichCaret::new(6)), 2);

    assert_eq!(h.editor.selection(), span((0, 4), (0, 13)));
}

#[test]
fn a_triple_click_selects_the_blocks_text() {
    let mut h = make("First block\n\nSecond block\n");

    h.click_n(h.at(1, RichCaret::new(4)), 3);

    assert_eq!(h.editor.selection(), span((1, 0), (1, 12)));
}

#[test]
fn clicks_in_a_table_cell_stay_in_that_cell() {
    let mut h = make("| a | b |\n|---|---|\n| one | three |\n");
    let cell = RichCaret {
        offset: 2,
        at_line_end: false,
        row: 1,
        column: 1,
    };

    h.click(h.at(0, cell));
    assert_eq!(
        h.editor.selection(),
        RichSelection::at(DocPosition::in_cell(0, 2, 1, 1))
    );

    h.click_n(h.at(0, cell), 3);
    assert_eq!(
        h.editor.selection(),
        RichSelection::new(
            DocPosition::in_cell(0, 0, 1, 1),
            DocPosition::in_cell(0, 5, 1, 1)
        )
    );
}

#[test]
fn a_click_between_or_below_blocks_goes_to_the_nearest_block() {
    let mut h = make("First block\n\nSecond block\n");

    // The spacing under the first block's text is still the first block: its caret goes on its last line.
    let first = h.editor.view().block_bounds(0).unwrap();
    h.click(h.to_editor(Point::new(first.right() - 1.0, first.bottom() + 0.5)));
    assert_eq!(h.editor.selection(), at(11));

    // The document is shorter than the view: a click under it goes to the end of the last block.
    h.click(Point::new(350.0, 2.0));
    assert_eq!(
        h.editor.selection(),
        RichSelection::at(DocPosition::new(1, 12))
    );
}

#[test]
fn a_click_on_a_raw_block_selects_it_whole() {
    let mut h = make("Before\n\n***\n\nAfter\n");
    let activated = activated(&mut h);

    h.click(h.block_center(1));

    assert_eq!(h.editor.selection(), whole_block(h.editor.document(), 1));
    assert_eq!(activated.get(), None);
}

#[test]
fn a_double_click_on_a_raw_block_raises_where_its_source_starts() {
    let markdown = "Before\n\n***\n\nAfter\n";
    let mut h = make(markdown);
    let activated = activated(&mut h);

    h.click_n(h.block_center(1), 2);

    assert_eq!(activated.get(), markdown.find("***"));
    assert!(h.editor.selection().whole_block);
}

#[test]
fn the_source_offset_counts_an_edited_block_before_it() {
    let mut h = make("Before\n\n***\n\nAfter\n");
    insert_text(
        h.editor.document_mut(),
        DocPosition::new(0, 0),
        "Edited ",
        None,
    )
    .unwrap();
    h.editor.relayout(0, 0).unwrap();

    let offset = source_offset_of(h.editor.document(), 1);

    assert!(h.editor.markdown()[offset..].starts_with("***"));
}

#[test]
fn only_a_modifier_click_on_a_link_raises_link_clicked() {
    let mut h = make("Go [there](https://example.test/x) now\n");
    h.editor.use_mac_key_bindings = false;
    let urls = urls(&mut h);
    let on_link = h.at(0, RichCaret::new(5));

    h.click(on_link);
    assert_eq!(urls.borrow().len(), 0);
    assert_eq!(h.editor.selection(), at(5));

    h.held.ctrl = true;
    h.click(on_link);
    assert_eq!(*urls.borrow(), vec!["https://example.test/x".to_string()]);

    // Off the link, a modifier click is an ordinary click.
    h.click(h.at(0, RichCaret::new(1)));
    assert_eq!(urls.borrow().len(), 1);
}

#[test]
fn a_windows_key_click_on_a_link_only_places_the_caret() {
    // Every host reports Command as the command key; a Windows key can stay latched on X11 and in the browser.
    let mut h = make("Go [there](https://example.test/x) now\n");
    h.editor.use_mac_key_bindings = false;
    let urls = urls(&mut h);
    h.held.meta = true;

    h.click(h.at(0, RichCaret::new(5)));

    assert_eq!(urls.borrow().len(), 0);
    assert_eq!(h.editor.selection(), at(5));
}

#[test]
fn a_drag_after_a_double_click_grows_by_whole_words_keeping_the_first() {
    let mut h = make("alpha beta gamma delta\n");

    h.down(h.at(0, RichCaret::new(8)), 2);
    h.move_to(h.at(0, RichCaret::new(13)));
    assert_eq!(h.editor.selection(), span((0, 6), (0, 16)));

    let back = h.at(0, RichCaret::new(2));
    h.move_to(back);
    h.up(back);
    assert_eq!(h.editor.selection(), span((0, 10), (0, 0)));
}

#[test]
fn a_drag_after_a_triple_click_grows_by_whole_blocks() {
    let mut h = make("First block\n\nSecond block\n");

    h.down(h.at(1, RichCaret::new(4)), 3);
    let target = h.at(0, RichCaret::new(2));
    h.move_to(target);
    h.up(target);

    assert_eq!(h.editor.selection(), span((1, 12), (0, 0)));
}

#[test]
fn a_shift_click_on_a_raw_block_extends_from_the_anchor() {
    let mut h = make("Before\n\n***\n\nAfter\n");
    h.click(h.at(0, RichCaret::new(2)));

    h.held.shift = true;
    h.click(h.block_center(1));

    assert_eq!(h.editor.selection(), span((0, 2), (1, 1)));
}
