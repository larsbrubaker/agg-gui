//! Vertical-navigation keys (Up / Down / PageUp / PageDown), read-only Space,
//! and the pre-default key interceptor for [`TextField`], split out of
//! `text_field.rs` to keep that file under the 800-line cap.
//!
//! Mirrors agg-sharp `InternalTextEditWidget.OnKeyDown`: the edit widget
//! handles Up, Down, PageUp, PageDown and Space itself, read-only or not, so
//! none of them escape to the app's shortcuts while a text box has the
//! keyboard. In a single-line field the line above / below is the same line,
//! so `GotoLineAbove` / `GotoLineBelow` leave the caret where it is; an
//! unshifted press only ends the selection (Up / Down collapse it to its
//! start / end, as C#'s `turnOffSelection` does before moving). Command+Up /
//! Command+Down (Mac) jump to the start / end.
//!
//! Widgets that want those keys for themselves (a number box that steps its
//! value on Up / Down, as MatterCAD's `NumberField.SetupUpAndDownArrows` does
//! by subscribing to the edit widget's `KeyDown` event, which fires before
//! the built-in handling) install [`TextField::with_key_intercept`].

use super::*;

impl TextField {
    /// Install a pre-default key interceptor. It runs before the field's
    /// built-in key handling on every `KeyDown` while the field has focus;
    /// returning `true` consumes the event and skips the built-in action.
    ///
    /// This is how a wrapper claims keys the field would otherwise consume
    /// (e.g. Up / Down stepping a number). The callback may write the field's
    /// bound text cell ([`with_text_cell`](Self::with_text_cell)); the field
    /// picks the new text up on its next layout.
    pub fn with_key_intercept(
        mut self,
        cb: impl FnMut(&Key, &Modifiers) -> bool + 'static,
    ) -> Self {
        self.on_key_intercept = Some(Rc::new(RefCell::new(cb)));
        self
    }

    /// Run the installed interceptor, if any. `true` = it consumed the key.
    pub(crate) fn run_key_intercept(&mut self, key: &Key, mods: &Modifiers) -> bool {
        // Cloned out of `self` so the callback is free to reach shared state
        // (the text cell) without overlapping a borrow we hold.
        let Some(cb) = self.on_key_intercept.clone() else {
            return false;
        };
        let consumed = (cb.borrow_mut())(key, mods);
        consumed
    }

    /// Up / Down / PageUp / PageDown and read-only Space. `None` for any
    /// other key, which the main `handle_key` match then handles.
    pub(crate) fn handle_vertical_key(
        &mut self,
        key: &Key,
        mods: Modifiers,
    ) -> Option<EventResult> {
        // C# ends the selection on an unshifted, non-Control navigation key,
        // and on a Mac Command-arrow (a plain caret motion there).
        let ends_selection = text_key_bindings::motion_ends_selection(mods);
        match key {
            Key::ArrowUp | Key::ArrowDown => {
                self.flush_pending();
                let up = matches!(key, Key::ArrowUp);
                let (cur, anchor, len) = {
                    let st = self.edit.borrow();
                    (st.cursor, st.anchor, st.text.len())
                };
                let new_cur = if text_key_bindings::mac_command_requested(mods) {
                    // Mac: Command+Up / Command+Down = start / end of document.
                    if up {
                        0
                    } else {
                        len
                    }
                } else if ends_selection && up {
                    cur.min(anchor)
                } else if ends_selection {
                    cur.max(anchor)
                } else {
                    cur
                };
                let new_anchor = if mods.shift || !ends_selection {
                    anchor
                } else {
                    new_cur
                };
                let mut st = self.edit.borrow_mut();
                st.cursor = new_cur;
                st.anchor = new_anchor;
                drop(st);
                if new_cur == 0 {
                    self.scroll_x = 0.0;
                }
                self.ensure_cursor_visible();
                Some(EventResult::Consumed)
            }
            Key::PageUp | Key::PageDown => {
                // One line only, so a page is the same line: the caret stays.
                self.flush_pending();
                if ends_selection {
                    let mut st = self.edit.borrow_mut();
                    st.anchor = st.cursor;
                }
                Some(EventResult::Consumed)
            }
            // An editable field inserts the space (the `Key::Char` arm); a
            // read-only one still claims it, as C#'s `Keys.Space` case does.
            Key::Char(' ') if self.read_only && !(mods.ctrl || mods.meta) => {
                Some(EventResult::Consumed)
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::sync::Arc;

    use super::*;
    use crate::event::Event;
    use crate::widget::Widget;

    const FONT_BYTES: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");

    fn field(text: &str) -> TextField {
        let font = Arc::new(Font::from_slice(FONT_BYTES).expect("font"));
        let mut f = TextField::new(font).with_text(text);
        f.layout(Size::new(400.0, 32.0));
        f.on_event(&Event::FocusGained);
        f
    }

    fn key(f: &mut TextField, key: Key, modifiers: Modifiers) -> EventResult {
        f.on_event(&Event::KeyDown { key, modifiers })
    }

    fn select(f: &mut TextField, anchor: usize, cursor: usize) {
        let mut st = f.edit.borrow_mut();
        st.anchor = anchor;
        st.cursor = cursor;
    }

    fn caret(f: &TextField) -> (usize, usize) {
        let st = f.edit.borrow();
        (st.cursor, st.anchor)
    }

    fn shift() -> Modifiers {
        Modifiers {
            shift: true,
            ..Modifiers::default()
        }
    }

    #[test]
    fn plain_up_down_are_consumed_and_keep_the_caret() {
        let mut f = field("hello");
        select(&mut f, 2, 2);
        assert!(key(&mut f, Key::ArrowUp, Modifiers::default()).is_consumed());
        assert_eq!(caret(&f), (2, 2));
        assert!(key(&mut f, Key::ArrowDown, Modifiers::default()).is_consumed());
        assert_eq!(caret(&f), (2, 2));
    }

    #[test]
    fn plain_up_collapses_selection_to_its_start_down_to_its_end() {
        let mut f = field("hello world");
        select(&mut f, 1, 4);
        key(&mut f, Key::ArrowUp, Modifiers::default());
        assert_eq!(caret(&f), (1, 1));
        select(&mut f, 4, 1);
        key(&mut f, Key::ArrowDown, Modifiers::default());
        assert_eq!(caret(&f), (4, 4));
    }

    #[test]
    fn shift_up_down_keep_the_selection() {
        let mut f = field("hello world");
        select(&mut f, 1, 4);
        assert!(key(&mut f, Key::ArrowUp, shift()).is_consumed());
        assert_eq!(caret(&f), (4, 1));
        assert!(key(&mut f, Key::ArrowDown, shift()).is_consumed());
        assert_eq!(caret(&f), (4, 1));
    }

    #[test]
    fn command_up_down_jump_to_start_and_end() {
        let _mac = crate::platform::override_platform_for_thread(crate::platform::Platform::MacOS);
        let mut f = field("hello");
        select(&mut f, 2, 2);
        let meta = Modifiers {
            meta: true,
            ..Modifiers::default()
        };
        key(&mut f, Key::ArrowDown, meta);
        assert_eq!(caret(&f), (5, 5));
        key(&mut f, Key::ArrowUp, meta);
        assert_eq!(caret(&f), (0, 0));
    }

    #[test]
    fn page_up_down_are_consumed_and_end_the_selection() {
        let mut f = field("hello world");
        select(&mut f, 1, 4);
        assert!(key(&mut f, Key::PageUp, shift()).is_consumed());
        assert_eq!(caret(&f), (4, 1));
        assert!(key(&mut f, Key::PageDown, Modifiers::default()).is_consumed());
        assert_eq!(caret(&f), (4, 4));
    }

    #[test]
    fn read_only_field_consumes_vertical_keys_and_space() {
        let mut f = field("hello");
        f.read_only = true;
        for k in [Key::ArrowUp, Key::ArrowDown, Key::PageUp, Key::PageDown] {
            assert!(key(&mut f, k, Modifiers::default()).is_consumed());
        }
        assert!(key(&mut f, Key::Char(' '), Modifiers::default()).is_consumed());
        assert_eq!(f.text(), "hello");
        // other characters still pass a read-only field by, as C#'s KeyPress
        // returns unhandled for them
        assert!(!key(&mut f, Key::Char('a'), Modifiers::default()).is_consumed());
    }

    #[test]
    fn editable_field_types_space() {
        let mut f = field("ab");
        select(&mut f, 1, 1);
        assert!(key(&mut f, Key::Char(' '), Modifiers::default()).is_consumed());
        assert_eq!(f.text(), "a b");
    }

    #[test]
    fn key_intercept_sees_keys_before_the_field() {
        let seen = Rc::new(Cell::new(0));
        let font = Arc::new(Font::from_slice(FONT_BYTES).expect("font"));
        let cell = Rc::new(RefCell::new(String::from("1")));
        let mut f = TextField::new(font)
            .with_text_cell(Rc::clone(&cell))
            .with_key_intercept({
                let (seen, cell) = (Rc::clone(&seen), Rc::clone(&cell));
                move |k, _| {
                    if matches!(k, Key::ArrowUp) {
                        seen.set(seen.get() + 1);
                        *cell.borrow_mut() = "2".into();
                        return true;
                    }
                    false
                }
            });
        f.layout(Size::new(400.0, 32.0));
        f.on_event(&Event::FocusGained);
        assert!(key(&mut f, Key::ArrowUp, Modifiers::default()).is_consumed());
        assert_eq!(seen.get(), 1);
        // the field reads the interceptor's write to its text cell on layout
        f.layout(Size::new(400.0, 32.0));
        assert_eq!(f.text(), "2");
        // keys the interceptor declines get the built-in handling
        assert!(key(&mut f, Key::Char('x'), Modifiers::default()).is_consumed());
        assert_eq!(seen.get(), 1);
    }

    #[test]
    fn key_intercept_does_not_run_without_focus() {
        let seen = Rc::new(Cell::new(false));
        let font = Arc::new(Font::from_slice(FONT_BYTES).expect("font"));
        let mut f = TextField::new(font).with_key_intercept({
            let seen = Rc::clone(&seen);
            move |_, _| {
                seen.set(true);
                true
            }
        });
        assert!(!key(&mut f, Key::ArrowUp, Modifiers::default()).is_consumed());
        assert!(!seen.get());
    }
}
