//! The seam between the formatting toolbar and whatever owns the document
//! being edited: the [`RichEditCommands`] trait, and [`RichDocumentCommands`],
//! a widget-free implementation over a bare document.  Ports agg-sharp's
//! `IRichEditCommands.cs` (`IRichEditCommands`, `RichDocumentCommands`).
//!
//! The toolbar never touches the editor widget: it reads the document, the
//! selection and the pending style to show its state, and sends every change
//! through [`RichEditCommands::apply_edit`], so toolbar edits go through the
//! editor's one edit pipeline (undo history, relayout, change events) exactly
//! as typing does.
//!
//! C#'s `event EventHandler`s become handler lists added with
//! `on_selection_changed` / `on_document_changed`, and the edit function
//! returns a `Result` like the edit operations it calls: an edit that refuses
//! its input ([`RichEditError`]) changes nothing and raises nothing.

use super::document::RichDocument;
use super::edit_error::RichEditError;
use super::inline_style::RichPendingStyle;
use super::selection::RichSelection;

/// One toolbar edit: mutates the document in place (the `*_operations`
/// functions) and returns the selection to show next and whether it changed
/// the document at all.
pub type RichEdit<'a> = dyn FnMut(&mut RichDocument, RichSelection) -> Result<(RichSelection, bool), RichEditError>
    + 'a;

/// What the formatting toolbar needs from the document's owner (C#'s
/// `IRichEditCommands`).
pub trait RichEditCommands {
    /// The document being edited.  The toolbar only reads it; changes go
    /// through [`Self::apply_edit`].
    fn document(&self) -> &RichDocument;

    /// The current selection.
    fn selection(&self) -> RichSelection;

    /// Styles toggled at a collapsed caret for the next typed text.  The
    /// toolbar sets it when Bold (etc.) is pressed with nothing selected; the
    /// implementation clears it when the caret moves and raises the
    /// selection-changed handlers when it changes, so the buttons show it.
    fn pending_style(&self) -> RichPendingStyle;

    /// Sets [`Self::pending_style`].
    fn set_pending_style(&mut self, style: RichPendingStyle);

    /// Runs one edit.  The implementation:
    /// 1. records a changed edit as one undo step, and nothing for an
    ///    unchanged one (a click that did nothing must not leave an empty undo
    ///    step);
    /// 2. re-lays out everything: a toolbar edit may add, remove or change any
    ///    blocks — a code block toggle replaces several, a table insert adds
    ///    one;
    /// 3. stores the returned selection FIRST, then raises the
    ///    document-changed handlers (when changed) and the selection-changed
    ///    handlers (when the selection moved), so a handler never reads a
    ///    selection that points past the new document.
    ///
    /// An edit that returns an error changes nothing; the error is returned.
    fn apply_edit(&mut self, edit: &mut RichEdit<'_>) -> Result<(), RichEditError>;

    /// Gives keyboard focus back to the text, so a user who clicks Bold or
    /// closes the link row keeps typing.  The toolbar calls it after every
    /// toolbar edit, after a pending-style toggle and after the link row
    /// closes.
    fn focus_text(&mut self);

    /// Adds a handler raised when the caret, the selection or the pending
    /// style changes, so the toolbar can refresh.
    fn on_selection_changed(&mut self, handler: Box<dyn FnMut()>);

    /// Adds a handler raised after any edit (toolbar, typing, undo), or when
    /// the document is replaced.
    fn on_document_changed(&mut self, handler: Box<dyn FnMut()>);
}

/// [`RichEditCommands`] over a bare [`RichDocument`], with no widget, layout
/// or undo history: an edit simply runs and the handlers fire.  Drives the
/// toolbar in its tests and in any host that uses the toolbar without the
/// rich editor widget.
pub struct RichDocumentCommands {
    document: RichDocument,
    selection: RichSelection,
    pending_style: RichPendingStyle,
    selection_changed: Vec<Box<dyn FnMut()>>,
    document_changed: Vec<Box<dyn FnMut()>>,
    focus_requests: usize,
}

impl RichDocumentCommands {
    /// Commands over `document` with `selection`.
    pub fn new(document: RichDocument, selection: RichSelection) -> Self {
        Self {
            document,
            selection,
            pending_style: RichPendingStyle::default(),
            selection_changed: Vec::new(),
            document_changed: Vec::new(),
            focus_requests: 0,
        }
    }

    /// Moves the caret as a click would, so the pending style is cleared as
    /// the editor clears it.
    pub fn set_selection(&mut self, selection: RichSelection) {
        self.selection = selection;
        self.pending_style = RichPendingStyle::default();
        raise(&mut self.selection_changed);
    }

    /// How many times [`RichEditCommands::focus_text`] was called: there is
    /// no text widget to focus, so a test reads this to see the toolbar ask
    /// for it (C#'s overridable `FocusText`).
    pub fn focus_requests(&self) -> usize {
        self.focus_requests
    }

    /// Gives the document back.
    pub fn into_document(self) -> RichDocument {
        self.document
    }
}

impl RichEditCommands for RichDocumentCommands {
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
        self.pending_style = style;
        raise(&mut self.selection_changed);
    }

    fn apply_edit(&mut self, edit: &mut RichEdit<'_>) -> Result<(), RichEditError> {
        let (next, changed) = edit(&mut self.document, self.selection)?;
        let moved = next != self.selection;
        self.selection = next;
        if moved {
            self.pending_style = RichPendingStyle::default();
        }

        if changed {
            raise(&mut self.document_changed);
        }

        if moved {
            raise(&mut self.selection_changed);
        }

        Ok(())
    }

    fn focus_text(&mut self) {
        self.focus_requests += 1;
    }

    fn on_selection_changed(&mut self, handler: Box<dyn FnMut()>) {
        self.selection_changed.push(handler);
    }

    fn on_document_changed(&mut self, handler: Box<dyn FnMut()>) {
        self.document_changed.push(handler);
    }
}

fn raise(handlers: &mut [Box<dyn FnMut()>]) {
    for handler in handlers {
        handler();
    }
}
