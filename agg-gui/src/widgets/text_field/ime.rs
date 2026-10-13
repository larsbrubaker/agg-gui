//! Input-method (IME) support for [`TextField`](super::TextField): committed
//! text from [`Event::TextInput`] and inline composition (preedit) text from
//! [`Event::ImePreedit`].
//!
//! The composition is kept in `TextField::preedit`, beside the edit buffer: it
//! is shown spliced in at the caret (replacing the selection, as the commit
//! will) with an underline, but it is not undoable and raises no change
//! callback until committed. Split out of `text_field.rs` (800-line cap).

use super::*;
use crate::ime::Preedit;

impl TextField {
    /// The composition currently shown inline, if an input method is
    /// composing into this field.
    pub fn preedit(&self) -> Option<&Preedit> {
        self.preedit.as_ref()
    }

    /// The byte range the composition replaces: the selection, or the caret.
    fn preedit_anchor_range(&self) -> (usize, usize) {
        let st = self.edit.borrow();
        (st.cursor.min(st.anchor), st.cursor.max(st.anchor))
    }

    /// The text as displayed (before password masking): the buffer with the
    /// composition spliced in, plus the caret's byte offset in it and the
    /// composition's byte span. Without a composition, the buffer, its
    /// cursor, and `None`.
    pub(super) fn shown_text(&self) -> (String, usize, Option<(usize, usize)>) {
        match &self.preedit {
            Some(p) => {
                let (lo, hi) = self.preedit_anchor_range();
                let st = self.edit.borrow();
                (
                    p.splice(&st.text, lo, hi),
                    p.caret_in_spliced(lo),
                    Some((lo, lo + p.text.len())),
                )
            }
            None => {
                let st = self.edit.borrow();
                (st.text.clone(), st.cursor, None)
            }
        }
    }

    /// The displayed text up to the caret, composition included.
    pub(super) fn text_before_caret(&self) -> String {
        let (text, caret, _) = self.shown_text();
        text[..caret].to_string()
    }

    /// Whether the caret line should draw: always without a composition; with
    /// one, only if the input method asked for a caret.
    pub(super) fn ime_allows_caret(&self) -> bool {
        self.preedit.as_ref().is_none_or(|p| p.cursor.is_some())
    }

    /// Handle [`Event::TextInput`] / [`Event::ImePreedit`]; `None` for any
    /// other event.
    pub(super) fn handle_ime_event(&mut self, event: &Event) -> Option<EventResult> {
        match event {
            Event::TextInput { text } => {
                if self.read_only {
                    return Some(EventResult::Ignored);
                }
                self.preedit = None;
                // One line: a committed line break (a pasted-style commit from
                // a soft keyboard) has no place in the field.
                let line: String = text.chars().filter(|c| !matches!(c, '\n' | '\r')).collect();
                self.do_insert(&line, false);
                self.restart_blink();
                Some(EventResult::Consumed)
            }
            Event::ImePreedit { text, cursor } => {
                if self.read_only {
                    return Some(EventResult::Ignored);
                }
                self.preedit = Preedit::from_event(text, *cursor);
                self.restart_blink();
                self.ensure_cursor_visible();
                Some(EventResult::Consumed)
            }
            _ => None,
        }
    }

    /// Show the caret at once, as any keypress does.
    fn restart_blink(&mut self) {
        if self.focused {
            self.focus_time = Some(crate::clock::now());
        }
    }

    /// Underline the composition (a heavier line under the input method's
    /// active range), in the text colour. Called from `paint` with
    /// `(text origin x, baseline y, descent)`; password masking is honoured.
    pub(super) fn paint_preedit_underline(
        &self,
        ctx: &mut dyn DrawCtx,
        shown: &str,
        span: (usize, usize),
        (text_x, baseline_y, descent): (f64, f64, f64),
        color: Color,
    ) {
        let Some(p) = &self.preedit else { return };
        let font = self.active_font();
        let adv = |upto: usize| {
            if self.masking_active() {
                let n = shown[..upto].chars().count();
                measure_advance(&font, &"•".repeat(n), self.font_size)
            } else {
                measure_advance(&font, &shown[..upto], self.font_size)
            }
        };
        let y = baseline_y - descent - 1.0;
        ctx.set_stroke_color(color);
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.move_to(text_x + adv(span.0), y);
        ctx.line_to(text_x + adv(span.1), y);
        ctx.stroke();
        if let Some((a, b)) = p.cursor.filter(|(a, b)| a != b) {
            ctx.set_line_width(2.5);
            ctx.begin_path();
            ctx.move_to(text_x + adv(span.0 + a), y);
            ctx.line_to(text_x + adv(span.0 + b), y);
            ctx.stroke();
        }
    }
}
