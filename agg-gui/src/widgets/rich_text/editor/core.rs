//! `RichEditCore` — the **logical** heart of the interactive rich-text editor,
//! shared (behind `Rc<RefCell<_>>`) between the [`RichTextEdit`](super::super::RichTextEdit)
//! widget and the formatting toolbar via a [`RichEditHandle`].
//!
//! Everything here is geometry-free: it owns the document, the caret + anchor
//! (a [`DocRange`] selection), the *pending* caret style, and a time-coalescing
//! [`Undoer`] snapshotting `{doc, caret, anchor}`.  All structural editing goes
//! through [`super::super::model`]'s primitives and the command engine in
//! [`super::super::commands`]; the widget layer adds only pixel geometry,
//! scrolling and painting on top.
//!
//! # Pending caret style (Word behaviour)
//!
//! Toggling an inline format (bold, colour, …) with a *collapsed* selection does
//! not mutate any run — there is nothing selected.  Instead we remember the
//! toggled style as [`pending_style`](RichEditCore::pending_style); the next
//! inserted text is stamped with it.  Moving the caret or applying the format to
//! a real selection clears it.

use std::cell::RefCell;
use std::rc::Rc;

use crate::undo::Undoer;
use crate::widgets::text_field_core::{next_char_boundary, prev_char_boundary};

use super::super::commands::{apply_command, range_common_style, style_at, CommonStyle, RichCommand};
use super::super::model::{
    insert_text, merge_block_with_prev, remove_range, split_block, DocPos, DocRange, InlineStyle,
    ListKind, RichDoc,
};

/// One undo/redo snapshot: the whole document plus the caret and anchor, so an
/// undo restores the selection exactly as it was.
#[derive(Clone, PartialEq)]
pub(crate) struct EditSnapshot {
    doc: RichDoc,
    caret: DocPos,
    anchor: DocPos,
}

/// The shared, geometry-free editor state.
pub struct RichEditCore {
    doc: RichDoc,
    caret: DocPos,
    anchor: DocPos,
    pending_style: Option<InlineStyle>,
    default_font_size: f64,
    undoer: Undoer<EditSnapshot>,
    /// Bumped whenever `doc` changes — the view invalidates its layout cache.
    doc_rev: u64,
    /// Bumped on any caret / anchor / doc change — the view repaints.
    rev: u64,
}

impl RichEditCore {
    /// Create a core over `doc` with the given default font size (points).
    pub fn new(doc: RichDoc, default_font_size: f64) -> Self {
        Self {
            doc,
            caret: DocPos::default(),
            anchor: DocPos::default(),
            pending_style: None,
            default_font_size,
            undoer: Undoer::default(),
            doc_rev: 0,
            rev: 0,
        }
    }

    /// Replace the entire document, resetting the caret/selection to the start
    /// and **discarding the undo/redo history** — the freshly-loaded document is
    /// the new baseline, so there is nothing to undo back to.  Any armed pending
    /// caret style is cleared.
    pub fn load(&mut self, doc: RichDoc) {
        self.doc = doc;
        self.caret = DocPos::default();
        self.anchor = DocPos::default();
        self.pending_style = None;
        // A new document is a new history: drop every prior undo/redo snapshot.
        self.undoer = Undoer::default();
        self.bump_doc();
    }

    // ── Accessors ─────────────────────────────────────────────────────────

    /// The document being edited.
    pub fn doc(&self) -> &RichDoc {
        &self.doc
    }
    /// The document's plain text (blocks joined by `\n`).
    pub fn plain_text(&self) -> String {
        self.doc.plain_text()
    }
    /// The moving end of the selection (the blinking caret position).
    pub fn caret(&self) -> DocPos {
        self.caret
    }
    /// The fixed end of the selection (coincides with the caret when collapsed).
    pub fn anchor(&self) -> DocPos {
        self.anchor
    }
    /// Default font size (points) runs inherit when their style leaves it unset.
    pub fn default_font_size(&self) -> f64 {
        self.default_font_size
    }
    /// Change the inherited default font size and mark the document dirty.
    pub fn set_default_font_size(&mut self, size: f64) {
        self.default_font_size = size;
        self.bump_doc();
    }
    /// Revision counter bumped on every document change (drives layout caching).
    pub fn doc_rev(&self) -> u64 {
        self.doc_rev
    }
    /// Revision counter bumped on any document, caret, or selection change
    /// (drives repaint / scroll-into-view).
    pub fn rev(&self) -> u64 {
        self.rev
    }
    /// The armed pending caret style (a format toggled at a collapsed caret,
    /// awaiting the next keystroke), if any.
    pub fn pending_style(&self) -> Option<&InlineStyle> {
        self.pending_style.as_ref()
    }

    /// The current selection (anchor → caret; collapsed when they coincide).
    pub fn selection(&self) -> DocRange {
        DocRange::new(self.anchor, self.caret)
    }

    fn bump_rev(&mut self) {
        self.rev = self.rev.wrapping_add(1);
    }
    fn bump_doc(&mut self) {
        self.doc_rev = self.doc_rev.wrapping_add(1);
        self.bump_rev();
    }

    /// Clamp a raw `(block, byte)` onto a valid caret position: block in range,
    /// byte within the block and on a char boundary.
    pub fn clamp_pos(&self, pos: DocPos) -> DocPos {
        let block = pos.block.min(self.doc.blocks.len().saturating_sub(1));
        let Some(b) = self.doc.blocks.get(block) else {
            return DocPos::new(0, 0);
        };
        let text = b.text();
        let mut byte = pos.byte.min(text.len());
        while byte > 0 && !text.is_char_boundary(byte) {
            byte -= 1;
        }
        DocPos::new(block, byte)
    }

    // ── Caret / selection movement ────────────────────────────────────────

    /// Move the caret to `pos`.  `extend` keeps the anchor (extending a
    /// selection); otherwise the selection collapses.  Any pending caret style
    /// is discarded (moving the caret abandons an un-typed format).
    pub fn set_caret(&mut self, pos: DocPos, extend: bool) {
        let pos = self.clamp_pos(pos);
        self.caret = pos;
        if !extend {
            self.anchor = pos;
        }
        self.pending_style = None;
        self.bump_rev();
    }

    /// Select the whole document.
    pub fn select_all(&mut self) {
        self.anchor = DocPos::new(0, 0);
        self.caret = self.doc.end_pos();
        self.pending_style = None;
        self.bump_rev();
    }

    /// Set both endpoints of the selection explicitly — `anchor` is the fixed
    /// end, `caret` the moving end. Used by double/triple-click word/block
    /// selection and the drag that extends it.
    pub fn set_selection(&mut self, anchor: DocPos, caret: DocPos) {
        self.anchor = self.clamp_pos(anchor);
        self.caret = self.clamp_pos(caret);
        self.pending_style = None;
        self.bump_rev();
    }

    // ── Style introspection ───────────────────────────────────────────────

    /// Style a freshly-typed character would take: the pending caret style if
    /// one is armed, else the inherited [`style_at`] the caret.
    pub fn style_for_insert(&self) -> InlineStyle {
        self.pending_style
            .clone()
            .unwrap_or_else(|| style_at(&self.doc, self.caret))
    }

    /// Summary of the styles under the current selection, for toolbar state.
    /// With a collapsed selection and an armed pending style, reports that
    /// pending style so the toolbar reflects the format about to be typed.
    pub fn common_style_of_selection(&self) -> CommonStyle {
        let sel = self.selection();
        if sel.is_empty() {
            if let Some(p) = &self.pending_style {
                // Inline attributes come from the armed pending style, but
                // block-level align/list are independent of it — fold the
                // caret block's values in so the alignment/list toggles keep
                // reflecting the caret's block while a format is pending.
                let mut cs = CommonStyle::of_style(p);
                cs.merge_blocks(&self.doc, sel);
                return cs;
            }
        }
        range_common_style(&self.doc, sel)
    }

    /// The selected text as plain text, blocks joined by `\n` (empty when the
    /// selection is collapsed).  Drives Copy / Cut.
    pub fn selected_plain_text(&self) -> String {
        let sel = self.selection();
        if sel.is_empty() {
            return String::new();
        }
        let (a, b) = sel.ordered();
        let mut out = String::new();
        for bi in a.block..=b.block {
            let Some(block) = self.doc.blocks.get(bi) else {
                continue;
            };
            let text = block.text();
            let lo = if bi == a.block { a.byte } else { 0 };
            let hi = if bi == b.block { b.byte.min(text.len()) } else { text.len() };
            if bi > a.block {
                out.push('\n');
            }
            if lo <= hi {
                out.push_str(&text[lo..hi]);
            }
        }
        out
    }

    // ── Text mutation ─────────────────────────────────────────────────────

    /// Remove the active selection (if any), returning the collapse position.
    fn take_selection(&mut self) -> DocPos {
        let sel = self.selection();
        if sel.is_empty() {
            self.caret
        } else {
            let pos = remove_range(&mut self.doc, sel);
            self.caret = pos;
            self.anchor = pos;
            pos
        }
    }

    /// Insert `text` at the caret, replacing any selection.  Embedded `\n`
    /// characters split the paragraph (so pasted multi-line text lands as
    /// several blocks).  The inserted text takes [`Self::style_for_insert`].
    pub fn insert(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let style = self.style_for_insert();
        self.take_selection();
        let mut first = true;
        for segment in text.split('\n') {
            if !first {
                self.caret = split_block(&mut self.doc, self.caret);
            }
            if !segment.is_empty() {
                insert_text(&mut self.doc, self.caret, segment, style.clone());
                self.caret = DocPos::new(self.caret.block, self.caret.byte + segment.len());
            }
            first = false;
        }
        self.anchor = self.caret;
        self.pending_style = None;
        self.bump_doc();
    }

    /// Enter: split the paragraph at the caret (after clearing any selection).
    ///
    /// Standard editor behaviour for an **empty list item**: instead of adding
    /// yet another empty bullet, Enter first outdents (indent > 0 → indent − 1),
    /// and at indent 0 exits the list ([`ListKind::None`]) — no split in either
    /// case.  A non-empty (or non-list) block splits normally.
    pub fn split(&mut self) {
        self.take_selection();
        if let Some(block) = self.doc.blocks.get_mut(self.caret.block) {
            if block.list != ListKind::None && block.text_len() == 0 {
                if block.indent > 0 {
                    block.indent -= 1;
                } else {
                    block.list = ListKind::None;
                }
                self.anchor = self.caret;
                self.pending_style = None;
                self.bump_doc();
                return;
            }
        }
        self.caret = split_block(&mut self.doc, self.caret);
        self.anchor = self.caret;
        self.pending_style = None;
        self.bump_doc();
    }

    /// Backspace: delete the selection, else the char before the caret, else
    /// merge with the previous paragraph.
    pub fn backspace(&mut self) {
        if !self.selection().is_empty() {
            self.take_selection();
            self.pending_style = None;
            self.bump_doc();
            return;
        }
        if self.caret.byte > 0 {
            let text = self.doc.blocks[self.caret.block].text();
            let prev = prev_char_boundary(&text, self.caret.byte);
            let pos = remove_range(
                &mut self.doc,
                DocRange::new(DocPos::new(self.caret.block, prev), self.caret),
            );
            self.caret = pos;
            self.anchor = pos;
        } else if self.caret.block > 0 {
            let pos = merge_block_with_prev(&mut self.doc, self.caret.block);
            self.caret = pos;
            self.anchor = pos;
        } else {
            return;
        }
        self.pending_style = None;
        self.bump_doc();
    }

    /// Delete forward: the selection, else the char after the caret, else pull
    /// the next paragraph up into this one.
    pub fn delete_forward(&mut self) {
        if !self.selection().is_empty() {
            self.take_selection();
            self.pending_style = None;
            self.bump_doc();
            return;
        }
        let block_len = self.doc.blocks[self.caret.block].text_len();
        if self.caret.byte < block_len {
            let text = self.doc.blocks[self.caret.block].text();
            let next = next_char_boundary(&text, self.caret.byte);
            remove_range(
                &mut self.doc,
                DocRange::new(self.caret, DocPos::new(self.caret.block, next)),
            );
        } else if self.caret.block + 1 < self.doc.blocks.len() {
            // Merge the following block into this one; the join point is the
            // current caret (end of this block).
            merge_block_with_prev(&mut self.doc, self.caret.block + 1);
        } else {
            return;
        }
        self.pending_style = None;
        self.bump_doc();
    }

    // ── Command dispatch ──────────────────────────────────────────────────

    /// Apply a formatting command.  An inline command over a *collapsed*
    /// selection arms the pending caret style instead of mutating a run; block
    /// commands and inline commands over a real selection mutate the document.
    pub fn exec(&mut self, cmd: &RichCommand) {
        let sel = self.selection();
        if sel.is_empty() && is_inline(cmd) {
            let mut base = self.style_for_insert();
            apply_inline_to_style(&mut base, cmd);
            self.pending_style = Some(base);
            self.bump_rev();
        } else {
            apply_command(&mut self.doc, sel, cmd);
            self.pending_style = None;
            self.bump_doc();
        }
    }

    // ── Undo / redo (time-coalescing snapshots) ───────────────────────────
    //
    // Note: `feed_undo` (once per frame) and `can_undo` / `can_redo` each build
    // a `snapshot`, which clones the whole `RichDoc`. That is fine at demo
    // scale — per the measure-first rule we don't optimise on speculation — but
    // for very large documents this per-frame clone is the first thing to
    // revisit (e.g. a dirty flag or structural sharing).

    fn snapshot(&self) -> EditSnapshot {
        EditSnapshot {
            doc: self.doc.clone(),
            caret: self.caret,
            anchor: self.anchor,
        }
    }

    fn apply_snapshot(&mut self, snap: EditSnapshot) {
        self.doc = snap.doc;
        self.caret = snap.caret;
        self.anchor = snap.anchor;
        self.pending_style = None;
        self.bump_doc();
    }

    /// Feed the undoer the current state at `time` (seconds).  Returns `true`
    /// while a change is still coalescing, so the caller keeps frames coming.
    pub fn feed_undo(&mut self, time: f64) -> bool {
        let snap = self.snapshot();
        self.undoer.feed_state(time, &snap);
        self.undoer.is_in_flux()
    }

    pub fn can_undo(&self) -> bool {
        self.undoer.has_undo(&self.snapshot())
    }
    pub fn can_redo(&self) -> bool {
        self.undoer.has_redo(&self.snapshot())
    }

    /// Undo one step.  Returns `true` if the document changed.
    pub fn undo(&mut self) -> bool {
        let cur = self.snapshot();
        if let Some(prev) = self.undoer.undo(&cur).cloned() {
            self.apply_snapshot(prev);
            true
        } else {
            false
        }
    }

    /// Redo one step.  Returns `true` if the document changed.
    pub fn redo(&mut self) -> bool {
        let cur = self.snapshot();
        if let Some(next) = self.undoer.redo(&cur).cloned() {
            self.apply_snapshot(next);
            true
        } else {
            false
        }
    }
}

/// Whether a command formats inline characters (vs. block-level layout).
fn is_inline(cmd: &RichCommand) -> bool {
    matches!(
        cmd,
        RichCommand::ToggleBold
            | RichCommand::ToggleItalic
            | RichCommand::ToggleUnderline
            | RichCommand::ToggleStrikethrough
            | RichCommand::SetFontFamily(_)
            | RichCommand::SetFontSize(_)
            | RichCommand::SetTextColor(_)
            | RichCommand::SetHighlight(_)
    )
}

/// Fold an inline command into a single style (for the pending caret style).
/// Toggles flip relative to `style`; setters overwrite.
fn apply_inline_to_style(style: &mut InlineStyle, cmd: &RichCommand) {
    match cmd {
        RichCommand::ToggleBold => style.bold = !style.bold,
        RichCommand::ToggleItalic => style.italic = !style.italic,
        RichCommand::ToggleUnderline => style.underline = !style.underline,
        RichCommand::ToggleStrikethrough => style.strikethrough = !style.strikethrough,
        RichCommand::SetFontFamily(f) => style.font_family = Some(f.clone()),
        RichCommand::SetFontSize(s) => style.font_size = Some(*s),
        RichCommand::SetTextColor(c) => style.text_color = Some(*c),
        RichCommand::SetHighlight(c) => style.highlight = *c,
        _ => {}
    }
}

/// A cheap, cloneable handle to a shared [`RichEditCore`], so a toolbar can
/// drive the same editor the widget renders.  All mutating calls request a
/// redraw.
#[derive(Clone)]
pub struct RichEditHandle {
    core: Rc<RefCell<RichEditCore>>,
}

impl RichEditHandle {
    pub(crate) fn new(core: Rc<RefCell<RichEditCore>>) -> Self {
        Self { core }
    }

    /// Apply a formatting command through the shared core.
    pub fn exec(&self, cmd: &RichCommand) {
        self.core.borrow_mut().exec(cmd);
        crate::animation::request_draw();
    }

    /// Summary of the styles under the current selection (drives toolbar state).
    pub fn common_style_of_selection(&self) -> CommonStyle {
        self.core.borrow().common_style_of_selection()
    }

    /// Select the whole document, then request a redraw.
    pub fn select_all(&self) {
        self.core.borrow_mut().select_all();
        crate::animation::request_draw();
    }

    /// Move the caret to `pos` (clamped onto a valid position), collapsing any
    /// selection, and request a redraw.
    pub fn set_caret(&self, pos: DocPos) {
        self.core.borrow_mut().set_caret(pos, false);
        crate::animation::request_draw();
    }

    /// Set the selection to `range` — `range.start` becomes the fixed anchor and
    /// `range.end` the moving caret, each clamped onto a valid position — and
    /// request a redraw.
    pub fn set_selection(&self, range: DocRange) {
        self.core
            .borrow_mut()
            .set_selection(range.start, range.end);
        crate::animation::request_draw();
    }

    /// The current selection (anchor → caret; collapsed when they coincide).
    pub fn selection(&self) -> DocRange {
        self.core.borrow().selection()
    }

    /// The document's plain text (blocks joined by `\n`).
    pub fn plain_text(&self) -> String {
        self.core.borrow().plain_text()
    }

    /// Replace the editor's document with `doc`, resetting the caret to the
    /// start and **discarding the undo/redo history** (the loaded document is
    /// the new baseline).  Requests a redraw.
    pub fn load(&self, doc: RichDoc) {
        self.core.borrow_mut().load(doc);
        crate::animation::request_draw();
    }

    /// Undo one step through the shared core, requesting a redraw on change.
    pub fn undo(&self) {
        if self.core.borrow_mut().undo() {
            crate::animation::request_draw();
        }
    }
    /// Redo one step through the shared core, requesting a redraw on change.
    pub fn redo(&self) {
        if self.core.borrow_mut().redo() {
            crate::animation::request_draw();
        }
    }
    /// Whether an undo step is available.
    pub fn can_undo(&self) -> bool {
        self.core.borrow().can_undo()
    }
    /// Whether a redo step is available.
    pub fn can_redo(&self) -> bool {
        self.core.borrow().can_redo()
    }
}
