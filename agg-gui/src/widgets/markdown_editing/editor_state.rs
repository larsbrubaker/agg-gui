//! The rich Markdown editor's state without its widget: the document, the
//! selection, the undo history, the pending style, the laid-out view, the
//! scroll offset and the one edit pipeline every keystroke, paste and toolbar
//! edit goes through.  It ports the non-widget half of agg-sharp's
//! `RichMarkdownEditWidget.cs` (selection, `SetDocument`, `Relayout`,
//! `Markdown`, the events) and `RichEditorKeyboard.Apply`.
//!
//! `RichMarkdownEditWidget` (the widget step) owns a [`RichEditorState`],
//! forwards its key and mouse events to `editor_keyboard`, `editor_mouse` and
//! `editor_clipboard` (all `impl RichEditorState` blocks), paints its
//! [`RichDocumentView`] and scrolls to [`RichEditorState::scroll_offset_from_top`].
//!
//! Coordinates: "editor coordinates" are the widget's own (y up, 0 at its
//! bottom, `viewport_height` at its top), what C#'s mouse events carried;
//! "view coordinates" are the [`RichDocumentView`]'s (y up, 0 at the
//! document's bottom).  The view sits at the top of the editor, scrolled up by
//! the scroll offset, as C#'s `ScrollArea` placed it.
//!
//! C#'s `event`s become handler lists (`on_*`); `RichCaretBlink.Restart`
//! becomes [`RichEditorState::placements`], which counts every placed
//! selection so the widget restarts its blink when it changes.

use crate::geometry::{Point, Rect};
use crate::platform;

use super::block::{RichBlock, RichBlockId, RichBlockKind};
use super::document::{DocPosition, RichDocument};
use super::document_view::{RichDocumentView, RichViewSync};
use super::edit_commands::{RichEdit, RichEditCommands};
use super::edit_error::RichEditError;
use super::edit_history::{RichEditHistory, RichEditKind};
use super::inline_style::RichPendingStyle;
use super::layout_style::RichLayoutStyle;
use super::markdown_parser;
use super::markdown_writer;
use super::selection::RichSelection;

/// Room the editor leaves right of the text for its scroll bar (C#'s
/// `ScrollArea.Margin` right of 15, as the viewer leaves it).
pub const SCROLL_BAR_ROOM: f64 = 15.0;

type Handlers = Vec<Box<dyn FnMut()>>;

/// Handlers raised with a link's url.
type LinkHandlers = Vec<Box<dyn FnMut(&str)>>;

/// Finds the link in a Raw block's rendered host: block index and a
/// view-coordinates point to its url.
pub type RichRawLinkLookup = Box<dyn Fn(usize, Point) -> Option<String>>;

/// Builds the layout style (fonts, colours, spacing) when the view needs one.
pub type RichStyleBuilder = Box<dyn FnMut() -> RichLayoutStyle>;

/// The rich editor's state; see the module header.
pub struct RichEditorState {
    pub(super) document: RichDocument,
    pub(super) selection: RichSelection,
    pub(super) caret_at_line_end: bool,
    pub(super) history: RichEditHistory,
    pub(super) pending_style: RichPendingStyle,
    pub(super) view: RichDocumentView,
    build_style: RichStyleBuilder,
    device_scale: f64,
    typography_epoch: u64,
    scroll_top: f64,
    pub(super) viewport_height: f64,
    placements: u64,
    /// Mac (Option word steps, Cmd+arrows to line and document ends) or
    /// Windows/Linux key bindings; defaults to [`platform::use_mac_key_bindings`].
    pub use_mac_key_bindings: bool,
    pub(super) keyboard: super::editor_keyboard::KeyboardState,
    pub(super) mouse: super::editor_mouse::MouseState,
    selection_changed: Handlers,
    document_changed: Handlers,
    pub(super) selection_changed_by_user: Handlers,
    pub(super) link_requested: Handlers,
    pub(super) focus_requested: Handlers,
    pub(super) link_clicked: LinkHandlers,
    pub(super) raw_block_activated: Vec<Box<dyn FnMut(usize)>>,
    pub(super) paste_requested: Vec<Box<dyn FnMut() -> bool>>,
    pub(super) raw_link_at: Option<RichRawLinkLookup>,
}

impl RichEditorState {
    /// An editor showing one empty paragraph, laying out with styles from
    /// `build_style` once it has a size ([`Self::set_viewport`]).
    pub fn new(build_style: RichStyleBuilder) -> Self {
        let mut state = Self {
            document: RichDocument::new(),
            selection: RichSelection::default(),
            caret_at_line_end: false,
            history: RichEditHistory::with_default_steps(),
            pending_style: RichPendingStyle::default(),
            view: RichDocumentView::new(),
            build_style,
            device_scale: 1.0,
            typography_epoch: 0,
            scroll_top: 0.0,
            viewport_height: 0.0,
            placements: 0,
            use_mac_key_bindings: platform::use_mac_key_bindings(),
            keyboard: Default::default(),
            mouse: Default::default(),
            selection_changed: Vec::new(),
            document_changed: Vec::new(),
            selection_changed_by_user: Vec::new(),
            link_requested: Vec::new(),
            focus_requested: Vec::new(),
            link_clicked: Vec::new(),
            raw_block_activated: Vec::new(),
            paste_requested: Vec::new(),
            raw_link_at: None,
        };
        state.ensure_a_block();
        state
    }

    /// The document shown.  Change it through edits ([`Self::apply_edit`]) or
    /// [`Self::set_document`].
    pub fn document(&self) -> &RichDocument {
        &self.document
    }

    /// The document for an edit made outside the pipeline; call
    /// [`Self::relayout`] after it, as C#'s callers of `Document` did.
    pub fn document_mut(&mut self) -> &mut RichDocument {
        &mut self.document
    }

    /// The selection.
    pub fn selection(&self) -> RichSelection {
        self.selection
    }

    /// Which side of a wrap the caret shows on (C#'s `CaretAtLineEnd`).
    pub fn caret_at_line_end(&self) -> bool {
        self.caret_at_line_end
    }

    /// This editor's undo history; loading markdown clears it.
    pub fn history(&self) -> &RichEditHistory {
        &self.history
    }

    /// The laid-out document.
    pub fn view(&self) -> &RichDocumentView {
        &self.view
    }

    /// The style the current layouts were built with.
    pub fn style(&self) -> Option<&RichLayoutStyle> {
        self.view.style()
    }

    /// How many selections have been placed: the widget restarts its caret
    /// blink when this changes (C#'s `caretBlink.Restart()` in
    /// `SetSelectionAfterEdit`).
    pub fn placements(&self) -> u64 {
        self.placements
    }

    /// The pending style for the next typed text.
    pub fn pending_style(&self) -> RichPendingStyle {
        self.pending_style
    }

    /// Sets the pending style; a change ends the typing run and raises the
    /// selection-changed handlers, so the toolbar follows it.
    pub fn set_pending_style(&mut self, style: RichPendingStyle) {
        if self.pending_style != style {
            self.pending_style = style;
            self.history.break_coalescing();
            raise(&mut self.selection_changed);
        }
    }

    /// Sizes the editor: `width` and `height` are the widget's.  The text is
    /// laid out to `width` less [`SCROLL_BAR_ROOM`]; a new width lays every
    /// block out again.
    pub fn set_viewport(&mut self, width: f64, height: f64) -> Result<(), RichEditError> {
        self.viewport_height = height;
        if self.view.set_width(f64::max(width - SCROLL_BAR_ROOM, 0.0)) {
            self.relayout_view_all()?;
        }
        self.set_scroll_offset_from_top(self.scroll_top);
        Ok(())
    }

    /// The device scale and typography epoch the style must be built at; a
    /// change rebuilds the style and lays everything out again.
    pub fn set_text_scale(
        &mut self,
        device_scale: f64,
        typography_epoch: u64,
    ) -> Result<(), RichEditError> {
        if self.device_scale != device_scale || self.typography_epoch != typography_epoch {
            self.device_scale = device_scale;
            self.typography_epoch = typography_epoch;
            self.relayout_view_all()?;
        }
        Ok(())
    }

    /// Forgets the style (a theme change) and lays everything out again.
    pub fn invalidate_style(&mut self) -> Result<(), RichEditError> {
        self.view.invalidate_style();
        self.relayout_view_all()
    }

    /// The document as markdown; an untouched document reads back
    /// byte-identical.
    pub fn markdown(&self) -> String {
        markdown_writer::write(&self.document)
    }

    /// Parses and shows `markdown` with the caret at its start, scrolled to
    /// the top, with an empty history.  Not an edit: raises no
    /// document-changed handlers.
    pub fn set_markdown(&mut self, markdown: &str) -> Result<(), RichEditError> {
        self.scroll_top = 0.0;
        self.history.clear();
        self.set_document(
            markdown_parser::parse(markdown),
            RichSelection::at(DocPosition::new(0, 0)),
        )
    }

    /// Shows `document` with `selection` clamped into it (an undo or redo
    /// result) and lays everything out again.  Not an edit.
    pub fn set_document(
        &mut self,
        document: RichDocument,
        selection: RichSelection,
    ) -> Result<(), RichEditError> {
        self.document = document;
        self.ensure_a_block();
        let clamped = RichSelection {
            anchor: self.clamp(selection.anchor),
            caret: self.clamp(selection.caret),
            whole_block: selection.whole_block,
        };
        self.set_selection(clamped, false);
        self.relayout_view_all()
    }

    fn clamp(&self, position: DocPosition) -> DocPosition {
        let block_index = position.block_index.min(self.document.blocks.len() - 1);
        let block = &self.document.blocks[block_index];
        let (mut row, mut column) = (0, 0);
        if block.kind == RichBlockKind::Table && !block.table_rows.is_empty() {
            row = position.row.min(block.table_rows.len() - 1);
            column = position
                .column
                .min(block.table_rows[row].len().saturating_sub(1));
        }
        let offset = position.offset.min(block.text_length(row, column));
        DocPosition::in_cell(block_index, offset, row, column)
    }

    /// Sets the selection with its caret at the start of a line where its
    /// offset wraps (C#'s `Selection` setter).
    pub fn select(&mut self, selection: RichSelection) {
        self.set_selection(selection, false);
    }

    /// Sets the selection and which side of a wrap its caret shows on.  A
    /// move other than by editing ends the typing run.
    pub fn set_selection(&mut self, selection: RichSelection, caret_at_line_end: bool) {
        if selection != self.selection || caret_at_line_end != self.caret_at_line_end {
            self.history.break_coalescing();
        }
        self.set_selection_after_edit(selection, caret_at_line_end);
    }

    /// Places the selection an edit produced without ending the typing run.
    pub(super) fn set_selection_after_edit(
        &mut self,
        selection: RichSelection,
        caret_at_line_end: bool,
    ) {
        let changed = selection != self.selection
            || caret_at_line_end != self.caret_at_line_end
            || !self.pending_style.is_empty();
        self.selection = selection;
        self.caret_at_line_end = caret_at_line_end;
        // A pending Bold belongs to the caret it was picked at; typed text has taken it on by now.
        self.pending_style = RichPendingStyle::default();
        self.placements += 1;
        if changed {
            raise(&mut self.selection_changed);
        }
    }

    /// A selection the user made with the mouse: also ends the typing run
    /// when unchanged, and raises the by-user handlers.
    pub(super) fn set_selection_by_user(
        &mut self,
        selection: RichSelection,
        caret_at_line_end: bool,
    ) {
        self.set_selection(selection, caret_at_line_end);
        // Even a click where the caret already was ends the typing run, as in other editors.
        self.history.break_coalescing();
        raise(&mut self.selection_changed_by_user);
    }

    /// One edit: records it in the history, runs `edit` on the document,
    /// commits, places the selection and re-lays out the blocks from the old
    /// selection's to the new one's (every block with `relayout_all`, the
    /// toolbar's contract).  An edit that refuses its input records nothing.
    pub fn apply_edit(
        &mut self,
        kind: RichEditKind,
        text: Option<&str>,
        relayout_all: bool,
        edit: &mut RichEdit<'_>,
    ) -> Result<(), RichEditError> {
        let before = self.selection;
        let block_count = self.document.blocks.len();
        self.history.record(&self.document, before, kind, text);
        let (after, changed) = match edit(&mut self.document, before) {
            Ok(result) => result,
            Err(e) => {
                self.history.committed(before, false);
                return Err(e);
            }
        };
        self.history.committed(after, changed);
        self.set_selection_after_edit(after, false);
        if !changed {
            // An edit that only moved the caret (Tab to the next cell, Backspace selecting a Raw block) is a
            // caret move, which ends the typing run.
            if after != before {
                self.history.break_coalescing();
            }
            return Ok(());
        }

        if relayout_all {
            return self.relayout_all();
        }

        let first = before.start().block_index.min(after.start().block_index);
        // After a removal the old selection's end names a block that has moved up unchanged; the edit ends
        // where its new selection does.
        let last = if self.document.blocks.len() < block_count {
            after.end().block_index
        } else {
            before.end().block_index.max(after.end().block_index)
        };
        self.relayout(first, first.max(last))
    }

    /// Call after editing the document outside the pipeline: re-lays out
    /// blocks `first..=last` (and blocks inserted, renumbered or reshaped),
    /// and raises the document-changed handlers.
    pub fn relayout(&mut self, first: usize, last: usize) -> Result<(), RichEditError> {
        self.ensure_a_block();
        let changed: Vec<RichBlockId> = self
            .document
            .blocks
            .iter()
            .enumerate()
            .filter(|(i, _)| *i >= first && *i <= last)
            .map(|(_, b)| b.id())
            .collect();
        let scroll = self.view.relayout(
            RichViewSync {
                document: &self.document,
                scroll_top: Some(self.scroll_top),
                device_scale: self.device_scale,
                typography_epoch: self.typography_epoch,
                build_style: &mut *self.build_style,
            },
            &changed,
        )?;
        self.restore_scroll(scroll);
        raise(&mut self.document_changed);
        Ok(())
    }

    /// Lays every block out again and raises the document-changed handlers.
    pub fn relayout_all(&mut self) -> Result<(), RichEditError> {
        self.ensure_a_block();
        self.relayout_view_all()?;
        raise(&mut self.document_changed);
        Ok(())
    }

    fn relayout_view_all(&mut self) -> Result<(), RichEditError> {
        let scroll = self.view.relayout_all(RichViewSync {
            document: &self.document,
            scroll_top: Some(self.scroll_top),
            device_scale: self.device_scale,
            typography_epoch: self.typography_epoch,
            build_style: &mut *self.build_style,
        })?;
        self.restore_scroll(scroll);
        Ok(())
    }

    fn restore_scroll(&mut self, scroll: Option<f64>) {
        if let Some(top) = scroll {
            self.set_scroll_offset_from_top(top);
        } else {
            self.set_scroll_offset_from_top(self.scroll_top);
        }
    }

    /// How far the view is scrolled down from its top.
    pub fn scroll_offset_from_top(&self) -> f64 {
        self.scroll_top
    }

    /// Scrolls to `offset` from the top, clamped to the document.
    pub fn set_scroll_offset_from_top(&mut self, offset: f64) {
        let max = f64::max(self.view.height() - self.viewport_height, 0.0);
        self.scroll_top = offset.clamp(0.0, max);
    }

    /// Where the view's bottom sits in editor coordinates.
    pub fn view_origin(&self) -> f64 {
        self.viewport_height - self.view.height() + self.scroll_top
    }

    /// An editor-coordinates point in view coordinates.
    pub fn to_view(&self, point: Point) -> Point {
        Point::new(point.x, point.y - self.view_origin())
    }

    /// A view-coordinates point in editor coordinates.
    pub fn from_view(&self, point: Point) -> Point {
        Point::new(point.x, point.y + self.view_origin())
    }

    /// The part of the view the editor shows, in view coordinates (C#'s
    /// `view.TransformFromParentSpace(editor, editor.LocalBounds)`).
    pub fn visible_bounds(&self) -> Rect {
        Rect::new(
            0.0,
            -self.view_origin(),
            self.view.width(),
            self.viewport_height,
        )
    }

    /// True when the document is one empty paragraph.
    pub fn is_empty(&self) -> bool {
        self.document.blocks.len() == 1
            && self.document.blocks[0].kind == RichBlockKind::Paragraph
            && self.document.blocks[0].text_length(0, 0) == 0
    }

    // An empty file is one empty paragraph to the user: there is always somewhere for the caret to be. It writes
    // nothing (its source is ""), so an empty file still reads back empty.
    fn ensure_a_block(&mut self) {
        if self.document.blocks.is_empty() {
            self.document
                .blocks
                .push(RichBlock::new(RichBlockKind::Paragraph));
        }
    }

    /// Adds a handler raised on every change of the selection, the caret's
    /// wrap side or the pending style.
    pub fn on_selection_changed(&mut self, handler: Box<dyn FnMut()>) {
        self.selection_changed.push(handler);
    }

    /// Adds a handler raised after every edit, undo and redo.
    pub fn on_document_changed(&mut self, handler: Box<dyn FnMut()>) {
        self.document_changed.push(handler);
    }

    /// Adds a handler raised when the user moves the caret or selection with
    /// the mouse.
    pub fn on_selection_changed_by_user(&mut self, handler: Box<dyn FnMut()>) {
        self.selection_changed_by_user.push(handler);
    }

    /// Adds a handler raised by Cmd/Ctrl+K: the host asks for a link and
    /// applies it with [`Self::apply_edit`].
    pub fn on_link_requested(&mut self, handler: Box<dyn FnMut()>) {
        self.link_requested.push(handler);
    }

    /// Adds a handler raised when a press asks for keyboard focus (C#'s
    /// `editor.Focus()` in the mouse handler and `FocusText`).
    pub fn on_focus_requested(&mut self, handler: Box<dyn FnMut()>) {
        self.focus_requested.push(handler);
    }

    /// Adds a handler raised with a link's url when it is Cmd/Ctrl+clicked.
    pub fn on_link_clicked(&mut self, handler: Box<dyn FnMut(&str)>) {
        self.link_clicked.push(handler);
    }

    /// Adds a handler raised when a Raw block is double-clicked, with where
    /// its source starts in [`Self::markdown`].
    pub fn on_raw_block_activated(&mut self, handler: Box<dyn FnMut(usize)>) {
        self.raw_block_activated.push(handler);
    }

    /// Adds a handler asked first on paste; returning true means the host
    /// pasted (an image it saved) and the editor reads no clipboard text.
    pub fn on_paste_requested(&mut self, handler: Box<dyn FnMut() -> bool>) {
        self.paste_requested.push(handler);
    }

    /// Sets how a Cmd/Ctrl+click finds a link in a Raw block's rendered host
    /// (block index, view-coordinates point): the hosts are the widget's.
    pub fn set_raw_link_lookup(&mut self, lookup: RichRawLinkLookup) {
        self.raw_link_at = Some(lookup);
    }

    pub(super) fn raise_document_changed(&mut self) {
        raise(&mut self.document_changed);
    }
}

pub(super) fn raise(handlers: &mut [Box<dyn FnMut()>]) {
    for handler in handlers {
        handler();
    }
}

/// The toolbar's view of the editor (C#'s `IRichEditCommands` on the edit
/// widget): one undo step of kind Other, every block laid out again.
impl RichEditCommands for RichEditorState {
    fn document(&self) -> &RichDocument {
        &self.document
    }

    fn selection(&self) -> RichSelection {
        self.selection
    }

    fn pending_style(&self) -> RichPendingStyle {
        self.pending_style
    }

    fn set_pending_style(&mut self, style: RichPendingStyle) {
        RichEditorState::set_pending_style(self, style);
    }

    fn apply_edit(&mut self, edit: &mut RichEdit<'_>) -> Result<(), RichEditError> {
        RichEditorState::apply_edit(self, RichEditKind::Other, None, true, edit)
    }

    fn focus_text(&mut self) {
        raise(&mut self.focus_requested);
    }

    fn on_selection_changed(&mut self, handler: Box<dyn FnMut()>) {
        self.selection_changed.push(handler);
    }

    fn on_document_changed(&mut self, handler: Box<dyn FnMut()>) {
        self.document_changed.push(handler);
    }
}
