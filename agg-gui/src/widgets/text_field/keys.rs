//! Keyboard handling for [`TextField`]: printable characters, the clipboard
//! and undo chords, Backspace / Delete, caret motion (arrows, Home, End),
//! Enter and Escape. `widget_impl.rs` calls [`TextField::handle_key`] for a
//! key the suggestion list and any key interceptor passed on; Up / Down /
//! PageUp / PageDown are `vertical_keys.rs`. Split out of `text_field.rs` to
//! keep it under the 800-line cap.

use super::*;

impl TextField {
    pub(super) fn handle_key(&mut self, key: &Key, mods: Modifiers) -> EventResult {
        // Snapshot cursor/anchor before movement so we can keep anchor on Shift.
        let anchor_before = self.edit.borrow().anchor;

        // Command modifier (clipboard / select-all / undo): `Ctrl` on Windows
        // and Linux, `Cmd` (meta) on macOS.  Treating the two as equivalent
        // means the same handler serves both OSes without branching.
        let cmd = mods.ctrl || mods.meta;
        // Word-wise motion and deletion: `Ctrl` on Windows/Linux, `Option`
        // on macOS; line-wise: Command on macOS (`text_key_bindings`).
        let word = text_key_bindings::word_jump_requested(mods);
        let line = text_key_bindings::mac_command_requested(mods);

        // Up / Down / PageUp / PageDown and read-only Space (`vertical_keys.rs`).
        if let Some(result) = self.handle_vertical_key(key, mods) {
            return result;
        }

        match key {
            // ── Printable characters (and Ctrl/Cmd shortcuts on Char) ──────
            Key::Char(c) if !self.read_only || cmd => {
                if cmd {
                    return match c {
                        'a' | 'A' => {
                            self.select_all_text();
                            EventResult::Consumed
                        }
                        'z' | 'Z' if !mods.shift => {
                            if !self.read_only {
                                self.do_undo();
                            }
                            EventResult::Consumed
                        }
                        'z' | 'Z' | 'y' | 'Y' => {
                            if !self.read_only {
                                self.do_redo();
                            }
                            EventResult::Consumed
                        }
                        'x' | 'X' => {
                            self.clipboard_cut();
                            EventResult::Consumed
                        }
                        'c' | 'C' => {
                            self.clipboard_copy();
                            EventResult::Consumed
                        }
                        'v' | 'V' => {
                            self.clipboard_paste();
                            EventResult::Consumed
                        }
                        _ => EventResult::Ignored,
                    };
                }
                if self.read_only {
                    return EventResult::Ignored;
                }
                let mut buf = [0u8; 4];
                let s = c.encode_utf8(&mut buf);
                self.do_insert(s, true);
                EventResult::Consumed
            }

            // ── Insert clipboard shortcuts ────────────────────────────────
            // Classic Windows bindings (still common on Linux):
            //   Shift+Insert = Paste
            //   Ctrl+Insert  = Copy
            // Plain `Insert` toggles overwrite mode in many editors — we
            // don't model overwrite, so plain Insert is a no-op here.
            Key::Insert => {
                if mods.shift {
                    self.clipboard_paste();
                    return EventResult::Consumed;
                }
                if cmd {
                    self.clipboard_copy();
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }

            // ── Backspace ─────────────────────────────────────────────────
            Key::Backspace if !self.read_only => {
                // Mac Command+Backspace deletes back to the start of the
                // line: select back to it, then delete the selection (C#).
                if line && !word {
                    let mut st = self.edit.borrow_mut();
                    if st.cursor == st.anchor {
                        st.anchor = 0;
                    }
                }
                self.do_delete(false, word);
                EventResult::Consumed
            }

            // ── Delete ────────────────────────────────────────────────────
            Key::Delete if !self.read_only => {
                if mods.shift {
                    // Shift+Delete = Cut
                    self.clipboard_cut();
                } else {
                    self.do_delete(true, word);
                }
                EventResult::Consumed
            }

            // ── Arrow Left ────────────────────────────────────────────────
            // Mac: `Cmd+Left` = start of line (Home behaviour).
            // Win: `Ctrl+Left`, Mac: `Option+Left` = previous word.
            // Plain: one character back (or collapse selection to left).
            Key::ArrowLeft => {
                self.flush_pending();
                let (cur, anchor) = {
                    let st = self.edit.borrow();
                    (st.cursor, st.anchor)
                };
                let new_cur = if line {
                    0 // Mac: Cmd+Left = line start
                } else if !mods.shift && cur != anchor {
                    cur.min(anchor) // collapse to left
                } else if word {
                    prev_word_boundary(&self.edit.borrow().text, cur)
                } else {
                    prev_char_boundary(&self.edit.borrow().text, cur)
                };
                let new_anchor = if mods.shift { anchor } else { new_cur };
                let mut st = self.edit.borrow_mut();
                st.cursor = new_cur;
                st.anchor = new_anchor;
                drop(st);
                if new_cur == 0 {
                    self.scroll_x = 0.0;
                }
                self.ensure_cursor_visible();
                EventResult::Consumed
            }

            // ── Arrow Right ───────────────────────────────────────────────
            // Symmetric with ArrowLeft.  Mac: `Cmd+Right` = end of line.
            Key::ArrowRight => {
                self.flush_pending();
                let text_len = self.edit.borrow().text.len();
                let (cur, anchor) = {
                    let st = self.edit.borrow();
                    (st.cursor, st.anchor)
                };
                let new_cur = if line {
                    text_len // Mac: Cmd+Right = line end
                } else if !mods.shift && cur != anchor {
                    cur.max(anchor) // collapse to right
                } else if word {
                    next_word_boundary(&self.edit.borrow().text, cur)
                } else if cur < text_len {
                    next_char_boundary(&self.edit.borrow().text, cur)
                } else {
                    cur
                };
                let new_anchor = if mods.shift { anchor } else { new_cur };
                let mut st = self.edit.borrow_mut();
                st.cursor = new_cur;
                st.anchor = new_anchor;
                drop(st);
                self.ensure_cursor_visible();
                EventResult::Consumed
            }

            // ── Home ──────────────────────────────────────────────────────
            // Ctrl+Home is "start of document" on Windows — for a single-
            // line field that's the same as plain Home; accept both.
            Key::Home => {
                self.flush_pending();
                let mut st = self.edit.borrow_mut();
                st.cursor = 0;
                if !mods.shift {
                    st.anchor = 0;
                }
                drop(st);
                self.scroll_x = 0.0;
                EventResult::Consumed
            }

            // ── End ───────────────────────────────────────────────────────
            // Ctrl+End analogous to Ctrl+Home — treated as plain End here.
            Key::End => {
                self.flush_pending();
                let len = self.edit.borrow().text.len();
                let mut st = self.edit.borrow_mut();
                st.cursor = len;
                if !mods.shift {
                    st.anchor = len;
                }
                drop(st);
                self.ensure_cursor_visible();
                EventResult::Consumed
            }

            // ── Enter ─────────────────────────────────────────────────────
            // Commit as edit-complete too so numeric/parsed fields apply the
            // value on Enter (not only on blur).  Snapshot text to prevent a
            // second edit-complete firing on later focus loss.
            Key::Enter => {
                self.flush_pending();
                self.notify_enter();
                if self.text() != self.text_on_focus {
                    self.notify_edit_complete();
                    self.text_on_focus = self.text();
                }
                EventResult::Consumed
            }

            // Escape: clear selection if any, else let the parent
            // (typically a modal dialog) handle it.
            Key::Escape => {
                self.flush_pending();
                let (cur, anc) = {
                    let st = self.edit.borrow();
                    (st.cursor, st.anchor)
                };
                if cur != anc {
                    self.edit.borrow_mut().anchor = cur;
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }

            _ => {
                let _ = anchor_before;
                EventResult::Ignored
            }
        }
    }
}
