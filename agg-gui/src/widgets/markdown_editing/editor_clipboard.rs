//! Cut, copy and paste between the rich editor and the clipboard — a port of
//! agg-sharp's `RichEditorClipboard.cs`, over agg-gui's [`crate::clipboard`].
//!
//! Copy puts the text a reader sees on the clipboard as the plain text —
//! pasting into a plain field anywhere else gives the words, not markdown
//! punctuation — and the selection's markdown rendered as styled HTML for
//! rich targets (mail, documents), through the viewer's
//! [`crate::widgets::markdown::to_styled_html`] in a light, default theme.
//! The clipboard holds only text and HTML, so the editor also keeps the
//! copied fragment itself: a paste whose clipboard text is still that copy's
//! text inserts the fragment, styles and all.  Any other clipboard text is read as markdown,
//! so text written with **bold**, lists or headings pastes formatted, except
//! that a single line break stays a line break.  A single plain line goes in
//! as typing would, in the caret's style.  A code block takes only plain text
//! and a table cell one line.  A paste is never merged with typing in the
//! undo history.

use std::cell::RefCell;

use crate::clipboard;
use crate::widgets::markdown::{to_styled_html, HtmlStyleTheme};

use super::block::RichBlockKind;
use super::document::RichDocument;
use super::edit_error::RichEditError;
use super::edit_history::RichEditKind;
use super::edit_operations as ops;
use super::editor_state::RichEditorState;
use super::inline::RichInline;
use super::markdown_writer;
use super::paste_operations::{parse_clipboard_text, slice, visible_text};
use super::style_operations::typing_style;

/// The last copy's fragment and the plain text it put on the clipboard, which
/// identifies it on paste.  Shared by every editor on the UI thread, as the
/// clipboard is, so a copy from one notes field pastes formatted into
/// another.
struct Copied {
    fragment: RichDocument,
    text: String,
}

thread_local! {
    static COPIED: RefCell<Option<Copied>> = const { RefCell::new(None) };
}

impl RichEditorState {
    /// Copies the selection; false when nothing is selected.
    pub fn copy(&mut self) -> Result<bool, RichEditError> {
        if self.selection.is_empty() {
            return Ok(false);
        }
        let fragment = slice(&self.document, self.selection)?;
        if fragment.blocks.is_empty() {
            return Ok(false);
        }

        let markdown = markdown_writer::write(&fragment);
        let mut text = visible_text(&fragment);
        if text.is_empty() {
            // An image alone has no visible text; its markdown still says what was copied.
            text = markdown.trim().to_string();
        }

        // A light, default theme rather than the editor's: a copy from a dark theme would otherwise carry its
        // dark background and light text into the mail or document it is pasted into.
        let neutral = HtmlStyleTheme {
            default_font_size: self.copy_font_size,
            ..HtmlStyleTheme::default()
        };
        let html = to_styled_html(&markdown, &neutral);
        if html.trim().is_empty() {
            clipboard::set_text(&text);
        } else {
            clipboard::set_rich_text(&text, &html);
        }
        let text = text.replace("\r\n", "\n");
        COPIED.with(|slot| *slot.borrow_mut() = Some(Copied { fragment, text }));
        Ok(true)
    }

    /// Copies the selection, then deletes it as one undo step.
    pub fn cut(&mut self) -> Result<(), RichEditError> {
        if self.copy()? {
            self.apply_edit(
                RichEditKind::Other,
                None,
                false,
                &mut |document, selection| Ok((ops::delete_selection(document, selection)?, true)),
            )?;
        }
        Ok(())
    }

    /// Pastes at the caret, replacing the selection in the same undo step.
    /// The host's paste handlers get the first chance, so an application can
    /// paste an image itself.
    pub fn paste(&mut self) -> Result<(), RichEditError> {
        let mut handled = false;
        for handler in &mut self.paste_requested {
            handled |= handler();
        }
        if handled {
            return Ok(());
        }

        let Some(text) = clipboard::get_text().filter(|t| !t.is_empty()) else {
            return Ok(());
        };
        let text = text.replace("\r\n", "\n");

        // A system clipboard may hand back line endings of its own, so the copy is recognised by its text alone.
        let fragment = COPIED.with(|slot| {
            slot.borrow()
                .as_ref()
                .filter(|copied| copied.text == text)
                .map(|copied| copied.fragment.deep_clone())
        });
        let pending = self.pending_style;
        self.apply_edit(
            RichEditKind::Other,
            None,
            true,
            &mut |document, selection| {
                let caret = if selection.is_empty() {
                    selection.caret
                } else {
                    ops::delete_selection(document, selection)?.caret
                };
                let kind = document
                    .blocks
                    .get(caret.block_index)
                    .map_or(RichBlockKind::Paragraph, |b| b.kind);
                if kind == RichBlockKind::CodeBlock {
                    let plain = fragment.as_ref().map_or_else(|| text.clone(), visible_text);
                    return Ok((ops::insert_text(document, caret, &plain, None)?, true));
                }

                let parsed;
                let pasted = match &fragment {
                    Some(fragment) => fragment,
                    None => {
                        parsed = parse_clipboard_text(&text);
                        if let Some(line) = plain_line(&parsed) {
                            let style: Option<RichInline> =
                                if pending.is_empty() || kind == RichBlockKind::Raw {
                                    None
                                } else {
                                    typing_style(document, caret)
                                        .map(|base| pending.apply_pending(&base))
                                };
                            return Ok((
                                ops::insert_text(document, caret, &line, style.as_ref())?,
                                true,
                            ));
                        }
                        &parsed
                    }
                };
                Ok((
                    super::paste_insert::insert_document(document, caret, pasted)?,
                    true,
                ))
            },
        )
    }
}

/// The text of a parsed paste that is one paragraph with no formatting at
/// all, or `None`.
fn plain_line(parsed: &RichDocument) -> Option<String> {
    let [block] = parsed.blocks.as_slice() else {
        return None;
    };
    if block.kind != RichBlockKind::Paragraph || block.align_group.is_some() {
        return None;
    }
    let plain = block.inlines.iter().all(|inline| {
        inline.as_run().is_some_and(|run| !run.code)
            && !inline.bold
            && !inline.italic
            && !inline.strike
            && inline.link_url.is_none()
    });
    plain.then(|| visible_text(parsed))
}
