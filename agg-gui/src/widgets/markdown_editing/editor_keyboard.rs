//! The rich editor's keyboard — a port of agg-sharp's `RichEditorKeyboard.cs`:
//! typing, the editing keys, caret movement and the Cmd/Ctrl shortcuts, all
//! through [`RichEditorState::apply_edit`].
//!
//! Bindings follow the plain text field: on mac a word step is Option (Alt),
//! Cmd+Left/Right go to the line's ends and Cmd+Up/Down to the document's;
//! elsewhere Ctrl steps words and Ctrl+Home/End go to the document's ends.
//! Shift with any caret key extends the selection.
//!
//! Differences from C#, all from the host rather than the behaviour:
//! - agg-gui delivers typed characters as `Key::Char` key downs (no separate
//!   KeyPress), so a character is typed when neither Ctrl nor Cmd is held,
//!   and is a shortcut letter otherwise.
//! - The command key is Cmd (`meta`) with mac bindings and Ctrl otherwise;
//!   C#'s mac layer folded Command onto Control before the editor saw it.
//! - A Rust `char` is a whole Unicode scalar, so C#'s wait for the second
//!   half of a surrogate pair has nothing to do.
//! - PageUp/PageDown are not the editor's (C# did not bind them either); the
//!   widget's scroll area scrolls with them.

use crate::event::{Key, Modifiers};

use super::block::RichBlockKind;
use super::caret_navigator::{self as nav, RichCaretTarget};
use super::document::DocPosition;
use super::edit_error::RichEditError;
use super::edit_history::RichEditKind;
use super::edit_operations::{self as ops, blank_line, end_of};
use super::editor_state::RichEditorState;
use super::inline::{InlineAtomKind, RichInline};
use super::inline_style::RichInlineStyle;
use super::inlines;
use super::selection::RichSelection;
use super::{block_operations, style_operations, table_code_operations};

/// What the keyboard remembers between keys.
#[derive(Default)]
pub(crate) struct KeyboardState {
    // The x Up/Down aim for, kept while they repeat so the caret comes back to its column after a short line.
    // It belongs to the caret it was measured for; any other caret (a click, an edit) measures afresh.
    goal_x: f64,
    goal_caret: Option<DocPosition>,
}

impl RichEditorState {
    /// A key press.  Returns whether the editor handled it (an unhandled Tab
    /// in a paragraph is left for focus traversal).  A handled key scrolls
    /// the caret into view.
    pub fn key_down(&mut self, key: &Key, modifiers: Modifiers) -> Result<bool, RichEditError> {
        let handled = match key {
            Key::Char(c) if !modifiers.ctrl && !modifiers.meta => {
                self.key_press(*c)?;
                true
            }
            _ => self.handle(key, modifiers)?,
        };
        if handled {
            self.scroll_caret_into_view()?;
        }
        Ok(handled)
    }

    /// A typed character (C#'s `KeyPress`): control characters are not typed.
    /// A space at a collapsed caret then tries the line-start shortcuts
    /// (`# `, `- `, `1. `, `> `) as its own undo step, so one undo leaves the
    /// marker as typed.
    fn key_press(&mut self, c: char) -> Result<(), RichEditError> {
        if c < ' ' || c == '\u{7f}' {
            return Ok(());
        }
        let text = c.to_string();
        self.type_text(&text)?;
        if text == " " && self.selection.is_empty() {
            let caret = self.selection.caret;
            self.apply_edit(RichEditKind::Other, None, false, &mut |document, _| {
                Ok(
                    match block_operations::try_apply_line_start_shortcut(document, caret) {
                        Some(new_caret) => (RichSelection::at(new_caret), true),
                        None => (RichSelection::at(caret), false),
                    },
                )
            })?;
        }
        Ok(())
    }

    /// Types `text` at the caret in the pending style, replacing the
    /// selection in the same step.
    pub fn type_text(&mut self, text: &str) -> Result<(), RichEditError> {
        let pending = self.pending_style;
        self.apply_edit(
            RichEditKind::Typing,
            Some(text),
            false,
            &mut |document, selection| {
                let caret = if selection.is_empty() {
                    selection.caret
                } else {
                    ops::delete_selection(document, selection)?.caret
                };
                let block = document.blocks.get(caret.block_index);
                let styled =
                    block.is_some_and(|b| b.is_text_block() || b.kind == RichBlockKind::Table);
                let style: Option<RichInline> = if pending.is_empty() || !styled {
                    None
                } else {
                    style_operations::typing_style(document, caret)
                        .map(|base| pending.apply_pending(&base))
                };
                Ok((
                    ops::insert_text(document, caret, text, style.as_ref())?,
                    true,
                ))
            },
        )
    }

    fn handle(&mut self, key: &Key, modifiers: Modifiers) -> Result<bool, RichEditError> {
        let mac = self.use_mac_key_bindings;
        let command = if mac { modifiers.meta } else { modifiers.ctrl };
        let word = if mac { modifiers.alt } else { modifiers.ctrl };
        let shift = modifiers.shift;
        match key {
            Key::ArrowLeft | Key::ArrowRight => {
                let right = *key == Key::ArrowRight;
                if mac && command {
                    let to = self.line_edge(right)?;
                    return Ok(self.move_to(Some(to), shift));
                }
                if !shift && !word && !self.selection.is_empty() {
                    // An arrow collapses a selection to the side it points to, as text fields do.
                    let side = if right {
                        self.selection.end()
                    } else {
                        self.selection.start()
                    };
                    return Ok(self.move_to(Some(target(side)), false));
                }
                let caret = self.selection.caret;
                let to = match (word, right) {
                    (true, true) => nav::next_word(&self.document, caret)?,
                    (true, false) => nav::previous_word(&self.document, caret)?,
                    (false, true) => nav::next(&self.view, &self.document, caret)?,
                    (false, false) => nav::previous(&self.view, &self.document, caret)?,
                };
                Ok(self.move_to(to, shift))
            }
            Key::ArrowUp | Key::ArrowDown => {
                let up = *key == Key::ArrowUp;
                if mac && command {
                    let to = if up {
                        self.document_start()
                    } else {
                        self.document_end()
                    };
                    return Ok(self.move_to(Some(to), shift));
                }
                self.move_vertically(if up { -1 } else { 1 }, shift)
            }
            Key::Home | Key::End => {
                let end = *key == Key::End;
                if command {
                    let to = if end {
                        self.document_end()
                    } else {
                        self.document_start()
                    };
                    return Ok(self.move_to(Some(to), shift));
                }
                let to = self.line_edge(end)?;
                Ok(self.move_to(Some(to), shift))
            }
            Key::Backspace | Key::Delete => {
                self.delete_key(*key == Key::Backspace, word)?;
                Ok(true)
            }
            Key::Enter => {
                self.enter(shift)?;
                Ok(true)
            }
            Key::Tab => self.tab(shift),
            Key::Escape => {
                if self.selection.is_empty() {
                    return Ok(false);
                }
                Ok(self.move_to(Some(target(self.selection.caret)), false))
            }
            Key::Char(c) if command => self.shortcut(c.to_ascii_lowercase(), shift, mac),
            _ => Ok(false),
        }
    }

    fn shortcut(&mut self, letter: char, shift: bool, mac: bool) -> Result<bool, RichEditError> {
        match letter {
            'a' => {
                let all = RichSelection::new(
                    self.document_start().position,
                    self.document_end().position,
                );
                self.set_selection(all, false);
            }
            'c' => {
                self.copy()?;
            }
            'x' => self.cut()?,
            'v' => self.paste()?,
            'b' => self.toggle_style(RichInlineStyle::BOLD)?,
            'i' => self.toggle_style(RichInlineStyle::ITALIC)?,
            'k' => super::editor_state::raise(&mut self.link_requested),
            'z' if shift => self.redo()?,
            'z' => self.undo()?,
            // Ctrl+Y is Windows' redo; Cmd+Y is not a mac redo.
            'y' if !mac => self.redo()?,
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn document_start(&self) -> RichCaretTarget {
        target(DocPosition::new(0, 0))
    }

    fn document_end(&self) -> RichCaretTarget {
        target(end_of(&self.document, self.document.blocks.len() - 1))
    }

    fn line_edge(&self, end: bool) -> Result<RichCaretTarget, RichEditError> {
        nav::line_edge(
            &self.view,
            &self.document,
            self.selection.caret,
            self.caret_at_line_end,
            end,
        )
    }

    /// Puts the caret at `to`, extending the selection from its anchor when
    /// `extend`.  A key with nowhere to go (Left at the start) is still
    /// handled, so it never moves focus.
    fn move_to(&mut self, to: Option<RichCaretTarget>, extend: bool) -> bool {
        if let Some(to) = to {
            let anchor = if extend {
                self.selection.anchor
            } else {
                to.position
            };
            self.set_selection(RichSelection::new(anchor, to.position), to.at_line_end);
        }
        true
    }

    fn move_vertically(&mut self, direction: i32, extend: bool) -> Result<bool, RichEditError> {
        let caret = self.selection.caret;
        if self.keyboard.goal_caret != Some(caret) {
            let bounds =
                nav::caret_bounds(&self.view, &self.document, caret, self.caret_at_line_end)?;
            self.keyboard.goal_x = bounds.map_or(0.0, |b| b.x + b.width / 2.0);
        }
        let x = self.keyboard.goal_x;
        let to = nav::vertical(
            &self.view,
            &self.document,
            caret,
            self.caret_at_line_end,
            x,
            direction,
        )?;
        self.move_to(to, extend);
        // The goal is tied to the caret it produced.
        self.keyboard.goal_x = x;
        self.keyboard.goal_caret = Some(self.selection.caret);
        Ok(true)
    }

    fn delete_key(&mut self, backspace: bool, word: bool) -> Result<(), RichEditError> {
        let kind = if backspace {
            RichEditKind::Backspace
        } else {
            RichEditKind::Delete
        };
        let selection = self.selection;
        if !selection.is_empty() {
            // A selected Raw block (whole_block) goes as a block; any other selection as text.
            return self.apply_edit(kind, None, false, &mut |document, s| {
                Ok((ops::delete_selection(document, s)?, true))
            });
        }

        if word {
            let caret = selection.caret;
            let to = if backspace {
                nav::previous_word(&self.document, caret)?
            } else {
                nav::next_word(&self.document, caret)?
            };
            // A word delete stays inside its block or cell; at an edge it acts as a plain Backspace or Delete.
            if let Some(to) = to
                .filter(|t| same_container(t.position, caret) && t.position.offset != caret.offset)
            {
                return self.apply_edit(RichEditKind::Other, None, false, &mut |document, _| {
                    Ok((ops::delete_range(document, caret, to.position)?, true))
                });
            }
        }

        self.apply_edit(kind, None, false, &mut |document, s| {
            if backspace {
                ops::backspace_changed(document, s.caret)
            } else {
                ops::delete_changed(document, s.caret)
            }
        })
    }

    /// Enter splits the block.  Shift+Enter in a paragraph, list item or quote
    /// inserts a hard line break (an atom written as a backslash and a line
    /// ending); anywhere else — headings and table cells cannot hold one, code
    /// takes plain lines — it is Enter.
    fn enter(&mut self, shift: bool) -> Result<(), RichEditError> {
        self.apply_edit(
            RichEditKind::Other,
            None,
            false,
            &mut |document, selection| {
                let caret = if selection.is_empty() {
                    selection.caret
                } else {
                    ops::delete_selection(document, selection)?.caret
                };
                let kind = document.blocks.get(caret.block_index).map(|b| b.kind);
                let breaks = matches!(
                    kind,
                    Some(RichBlockKind::Paragraph | RichBlockKind::ListItem | RichBlockKind::Quote)
                );
                if shift && breaks {
                    let line = blank_line(document);
                    let raw = format!("\\{}", &line[..line.len() / 2]);
                    let block = &mut document.blocks[caret.block_index];
                    let index = inlines::split_at(&mut block.inlines, caret.offset)
                        .map_err(|_| RichEditError::PositionOutOfRange(caret))?;
                    block
                        .inlines
                        .insert(index, RichInline::atom(InlineAtomKind::HardBreak, raw));
                    block.dirty = true;
                    let after = DocPosition {
                        offset: caret.offset + 1,
                        ..caret
                    };
                    return Ok((RichSelection::at(after), true));
                }
                Ok((ops::split_block(document, caret)?, true))
            },
        )
    }

    /// Tab moves between table cells (adding a row past the last), indents a
    /// list item and types spaces in code.  In a paragraph, heading or quote
    /// it is not handled, so Tab still moves focus out of the editor.
    fn tab(&mut self, shift: bool) -> Result<bool, RichEditError> {
        let caret = self.selection.caret;
        let kind = self.document.blocks.get(caret.block_index).map(|b| b.kind);
        match kind {
            Some(RichBlockKind::Table) => {
                // Moving between cells changes nothing; only Tab past the last cell adds a row.
                self.apply_edit(
                    RichEditKind::Other,
                    None,
                    false,
                    &mut |document, selection| {
                        if shift {
                            Ok((
                                table_code_operations::previous_cell(document, selection.caret)?,
                                false,
                            ))
                        } else {
                            table_code_operations::next_cell_added_row(document, selection.caret)
                        }
                    },
                )?;
                Ok(true)
            }
            Some(RichBlockKind::ListItem) => {
                self.apply_edit(
                    RichEditKind::Other,
                    None,
                    false,
                    &mut |document, selection| {
                        let changed = if shift {
                            block_operations::outdent(document, selection)
                        } else {
                            block_operations::indent(document, selection)
                        };
                        Ok((selection, changed))
                    },
                )?;
                Ok(true)
            }
            Some(RichBlockKind::CodeBlock) => {
                // Shift+Tab in code is consumed but does nothing: leaving the editor mid-code would surprise.
                if !shift {
                    self.type_text(table_code_operations::CODE_TAB)?;
                }
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    /// Cmd/Ctrl+B or I: over a selection it toggles the style; at a caret it
    /// flips the style the next typed text takes, which starts a new undo
    /// step.
    fn toggle_style(&mut self, style: RichInlineStyle) -> Result<(), RichEditError> {
        if self.selection.is_empty() {
            let pending = self.pending_style.toggle(style);
            self.set_pending_style(pending);
            self.history.break_coalescing();
            return Ok(());
        }
        self.apply_edit(
            RichEditKind::Other,
            None,
            false,
            &mut |document, selection| {
                Ok((
                    style_operations::toggle_style(document, selection, style)?,
                    true,
                ))
            },
        )
    }

    /// Steps the history back, adopting its document and selection.
    pub fn undo(&mut self) -> Result<(), RichEditError> {
        if let Some((document, selection)) = self.history.undo(&self.document, self.selection) {
            self.set_document(document, selection)?;
            self.raise_document_changed();
        }
        Ok(())
    }

    /// Re-applies the latest undone step.
    pub fn redo(&mut self) -> Result<(), RichEditError> {
        if let Some((document, selection)) = self.history.redo(&self.document, self.selection) {
            self.set_document(document, selection)?;
            self.raise_document_changed();
        }
        Ok(())
    }

    /// Scrolls the least distance that shows the whole caret line; at the
    /// document's start or end it shows the page's edge too.
    pub fn scroll_caret_into_view(&mut self) -> Result<(), RichEditError> {
        let caret = self.selection.caret;
        let Some(bounds) =
            nav::caret_bounds(&self.view, &self.document, caret, self.caret_at_line_end)?
        else {
            return Ok(());
        };
        let (mut bottom, mut top) = (bounds.y, bounds.y + bounds.height);
        if caret == self.document_start().position {
            top = self.view.height();
        } else if caret == self.document_end().position {
            bottom = 0.0;
        }
        let visible = self.visible_bounds();
        let (visible_bottom, visible_top) = (visible.y, visible.y + visible.height);
        let scroll = self.scroll_offset_from_top();
        if top > visible_top {
            self.set_scroll_offset_from_top(scroll - (top - visible_top));
        } else if bottom < visible_bottom {
            self.set_scroll_offset_from_top(scroll + (visible_bottom - bottom));
        }
        Ok(())
    }
}

fn target(position: DocPosition) -> RichCaretTarget {
    RichCaretTarget {
        position,
        at_line_end: false,
    }
}

fn same_container(a: DocPosition, b: DocPosition) -> bool {
    a.block_index == b.block_index && a.row == b.row && a.column == b.column
}
