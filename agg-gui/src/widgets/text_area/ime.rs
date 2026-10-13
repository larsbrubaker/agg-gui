//! Input-method (IME) support for [`TextArea`]: committed text from
//! [`Event::TextInput`] and inline composition (preedit) text from
//! [`Event::ImePreedit`].
//!
//! The wrap, scroll, selection and caret code all read the edit buffer, so the
//! composition is shown by placing it **in the buffer** (replacing the
//! selection) without an undo step or change callback, and taking it out again
//! before anything real happens: a commit first restores the replaced
//! selection and then inserts through the ordinary `insert_str`, so the undo
//! step and `on_change` are exactly those of typing the committed text, and a
//! cancelled composition leaves the original text and selection. While a
//! composition is showing, `text()` and a bound edit state include it.
//! Split out of `text_area.rs` (800-line cap).

use super::*;
use crate::ime::Preedit;

/// A composition placed in the buffer: where it starts, its length in bytes,
/// the selection text it displaced, and the preedit it shows.
#[derive(Clone, Debug)]
pub(super) struct Composition {
    start: usize,
    len: usize,
    replaced: String,
    preedit: Preedit,
}

impl TextArea {
    /// The composition currently shown inline, if an input method is
    /// composing into this area.
    pub fn preedit(&self) -> Option<&Preedit> {
        self.composition.as_ref().map(|c| &c.preedit)
    }

    /// Take the composition out of the buffer. With `restore`, put back the
    /// selection it displaced, selected as it was (a cancelled composition);
    /// without, leave the caret where the composition began (a commit is about
    /// to insert there).
    pub(super) fn end_composition(&mut self, restore: bool) {
        let Some(c) = self.composition.take() else {
            return;
        };
        {
            let mut st = self.edit.borrow_mut();
            let end = (c.start + c.len).min(st.text.len());
            let start = c.start.min(end);
            st.text.replace_range(start..end, "");
            st.cursor = start;
            st.anchor = start;
            if restore {
                st.text.insert_str(start, &c.replaced);
                st.anchor = start;
                st.cursor = start + c.replaced.len();
            }
            st.note_text_change();
        }
        self.mark_dirty();
    }

    /// Handle [`Event::TextInput`] / [`Event::ImePreedit`]; `None` for any
    /// other event.
    pub(super) fn handle_ime_event(&mut self, event: &Event) -> Option<EventResult> {
        match event {
            Event::TextInput { text } => {
                // Restore the displaced selection, then insert over it: one
                // undo step that brings the selection back.
                self.end_composition(true);
                self.insert_str(text);
                self.after_ime_edit();
                Some(EventResult::Consumed)
            }
            Event::ImePreedit { text, cursor } => {
                let Some(preedit) = Preedit::from_event(text, *cursor) else {
                    self.end_composition(true);
                    self.after_ime_edit();
                    return Some(EventResult::Consumed);
                };
                // Continue the composition's anchor, or start one at the
                // selection.
                let (start, replaced) = match self.composition.clone() {
                    Some(c) => {
                        self.end_composition(false);
                        (c.start, c.replaced)
                    }
                    None => {
                        let mut st = self.edit.borrow_mut();
                        let lo = st.cursor.min(st.anchor).min(st.text.len());
                        let hi = st.cursor.max(st.anchor).min(st.text.len());
                        let replaced = st.text[lo..hi].to_string();
                        st.text.replace_range(lo..hi, "");
                        (lo, replaced)
                    }
                };
                {
                    let mut st = self.edit.borrow_mut();
                    st.text.insert_str(start, &preedit.text);
                    let caret = start + preedit.cursor.map_or(preedit.text.len(), |(a, _)| a);
                    st.cursor = caret;
                    st.anchor = caret;
                    st.note_text_change();
                }
                self.composition = Some(Composition {
                    start,
                    len: preedit.text.len(),
                    replaced,
                    preedit,
                });
                self.mark_dirty();
                self.after_ime_edit();
                Some(EventResult::Consumed)
            }
            _ => None,
        }
    }

    /// Keep the caret on screen and visible, as after any keypress.
    fn after_ime_edit(&mut self) {
        self.ensure_cursor_visible();
        self.focus_time = Some(crate::clock::now());
        crate::animation::request_draw();
    }

    /// Underline the composition on every visual line it spans (a heavier line
    /// under the input method's active range). Drawn in `paint_overlay`, after
    /// the cached bitmap, clamped to the padded inner rect like the caret.
    pub(super) fn paint_preedit_underline(&self, ctx: &mut dyn DrawCtx) {
        let Some(c) = &self.composition else { return };
        let (start, end) = (c.start, c.start + c.len);
        let inner_lo = self.padding;
        let inner_hi = (self.bounds.height - self.padding).max(inner_lo);
        let v = ctx.visuals();
        ctx.set_stroke_color(self.theme.cursor_color.unwrap_or(v.text_color));
        let heavy = c.preedit.cursor.filter(|(a, b)| a != b);
        for line in &self.cached_lines {
            let (a, b) = (start.max(line.start), end.min(line.end));
            if a >= b {
                continue;
            }
            let p0 = self.pos_for_cursor(a);
            let p1 = self.pos_for_cursor(b);
            let y = p0.y + 2.0;
            if y < inner_lo || y > inner_hi {
                continue;
            }
            ctx.set_line_width(1.0);
            ctx.begin_path();
            ctx.move_to(p0.x, y);
            ctx.line_to(p1.x, y);
            ctx.stroke();
            if let Some((ha, hb)) = heavy {
                let (ha, hb) = ((start + ha).max(a), (start + hb).min(b));
                if ha < hb {
                    ctx.set_line_width(2.5);
                    ctx.begin_path();
                    ctx.move_to(self.pos_for_cursor(ha).x, y);
                    ctx.line_to(self.pos_for_cursor(hb).x, y);
                    ctx.stroke();
                }
            }
        }
    }
}
