//! agg-sharp's `RichEditHistoryTests`
//! (`Tests/Agg.Tests/MarkdigAgg/RichEditHistoryTests.cs`), ported 1:1: which
//! edits merge into one undo step, undo/redo of documents and selections, the
//! step cap, and the no-op protocol.
//!
//! C#'s `new RichEditHistory(0)` throws; [`RichEditHistory::new`] returns
//! `Err(RichEditError::MaxStepsOutOfRange)`.  C#'s `Changed` event is
//! `on_changed`, whose handler receives the history, so the re-entrant undo
//! test undoes through that rather than a captured reference.

use std::cell::Cell;
use std::rc::Rc;

use agg_gui::widgets::markdown_editing::block_operations as block_ops;
use agg_gui::widgets::markdown_editing::edit_operations as ops;
use agg_gui::widgets::markdown_editing::markdown_parser::parse;
use agg_gui::widgets::markdown_editing::markdown_writer::write;
use agg_gui::widgets::markdown_editing::{
    DocPosition, RichBlock, RichBlockKind, RichDocument, RichEditError, RichEditHistory,
    RichEditKind, RichSelection,
};

const ORIGINAL: &str = "# Title\n\nFirst *para*  \nwith a hard break.\n\n- one\n- two\n";

/// Drives the real edit operations the way the editor widget will: record before, committed after.
struct Editor {
    document: RichDocument,
    selection: RichSelection,
    history: RichEditHistory,
}

impl Editor {
    fn new(markdown: &str, caret: DocPosition) -> Self {
        Self::with_max_steps(markdown, caret, RichEditHistory::DEFAULT_MAX_STEPS)
    }

    fn with_max_steps(markdown: &str, caret: DocPosition, max_steps: usize) -> Self {
        Self {
            document: parse(markdown),
            selection: RichSelection::at(caret),
            history: RichEditHistory::new(max_steps).expect("a positive step count"),
        }
    }

    // One keystroke per character, as typing arrives.
    fn type_text(&mut self, text: &str) {
        for c in text.chars() {
            let typed = c.to_string();
            self.edit(
                RichEditKind::Typing,
                Some(&typed),
                false,
                |document, selection| {
                    let caret = if selection.is_empty() {
                        selection.caret
                    } else {
                        ops::delete_selection(document, selection)
                            .expect("a valid selection")
                            .caret
                    };
                    ops::insert_text(document, caret, &typed, None).expect("a valid caret")
                },
            );
        }
    }

    // A paste arrives as one multi-character insert.
    fn paste(&mut self, text: &str) {
        self.edit(
            RichEditKind::Typing,
            Some(text),
            false,
            |document, selection| {
                ops::insert_text(document, selection.caret, text, None).expect("a valid caret")
            },
        );
    }

    // detect_change reports a no-op (nothing written differently) to the history, as the widget will.
    fn backspace(&mut self, detect_change: bool) {
        self.edit(
            RichEditKind::Backspace,
            None,
            detect_change,
            |document, selection| {
                if selection.is_empty() {
                    ops::backspace(document, selection.caret).expect("a valid caret")
                } else {
                    ops::delete_selection(document, selection).expect("a valid selection")
                }
            },
        );
    }

    fn delete(&mut self, detect_change: bool) {
        self.edit(
            RichEditKind::Delete,
            None,
            detect_change,
            |document, selection| ops::delete(document, selection.caret).expect("a valid caret"),
        );
    }

    // The widget's space key: the space is typing, then the shortcut (if it applies) is its own step.
    fn type_space_with_shortcut(&mut self) {
        self.type_text(" ");
        let caret = self.selection.caret;
        self.history
            .record(&self.document, self.selection, RichEditKind::Other, None);
        let applied = block_ops::try_apply_line_start_shortcut(&mut self.document, caret);
        self.selection = RichSelection::at(applied.unwrap_or(caret));
        self.history.committed(self.selection, applied.is_some());
    }

    fn enter(&mut self) {
        self.edit(RichEditKind::Other, None, false, |document, selection| {
            ops::split_block(document, selection.caret).expect("a valid caret")
        });
    }

    fn set_heading(&mut self, level: u8) {
        self.edit(RichEditKind::Other, None, false, |document, selection| {
            block_ops::set_block_kind(document, selection, level).expect("a valid level");
            selection
        });
    }

    fn move_caret(&mut self, caret: DocPosition) {
        self.selection = RichSelection::at(caret);
        self.history.break_coalescing();
    }

    fn undo(&mut self) -> bool {
        let state = self.history.undo(&self.document, self.selection);
        self.adopt(state)
    }

    fn redo(&mut self) -> bool {
        let state = self.history.redo(&self.document, self.selection);
        self.adopt(state)
    }

    fn markdown(&self) -> String {
        write(&self.document)
    }

    fn edit(
        &mut self,
        kind: RichEditKind,
        text: Option<&str>,
        detect_change: bool,
        op: impl FnOnce(&mut RichDocument, RichSelection) -> RichSelection,
    ) {
        let before = detect_change.then(|| self.markdown());
        self.history
            .record(&self.document, self.selection, kind, text);
        self.selection = op(&mut self.document, self.selection);
        let changed = before.map_or(true, |before| self.markdown() != before);
        self.history.committed(self.selection, changed);
    }

    fn adopt(&mut self, state: Option<(RichDocument, RichSelection)>) -> bool {
        match state {
            None => false,
            Some((document, selection)) => {
                self.document = document;
                self.selection = selection;
                true
            }
        }
    }
}

fn p(block: usize, offset: usize) -> DocPosition {
    DocPosition::new(block, offset)
}

fn text(block: &RichBlock) -> String {
    block
        .inlines
        .iter()
        .map(|i| i.text().unwrap_or("@"))
        .collect()
}

// Each block's kind, heading level, dirty flag and text.
fn fingerprint(document: &RichDocument) -> String {
    document
        .blocks
        .iter()
        .map(|b| format!("{:?}{}{}:{}", b.kind, b.heading_level, b.dirty, text(b)))
        .collect::<Vec<_>>()
        .join("|")
}

// Block 1 is "First para\nwith a hard break." - the caret at its end.
fn end_of_paragraph(document: &RichDocument) -> DocPosition {
    p(1, document.blocks[1].text_length(0, 0))
}

#[test]
fn typing_a_word_is_one_step() {
    let mut editor = Editor::new(ORIGINAL, p(0, 5));
    editor.type_text("abc");
    assert_eq!(editor.history.undo_count(), 1);
    assert_eq!(text(&editor.document.blocks[0]), "Titleabc");

    assert!(editor.undo());
    assert_eq!(text(&editor.document.blocks[0]), "Title");
    assert!(!editor.history.can_undo());
}

#[test]
fn each_word_is_its_own_step() {
    let mut editor = Editor::new(ORIGINAL, p(0, 5));
    editor.type_text(" hello world, don't");
    // " ", then "hello " (a word after a space), "world, " and "don't" - the apostrophe stays in its word.
    assert_eq!(editor.history.undo_count(), 4);

    editor.undo();
    assert_eq!(text(&editor.document.blocks[0]), "Title hello world, ");
    editor.undo();
    assert_eq!(text(&editor.document.blocks[0]), "Title hello ");
    editor.undo();
    assert_eq!(text(&editor.document.blocks[0]), "Title ");
}

#[test]
fn type_then_heading_undoes_in_two_steps() {
    let mut editor = Editor::new(ORIGINAL, end_of_paragraph(&parse(ORIGINAL)));
    editor.type_text("xyz");
    editor.set_heading(2);
    assert_eq!(editor.document.blocks[1].kind, RichBlockKind::Heading);
    assert_eq!(editor.history.undo_count(), 2);

    editor.undo();
    assert_eq!(editor.document.blocks[1].kind, RichBlockKind::Paragraph);
    assert_eq!(
        editor.markdown(),
        "# Title\n\nFirst *para*  \nwith a hard break.xyz\n\n- one\n- two\n"
    );

    editor.undo();
    assert_eq!(editor.markdown(), ORIGINAL);
}

#[test]
fn undo_restores_selection_and_redo_reapplies() {
    let mut editor = Editor::new(ORIGINAL, p(0, 2));
    // Select "itl" and type over it: the replacement is one word, so one undo brings "itl" back, with the
    // selection as it was.
    let selection = RichSelection::new(p(0, 1), p(0, 4));
    editor.selection = selection;
    editor.type_text("ab");
    assert_eq!(editor.markdown(), ORIGINAL.replace("Title", "Tabe"));
    assert_eq!(editor.history.undo_count(), 1);

    editor.undo();
    assert_eq!(editor.markdown(), ORIGINAL);
    assert_eq!(editor.selection, selection);
    assert!(editor.history.can_redo());

    assert!(editor.redo());
    assert_eq!(editor.markdown(), ORIGINAL.replace("Title", "Tabe"));
    assert_eq!(editor.selection, RichSelection::at(p(0, 3)));
    assert!(!editor.history.can_redo());

    // A second round trip still restores the pristine snapshot.
    editor.undo();
    assert_eq!(editor.markdown(), ORIGINAL);
}

#[test]
fn new_edit_after_undo_clears_redo() {
    let mut editor = Editor::new(ORIGINAL, p(0, 5));
    editor.type_text("a");
    editor.undo();
    assert!(editor.history.can_redo());

    editor.type_text("b");
    assert!(!editor.history.can_redo());
    assert!(!editor.redo());
    assert_eq!(text(&editor.document.blocks[0]), "Titleb");
}

#[test]
fn caret_move_breaks_typing() {
    let mut editor = Editor::new(ORIGINAL, p(0, 5));
    editor.type_text("ab");
    // Moving away and back to the same spot still starts a new step.
    editor.move_caret(p(0, 0));
    editor.move_caret(p(0, 7));
    editor.type_text("cd");
    assert_eq!(editor.history.undo_count(), 2);

    // Typing somewhere other than where the last keystroke left the caret also breaks.
    editor.selection = RichSelection::at(p(0, 0));
    editor.type_text("e");
    assert_eq!(editor.history.undo_count(), 3);
}

#[test]
fn backspace_runs_merge_separately_from_typing() {
    let mut editor = Editor::new(ORIGINAL, p(0, 5));
    editor.type_text("abc");
    editor.backspace(false);
    editor.backspace(false);
    assert_eq!(editor.history.undo_count(), 2);
    editor.type_text("z");
    assert_eq!(editor.history.undo_count(), 3);

    editor.undo();
    assert_eq!(text(&editor.document.blocks[0]), "Titlea");
    editor.undo();
    assert_eq!(text(&editor.document.blocks[0]), "Titleabc");
}

#[test]
fn enter_is_its_own_step_and_breaks_typing() {
    let mut editor = Editor::new(ORIGINAL, p(0, 5));
    editor.type_text("ab");
    editor.enter();
    editor.type_text("cd");
    assert_eq!(editor.history.undo_count(), 3);
    editor.undo();
    editor.undo();
    assert_eq!(editor.document.blocks.len(), parse(ORIGINAL).blocks.len());
}

#[test]
fn cap_drops_oldest_steps() {
    let mut editor = Editor::with_max_steps(ORIGINAL, p(0, 5), 3);
    for _ in 0..5 {
        editor.enter();
    }

    assert_eq!(editor.history.undo_count(), 3);
    while editor.undo() {}

    // Two Enters are beyond the cap and stay.
    assert_eq!(
        editor.document.blocks.len(),
        parse(ORIGINAL).blocks.len() + 2
    );
    assert_eq!(RichEditHistory::default().max_steps(), 200);
}

#[test]
fn undo_to_start_writes_original_bytes() {
    let mut editor = Editor::new(ORIGINAL, p(2, 0));
    let changes = Rc::new(Cell::new(0));
    let counter = Rc::clone(&changes);
    editor
        .history
        .on_changed(move |_| counter.set(counter.get() + 1));

    // Touch a list item (dirty plus list group regeneration), the paragraph and the heading.
    editor.type_text("zero ");
    editor.enter();
    let end = end_of_paragraph(&editor.document);
    editor.move_caret(end);
    editor.backspace(false);
    editor.move_caret(p(0, 0));
    editor.set_heading(0);
    // The edited state is compared as a model fingerprint (C#'s writer could not yet write an edited list item).
    let edited = fingerprint(&editor.document);
    assert!(editor.document.blocks.iter().filter(|b| b.dirty).count() > 1);
    assert!(changes.get() > 0);

    while editor.undo() {}

    assert_eq!(editor.markdown(), ORIGINAL);
    assert!(editor.document.blocks.iter().all(|b| !b.dirty));

    // And redo all the way gets back the edited text.
    while editor.redo() {}

    assert_eq!(fingerprint(&editor.document), edited);
    editor.history.clear();
    assert!(!(editor.history.can_undo() || editor.history.can_redo()));
}

#[test]
fn pending_style_change_breaks_typing() {
    // The widget calls break_coalescing when the user picks a pending style mid-word.
    let mut editor = Editor::new(ORIGINAL, p(0, 5));
    editor.type_text("hel");
    editor.history.break_coalescing();
    editor.type_text("lo");
    assert_eq!(editor.history.undo_count(), 2);
}

#[test]
fn paste_is_its_own_step() {
    let mut editor = Editor::new(ORIGINAL, p(0, 5));
    editor.type_text("ab");
    editor.paste("cd");
    editor.type_text("e");
    assert_eq!(editor.history.undo_count(), 3);
    editor.undo();
    editor.undo();
    assert_eq!(text(&editor.document.blocks[0]), "Titleab");
}

#[test]
fn no_op_edit_adds_no_step() {
    let mut editor = Editor::new("Plain text.\n", p(0, 0));
    editor.backspace(true);
    assert!(!editor.history.can_undo());

    // A no-op neither ends a typing run nor clears redo.
    editor.move_caret(p(0, 11));
    editor.type_text("ab");
    editor.delete(true);
    editor.type_text("c");
    assert_eq!(editor.history.undo_count(), 1);

    editor.type_text(" d");
    editor.undo();
    editor.delete(true);
    assert!(editor.history.can_redo());
}

#[test]
fn undo_from_a_changed_handler_keeps_the_outer_result() {
    let mut editor = Editor::new(ORIGINAL, p(0, 5));
    editor.type_text("a");
    editor.enter();
    let reentered = Rc::new(Cell::new(false));
    let flag = Rc::clone(&reentered);
    let document = editor.document.deep_clone();
    let selection = editor.selection;
    editor.history.on_changed(move |history| {
        if !flag.get() {
            flag.set(true);
            history.undo(&document, selection);
        }
    });

    let result = editor.history.undo(&editor.document, editor.selection);
    assert!(result.is_some());
    assert!(reentered.get());
}

#[test]
fn one_undo_reverts_just_the_line_start_shortcut() {
    let mut editor = Editor::new("Plain text.\n", p(0, 0));
    editor.enter();
    editor.move_caret(p(0, 0));
    editor.type_text("#");
    editor.type_space_with_shortcut();
    assert_eq!(editor.document.blocks[0].kind, RichBlockKind::Heading);

    editor.undo();
    assert_eq!(editor.document.blocks[0].kind, RichBlockKind::Paragraph);
    assert_eq!(text(&editor.document.blocks[0]), "# ");
    assert_eq!(editor.selection, RichSelection::at(p(0, 2)));
}

#[test]
fn max_steps_must_be_positive() {
    assert_eq!(
        RichEditHistory::new(0).err(),
        Some(RichEditError::MaxStepsOutOfRange)
    );
}
