//! Event handling for [`TextArea`]: pointer selection and drags, the scroll
//! bar, keys, clipboard chords, focus, and the input-method events (routed
//! on to `ime.rs`). The body of `Widget::on_event`, which `widget_impl.rs`
//! delegates here; split out of it to keep both under the 800-line cap.

use super::*;

impl TextArea {
    /// Handle `event` (the `Widget::on_event` of the area).
    pub(super) fn handle_event(&mut self, event: &Event) -> EventResult {
        // A disabled area takes no input (C# `Enabled = false`); it only lets
        // go of focus and hover.
        if !self.enabled_now() && !matches!(event, Event::FocusLost) {
            if matches!(event, Event::MouseMove { .. }) && self.hovered {
                self.hovered = false;
                crate::animation::request_draw();
            }
            return EventResult::Ignored;
        }
        // While the right-click menu is open it captures events (see
        // `has_active_modal`); route them through it first.
        if let Some(result) = self.route_context_menu(event) {
            return result;
        }
        match event {
            Event::MouseMove { pos } => {
                // Scroll bar takes priority: continue a thumb drag, or update
                // its hover state before the text-hover / selection logic.
                let bar_hover_changed = match self.scrollbar_on_mouse_move(*pos) {
                    super::scroll::ScrollMove::Dragging(moved) => {
                        if moved {
                            crate::animation::request_draw();
                        }
                        return EventResult::Consumed;
                    }
                    super::scroll::ScrollMove::Hover(changed) => changed,
                };
                let was = self.hovered;
                self.hovered = self.hit_test(*pos);
                // Keep the I-beam for the whole selection drag, which may
                // leave the area (it holds capture).
                if self.hovered || self.selecting_drag {
                    set_cursor_icon(CursorIcon::Text);
                }
                if self.selecting_drag {
                    let off = self.byte_offset_at(*pos);
                    self.extend_selection_drag(off);
                    crate::animation::request_draw();
                    return EventResult::Consumed;
                }
                if was != self.hovered || bar_hover_changed {
                    crate::animation::request_draw();
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }
            Event::MouseDown {
                button: MouseButton::Left,
                pos,
                modifiers,
            } => {
                // Grabbing the scroll thumb must not also place the caret.
                if self.scrollbar_begin_drag(*pos) {
                    crate::animation::request_draw();
                    return EventResult::Consumed;
                }
                let off = self.byte_offset_at(*pos);
                let clicks = self.multi_click.register(*pos);
                self.begin_pointer_selection(off, clicks, modifiers.shift);
                self.selecting_drag = true;
                self.focus_time = Some(crate::clock::now());
                crate::animation::request_draw();
                EventResult::Consumed
            }
            Event::MouseDown {
                button: MouseButton::Right,
                pos,
                ..
            } => {
                if self.context_menu_enabled {
                    self.open_context_menu(*pos);
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            Event::MouseUp {
                button: MouseButton::Left,
                ..
            } => {
                self.scrollbar_end_drag();
                self.selecting_drag = false;
                EventResult::Consumed
            }
            // The press ended without a release (the window lost activation
            // mid-drag): a selection or scroll-bar drag ends where it was.
            Event::MouseCaptureLost => {
                self.scrollbar_end_drag();
                self.selecting_drag = false;
                EventResult::Consumed
            }
            Event::MouseWheel { delta_y, .. } => {
                if self.scroll_by_wheel(*delta_y) {
                    crate::animation::request_draw();
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            Event::FocusGained => {
                self.focused = true;
                self.focus_time = Some(crate::clock::now());
                self.text_on_focus = self.text();
                crate::animation::request_draw();
                EventResult::Ignored
            }
            Event::TextInput { .. } | Event::ImePreedit { .. } if self.focused => {
                self.handle_ime_event(event).unwrap_or(EventResult::Ignored)
            }
            Event::FocusLost => {
                self.end_composition(true);
                self.focused = false;
                self.selecting_drag = false;
                self.notify_edit_complete_if_changed();
                crate::animation::request_draw();
                EventResult::Ignored
            }
            Event::KeyDown { key, modifiers } => {
                // Pre-default key-chord interception (e.g. the demo's Ctrl/Cmd+Y
                // case-toggle). A `true` return consumes the event and skips
                // built-in handling. Cloned out of `self` so the callback can
                // freely borrow the shared edit state we also hold.
                if let Some(cb) = self.on_key_chord.clone() {
                    self.begin_undoable_edit();
                    let epoch_before = self.edit.borrow().epoch;
                    if (cb.borrow_mut())(key, modifiers) {
                        // An interceptor that edits text advances the content
                        // epoch (via `note_text_change`); mirror the built-in
                        // funnels by re-wrapping, firing `on_change`, and
                        // scrolling the (possibly moved) caret back into view.
                        if self.edit.borrow().epoch != epoch_before {
                            self.record_edit();
                            self.mark_dirty();
                            self.notify_change();
                            self.ensure_cursor_visible();
                        }
                        crate::animation::request_draw();
                        return EventResult::Consumed;
                    }
                }
                let shift = modifiers.shift;
                let cmd = modifiers.ctrl || modifiers.meta;
                // Word-wise motion and deletion (Ctrl on Windows/Linux,
                // Option on macOS) and the Mac Command chord (line-wise on
                // Left/Right/Backspace, document-wise on Up/Down); see
                // `text_key_bindings`.
                let word = text_key_bindings::word_jump_requested(*modifiers);
                let line = text_key_bindings::mac_command_requested(*modifiers);
                match key {
                    Key::ArrowLeft => {
                        if word {
                            self.move_word(-1, shift);
                        } else if line {
                            let target = self.visual_line_start_at_cursor();
                            self.move_cursor_to(target, shift);
                        } else {
                            self.move_char(-1, shift);
                        }
                    }
                    Key::ArrowRight => {
                        if word {
                            self.move_word(1, shift);
                        } else if line {
                            let target = self.visual_line_end_at_cursor();
                            self.move_cursor_to(target, shift);
                        } else {
                            self.move_char(1, shift);
                        }
                    }
                    Key::ArrowUp => {
                        if line {
                            // Command-Up is the Mac spelling of Control+Home
                            self.move_cursor_to(0, shift);
                        } else {
                            self.move_line(-1, shift);
                        }
                    }
                    Key::ArrowDown => {
                        if line {
                            // Command-Down is the Mac spelling of Control+End
                            let len = self.edit.borrow().text.len();
                            self.move_cursor_to(len, shift);
                        } else {
                            self.move_line(1, shift);
                        }
                    }
                    Key::Home => {
                        // Ctrl/Cmd+Home → document start; plain Home → start of
                        // the current visual (wrapped) line.
                        let target = if cmd {
                            0
                        } else {
                            self.visual_line_start_at_cursor()
                        };
                        self.move_cursor_to(target, shift);
                    }
                    Key::End => {
                        // Ctrl/Cmd+End → document end; plain End → end of the
                        // current visual (wrapped) line.
                        let target = if cmd {
                            self.edit.borrow().text.len()
                        } else {
                            self.visual_line_end_at_cursor()
                        };
                        self.move_cursor_to(target, shift);
                    }
                    Key::PageUp => {
                        let n = self.page_lines() as isize;
                        self.move_lines(-n, shift);
                    }
                    Key::PageDown => {
                        let n = self.page_lines() as isize;
                        self.move_lines(n, shift);
                    }
                    Key::Backspace => {
                        if word {
                            self.delete_word(-1);
                        } else if line {
                            self.delete_to_line_start();
                        } else {
                            self.delete(-1);
                        }
                    }
                    Key::Delete => {
                        if word {
                            self.delete_word(1);
                        } else {
                            self.delete(1);
                        }
                    }
                    Key::Enter => {
                        self.insert_str("\n");
                    }
                    Key::Tab => {
                        // Code-editor Tab: Shift+Tab always outdents; a plain
                        // Tab over a multi-line selection indents every touched
                        // line, otherwise it inserts a single indent (replacing
                        // any within-line selection).
                        let multi_line = {
                            let st = self.edit.borrow();
                            let (lo, hi) = (st.cursor.min(st.anchor), st.cursor.max(st.anchor));
                            hi > lo && st.text[lo..hi].contains('\n')
                        };
                        if shift {
                            self.outdent_selection();
                        } else if multi_line {
                            self.indent_selection();
                        } else {
                            self.insert_str("    ");
                        }
                    }
                    Key::Char('a') | Key::Char('A') if cmd => {
                        self.select_all_text();
                    }
                    Key::Char('c') | Key::Char('C') if cmd => {
                        self.clipboard_copy();
                    }
                    Key::Char('x') | Key::Char('X') if cmd => {
                        self.clipboard_cut();
                    }
                    Key::Char('v') | Key::Char('V') if cmd => {
                        self.clipboard_paste();
                    }
                    // C#'s InternalTextEditWidget: Ctrl+Z undoes,
                    // Ctrl+Shift+Z and Ctrl+Y redo (Cmd on macOS).
                    Key::Char('z') | Key::Char('Z') if cmd && !shift => {
                        self.undo();
                    }
                    Key::Char('z') | Key::Char('Z') | Key::Char('y') | Key::Char('Y') if cmd => {
                        self.redo();
                    }
                    Key::Char(c) if !cmd => {
                        let mut s = [0u8; 4];
                        self.insert_str(c.encode_utf8(&mut s));
                    }
                    _ => return EventResult::Ignored,
                }
                // Keep the caret on-screen after any edit or navigation
                // (re-wraps if the edit dirtied the cache, then scrolls).
                self.ensure_cursor_visible();
                self.focus_time = Some(crate::clock::now());
                crate::animation::request_draw();
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }
}
