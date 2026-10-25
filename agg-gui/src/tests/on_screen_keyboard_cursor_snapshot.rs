//! The selection snapshot behind the on-screen keyboard's caret gestures
//! (`widgets/on_screen_keyboard/caret_ops.rs`, `widget/app/keyboard_caret.rs`),
//! driven through the real `App` with touch events on the virtual UI clock
//! (helpers from `on_screen_keyboard_cursor.rs`).
//!
//! Taking the snapshot changes nothing in the field: a pending Bold stays
//! armed, a caret shown at the end of a soft-wrapped line stays there, and a
//! typing run stays one undo step. A restore puts the wrap side back too. The
//! snapshot belongs to the field that took it: when focus moves to another
//! field mid-gesture, the lift deletes nothing there and a cancel restores
//! nothing there. A gesture whose lift never came forgets its snapshot.

use std::time::Duration;

use super::on_screen_keyboard_cursor::{android, text_field, wait, Finger, FINGER, IPHONE};
use super::on_screen_keyboard_touch::{center, char_key, font, key_of, Phone};
use crate::input_profile::InputProfile;
use crate::platform::{override_platform_for_thread, Platform};
use crate::widgets::markdown_editing::document::DocPosition;
use crate::widgets::markdown_editing::edit_widget::RichMarkdownEditWidget;
use crate::widgets::markdown_editing::inline_style::{RichInlineStyle, RichPendingStyle};
use crate::widgets::markdown_editing::layout_style::RichLayoutFonts;
use crate::widgets::markdown_editing::selection::RichSelection;
use crate::widgets::on_screen_keyboard::caret_ops;
use crate::widgets::on_screen_keyboard::cursor_slide::{
    DELETE_WORD_STEP, TRACKPAD_CHAR_STEP, TRACKPAD_HOLD,
};
use crate::widgets::on_screen_keyboard::state::with_state_mut;
use crate::widgets::on_screen_keyboard::{self as osk, test_hook};
use crate::widgets::rich_text::{uniform_resolver, Block, RichCommand, RichDoc, RichTextEdit};
use crate::widgets::TextField;
use crate::{FlexColumn, Key, Modifiers, MouseButton, Widget};

fn iphone(root: Box<dyn Widget>) -> Phone {
    Phone::with(root, false, InputProfile::MobileIOS, IPHONE)
}

/// Hold the space bar until the trackpad starts, then let go (no move).
fn trackpad_hold(phone: &mut Phone) -> Finger {
    let finger = Finger::down(phone, center(key_of("Space").rect));
    wait(phone, TRACKPAD_HOLD + Duration::from_millis(100));
    assert!(test_hook::trackpad_active());
    finger
}

fn rich(phone: &mut Phone) -> &mut RichTextEdit {
    phone
        .app
        .root_mut()
        .as_any_mut()
        .and_then(|a| a.downcast_mut::<RichTextEdit>())
        .expect("the root is the rich text editor")
}

fn markdown(phone: &mut Phone) -> &mut RichMarkdownEditWidget {
    phone
        .app
        .root_mut()
        .as_any_mut()
        .and_then(|a| a.downcast_mut::<RichMarkdownEditWidget>())
        .expect("the root is the markdown editor")
}

fn markdown_editor(text: &str) -> Box<dyn Widget> {
    let fonts = RichLayoutFonts {
        regular: font(),
        bold: None,
        mono: None,
    };
    let mut editor = RichMarkdownEditWidget::new(fonts, None);
    editor.set_markdown(text).expect("markdown loads");
    Box::new(editor)
}

#[test]
fn a_trackpad_hold_keeps_a_pending_bold_in_the_rich_text_editor() {
    let _clock = crate::clock::scoped_virtual(None);
    let doc = RichDoc::from_blocks(vec![Block::plain("ab")]);
    let mut phone = iphone(Box::new(RichTextEdit::new(doc, uniform_resolver(font()))));
    rich(&mut phone).set_caret(crate::widgets::rich_text::DocPos::new(0, 2));
    rich(&mut phone).exec(&RichCommand::ToggleBold);
    assert_eq!(
        rich(&mut phone).common_style_of_selection().bold,
        Some(true)
    );
    let finger = trackpad_hold(&mut phone);
    finger.lift(&mut phone);
    assert_eq!(
        rich(&mut phone).common_style_of_selection().bold,
        Some(true),
        "the Bold toggled at the caret is still armed"
    );
}

#[test]
fn a_trackpad_hold_keeps_a_pending_bold_in_the_markdown_editor() {
    let _clock = crate::clock::scoped_virtual(None);
    let mut phone = iphone(markdown_editor("ab"));
    let state = markdown(&mut phone).state_mut();
    state.select(RichSelection::at(DocPosition::new(0, 2)));
    let bold = RichPendingStyle {
        flipped: RichInlineStyle::BOLD,
    };
    state.set_pending_style(bold);
    let finger = trackpad_hold(&mut phone);
    finger.lift(&mut phone);
    assert_eq!(markdown(&mut phone).state().pending_style(), bold);
}

#[test]
fn the_markdown_caret_at_a_soft_wrap_keeps_its_side_through_a_hold_and_a_restore() {
    let _clock = crate::clock::scoped_virtual(None);
    let mut phone = iphone(markdown_editor("abcdef ghijkl"));
    let at_wrap = RichSelection::at(DocPosition::new(0, 7));
    markdown(&mut phone)
        .state_mut()
        .set_selection(at_wrap, true);
    // A hold that starts the trackpad and lets go leaves the caret alone.
    let finger = trackpad_hold(&mut phone);
    finger.lift(&mut phone);
    let state = markdown(&mut phone).state();
    assert_eq!(state.selection(), at_wrap);
    assert!(
        state.caret_at_line_end(),
        "still at the end of the first line"
    );
    // A trackpad cancelled after moving puts the caret back on that side.
    let mut finger = trackpad_hold(&mut phone);
    finger.slide_by(&mut phone, 2.0 * TRACKPAD_CHAR_STEP + 1.0, 0.0);
    let _ = finger;
    phone.app.on_touch_cancel(FINGER.0, FINGER.1);
    phone.paint();
    let state = markdown(&mut phone).state();
    assert_eq!(state.selection(), at_wrap);
    assert!(state.caret_at_line_end(), "restored at the end of the line");
}

#[test]
fn a_trackpad_hold_does_not_split_a_typing_run() {
    let _bindings = override_platform_for_thread(Platform::Windows);
    let _clock = crate::clock::scoped_virtual(None);
    let mut phone = iphone(Box::new(TextField::new(font())));
    let m = Modifiers::default();
    for c in "abc".chars() {
        phone.app.on_key_down(Key::Char(c), m);
    }
    let finger = trackpad_hold(&mut phone);
    finger.lift(&mut phone);
    for c in "def".chars() {
        phone.app.on_key_down(Key::Char(c), m);
    }
    let ctrl = Modifiers {
        ctrl: true,
        ..Modifiers::default()
    };
    phone.app.on_key_down(Key::Char('z'), ctrl);
    assert_eq!(text_field(&mut phone).text(), "", "one undo takes the run");
}

/// Two fields, the first focused, both with their caret at the end.
fn two_fields() -> Phone {
    let root = FlexColumn::new()
        .add(Box::new(TextField::new(font()).with_text("one two three")))
        .add(Box::new(TextField::new(font()).with_text("abc")));
    let mut phone = Phone::new(Box::new(root), false);
    for (i, end) in [(0, 13), (1, 3)] {
        field_at(&mut phone, i).set_cursor_position(end);
    }
    phone
}

fn field_at(phone: &mut Phone, i: usize) -> &mut TextField {
    phone.app.root_mut().children_mut()[i]
        .as_any_mut()
        .and_then(|a| a.downcast_mut::<TextField>())
        .expect("a field")
}

#[test]
fn a_backspace_slide_deletes_nothing_in_a_field_focus_moved_to() {
    let _clock = crate::clock::scoped_virtual(None);
    let mut phone = two_fields();
    let mut finger = Finger::down(&mut phone, center(key_of("Backspace").rect));
    finger.slide_by(&mut phone, -DELETE_WORD_STEP - 1.0, 0.0);
    assert_eq!(field_at(&mut phone, 0).selection(), "three");
    phone.app.on_key_down(Key::Tab, Modifiers::default());
    let before = field_at(&mut phone, 1).selection();
    finger.lift(&mut phone);
    assert_eq!(
        field_at(&mut phone, 1).text(),
        "abc",
        "nothing deleted there"
    );
    assert_eq!(field_at(&mut phone, 1).selection(), before);
    assert_eq!(field_at(&mut phone, 0).text(), "one two three");
}

#[test]
fn a_cancel_restores_nothing_in_a_field_focus_moved_to() {
    let _clock = crate::clock::scoped_virtual(None);
    let mut phone = two_fields();
    let mut finger = Finger::down(&mut phone, center(key_of("Backspace").rect));
    finger.slide_by(&mut phone, -DELETE_WORD_STEP - 1.0, 0.0);
    phone.app.on_key_down(Key::Tab, Modifiers::default());
    field_at(&mut phone, 1).set_cursor_position(1);
    let _ = finger;
    phone.app.on_touch_cancel(FINGER.0, FINGER.1);
    phone.paint();
    assert_eq!(field_at(&mut phone, 1).cursor_pos(), 1, "its caret stays");
    assert_eq!(field_at(&mut phone, 1).text(), "abc");
}

#[test]
fn a_gesture_whose_lift_never_came_forgets_its_snapshot() {
    let _clock = crate::clock::scoped_virtual(None);
    // A new press while the old one is still held (its lift was lost).
    let mut phone = android("one two three", 13);
    let mut finger = Finger::down(&mut phone, center(key_of("Backspace").rect));
    finger.slide_by(&mut phone, -DELETE_WORD_STEP - 1.0, 0.0);
    assert!(caret_ops::snapshot().is_some());
    let q = center(char_key('q').rect);
    osk::handle_software_keyboard_mouse_down(q, MouseButton::Left, Modifiers::default());
    phone.paint();
    assert_eq!(caret_ops::snapshot(), None, "the replaced gesture ended");
    osk::cancel_press();
    phone.paint();
    drop(phone);

    // The keyboard lost the pointer capture under a gesture.
    let mut phone = android("one two three", 13);
    let mut finger = Finger::down(&mut phone, center(key_of("Backspace").rect));
    finger.slide_by(&mut phone, -DELETE_WORD_STEP - 1.0, 0.0);
    assert!(caret_ops::snapshot().is_some());
    with_state_mut(|s| s.captured_pointer = false);
    phone.paint();
    phone.paint();
    assert_eq!(caret_ops::snapshot(), None, "the dropped gesture ended");
}
