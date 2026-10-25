//! The edges of the on-screen keyboard's caret gestures
//! (`widgets/on_screen_keyboard/cursor_slide.rs`), driven through the real
//! `App` with touch events on the virtual UI clock (helpers from
//! `on_screen_keyboard_cursor.rs`).
//!
//! The Backspace slide gives words back exactly (the caret and selection
//! return to where they were), counts only words it could select, and a
//! short flick is still a Backspace. A second finger or a cancelled touch
//! during the Backspace slide or the trackpad cancels it: the caret and
//! selection go back and nothing is deleted; during a space-bar slide the
//! slide just ends. The trackpad sends line steps only to multi-line fields,
//! and haptic ticks come only for steps that moved the caret (and as the
//! trackpad starts, as on iOS). The other text widgets, the platforms' word
//! modifiers and the fallback for a widget that can't put its selection back
//! are in `on_screen_keyboard_cursor_editors.rs`. A held
//! Backspace's repeat is in the field before it is laid out.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use super::on_screen_keyboard_cursor::{
    android, count_ticks, ink, iphone_area, screen, space, text_area, text_field, wait, Finger,
    FINGER, IPHONE,
};
use super::on_screen_keyboard_touch::{center, char_key, font, key_of, Phone};
use crate::input_profile::InputProfile;
use crate::touch_state::TouchId;
use crate::widgets::on_screen_keyboard::cursor_slide::{
    DELETE_WORD_STEP, ENGAGE_DISTANCE, SPACE_STEP, TRACKPAD_CHAR_STEP, TRACKPAD_HOLD,
    TRACKPAD_LINE_STEP,
};
use crate::widgets::on_screen_keyboard::test_hook;
use crate::widgets::TextField;
use crate::Key;

fn backspace() -> crate::geometry::Point {
    center(key_of("Backspace").rect)
}

/// The field's selection and caret.
fn field_state(phone: &mut Phone) -> (String, usize) {
    let f = text_field(phone);
    (f.selection(), f.cursor_pos())
}

#[test]
fn giving_every_word_back_restores_the_caret_mid_text() {
    let _clock = crate::clock::scoped_virtual(None);
    let mut phone = android("one two three", 7);
    let start = backspace();
    let mut finger = Finger::down(&mut phone, start);
    finger.slide_by(&mut phone, -DELETE_WORD_STEP - 1.0, 0.0);
    assert_eq!(field_state(&mut phone), ("two".into(), 4));
    finger.slide_to(&mut phone, start);
    assert_eq!(
        field_state(&mut phone),
        (String::new(), 7),
        "nothing selected"
    );
    finger.lift(&mut phone);
    assert_eq!(text_field(&mut phone).text(), "one two three");
    // The next key types at the caret; no stray selection eats a space.
    let x = Finger::down(&mut phone, center(char_key('x').rect));
    x.lift(&mut phone);
    assert_eq!(text_field(&mut phone).text(), "one twox three");
}

#[test]
fn giving_every_word_back_restores_the_caret_mid_word() {
    let _clock = crate::clock::scoped_virtual(None);
    let mut phone = android("one two three", 5);
    let start = backspace();
    let mut finger = Finger::down(&mut phone, start);
    finger.slide_by(&mut phone, -DELETE_WORD_STEP - 1.0, 0.0);
    assert_eq!(field_state(&mut phone), ("t".into(), 4));
    finger.slide_by(&mut phone, -DELETE_WORD_STEP, 0.0);
    assert_eq!(field_state(&mut phone), ("one t".into(), 0));
    finger.slide_by(&mut phone, DELETE_WORD_STEP + 1.0, 0.0);
    assert_eq!(field_state(&mut phone), ("t".into(), 4), "one word back");
    finger.slide_to(&mut phone, start);
    assert_eq!(field_state(&mut phone), (String::new(), 5));
    finger.lift(&mut phone);
    assert_eq!(text_field(&mut phone).text(), "one two three");
}

#[test]
fn steps_past_the_start_of_the_text_select_nothing_more() {
    let _clock = crate::clock::scoped_virtual(None);
    let mut phone = android("ab cd", 5);
    let ticks = count_ticks();
    let mut finger = Finger::down(&mut phone, backspace());
    finger.slide_by(&mut phone, -4.0 * DELETE_WORD_STEP - 1.0, 0.0);
    assert_eq!(field_state(&mut phone), ("ab cd".into(), 0));
    assert_eq!(
        ticks.get(),
        2,
        "a tick per word selected, none past the start"
    );
    // One step back gives a word back at once.
    finger.slide_by(&mut phone, DELETE_WORD_STEP + 1.0, 0.0);
    assert_eq!(field_state(&mut phone), ("cd".into(), 3));
    assert_eq!(ticks.get(), 3, "one tick for the word given back");
    finger.lift(&mut phone);
    assert_eq!(text_field(&mut phone).text(), "ab ");
}

#[test]
fn a_short_flick_left_on_backspace_is_still_a_backspace() {
    let _clock = crate::clock::scoped_virtual(None);
    let mut phone = android("abc", 3);
    let mut finger = Finger::down(&mut phone, backspace());
    finger.slide_by(&mut phone, -(DELETE_WORD_STEP - 2.0), 0.0);
    const { assert!(DELETE_WORD_STEP - 2.0 > ENGAGE_DISTANCE) };
    finger.lift(&mut phone);
    assert_eq!(text_field(&mut phone).text(), "ab");
}

#[test]
fn the_trackpad_sends_no_line_steps_to_a_one_line_field() {
    let _clock = crate::clock::scoped_virtual(None);
    let vertical = Rc::new(Cell::new(0));
    let seen = Rc::clone(&vertical);
    let field = TextField::new(font())
        .with_text("12")
        .with_key_intercept(move |k, _| {
            if matches!(k, Key::ArrowUp | Key::ArrowDown) {
                seen.set(seen.get() + 1);
                return true;
            }
            false
        });
    let mut phone = Phone::with(Box::new(field), false, InputProfile::MobileIOS, IPHONE);
    text_field(&mut phone).set_cursor_position(2);
    let ticks = count_ticks();
    let mut finger = Finger::down(&mut phone, space());
    wait(&mut phone, TRACKPAD_HOLD + Duration::from_millis(100));
    assert!(test_hook::trackpad_active());
    assert_eq!(
        ticks.get(),
        1,
        "a tick as the trackpad starts, as iOS plays"
    );
    finger.slide_by(&mut phone, 0.0, 3.0 * TRACKPAD_LINE_STEP + 1.0);
    assert_eq!(vertical.get(), 0, "no Up or Down reached the field");
    assert_eq!(ticks.get(), 1, "no tick for a step that went nowhere");
    // Sideways still moves, a character a step and a tick each, and stops
    // at the start of the text.
    finger.slide_by(&mut phone, -3.0 * TRACKPAD_CHAR_STEP - 1.0, 0.0);
    assert_eq!(text_field(&mut phone).cursor_pos(), 0);
    assert_eq!(ticks.get(), 3);
    finger.lift(&mut phone);
    assert_eq!(text_field(&mut phone).text(), "12");
}

#[test]
fn a_space_bar_slide_ticks_only_while_the_caret_moves() {
    let _clock = crate::clock::scoped_virtual(None);
    let mut phone = android("ab", 2);
    let ticks = count_ticks();
    let mut finger = Finger::down(&mut phone, space());
    finger.slide_by(&mut phone, 3.0 * SPACE_STEP + 1.0, 0.0);
    assert_eq!(ticks.get(), 0, "the caret is already at the end");
    finger.slide_by(&mut phone, -6.0 * SPACE_STEP, 0.0);
    assert_eq!(text_field(&mut phone).cursor_pos(), 0);
    assert_eq!(ticks.get(), 2, "two characters to move over");
    finger.lift(&mut phone);
}

#[test]
fn a_finger_sliding_up_off_the_space_bar_types_the_key_it_lifts_on() {
    let _clock = crate::clock::scoped_virtual(None);
    let mut phone = android("", 0);
    let mut finger = Finger::down(&mut phone, space());
    finger.slide_to(&mut phone, center(char_key('v').rect));
    finger.lift(&mut phone);
    assert_eq!(text_field(&mut phone).text(), "v");
}

#[test]
fn the_trackpad_ends_when_the_finger_lifts_off_the_keyboard() {
    let _clock = crate::clock::scoped_virtual(None);
    let mut phone = iphone_area("abc\ndef\nghi");
    let mut finger = Finger::down(&mut phone, space());
    wait(&mut phone, TRACKPAD_HOLD + Duration::from_millis(100));
    finger.slide_by(&mut phone, 2.0 * TRACKPAD_CHAR_STEP + 1.0, 600.0);
    finger.lift(&mut phone);
    assert!(!test_hook::trackpad_active());
    assert!(
        ink(&phone.paint(), char_key('q').rect) > 0,
        "the letters are back"
    );
    let area = text_area(&mut phone);
    assert_eq!(area.text(), "abc\ndef\nghi", "nothing typed");
    assert_eq!(
        area.char_index_to_insert_before(),
        2,
        "the caret stays moved"
    );
}

/// A second finger lands on `q` and lifts, while the first stays down.
fn second_finger_taps_q(phone: &mut Phone) {
    let (x, y) = screen(phone, center(char_key('q').rect));
    phone.app.on_touch_start(FINGER.0, TouchId(2), x, y, None);
    phone.app.on_touch_end(FINGER.0, TouchId(2));
    phone.paint();
}

#[test]
fn a_second_finger_cancels_a_backspace_slide() {
    let _clock = crate::clock::scoped_virtual(None);
    let mut phone = android("one two three", 13);
    let mut finger = Finger::down(&mut phone, backspace());
    finger.slide_by(&mut phone, -DELETE_WORD_STEP - 1.0, 0.0);
    assert_eq!(field_state(&mut phone), ("three".into(), 8));
    second_finger_taps_q(&mut phone);
    // The slide is cancelled (nothing deleted, the caret back) and the
    // second finger types its key.
    assert_eq!(text_field(&mut phone).text(), "one two threeq");
    finger.lift(&mut phone);
    assert_eq!(text_field(&mut phone).text(), "one two threeq");
}

#[test]
fn a_second_finger_cancels_the_trackpad() {
    let _clock = crate::clock::scoped_virtual(None);
    let mut phone = iphone_area("abc");
    let mut finger = Finger::down(&mut phone, space());
    wait(&mut phone, TRACKPAD_HOLD + Duration::from_millis(100));
    finger.slide_by(&mut phone, 2.0 * TRACKPAD_CHAR_STEP + 1.0, 0.0);
    assert_eq!(text_area(&mut phone).char_index_to_insert_before(), 2);
    second_finger_taps_q(&mut phone);
    assert!(!test_hook::trackpad_active());
    assert_eq!(
        text_area(&mut phone).text(),
        "qabc",
        "the caret went back first"
    );
    finger.lift(&mut phone);
    assert_eq!(text_area(&mut phone).text(), "qabc");
}

#[test]
fn a_second_finger_ends_a_space_bar_slide_where_it_got_to() {
    let _clock = crate::clock::scoped_virtual(None);
    let mut phone = android("abc", 0);
    let mut finger = Finger::down(&mut phone, space());
    finger.slide_by(&mut phone, 2.0 * SPACE_STEP + 1.0, 0.0);
    second_finger_taps_q(&mut phone);
    assert_eq!(
        text_field(&mut phone).text(),
        "abqc",
        "no space, caret kept"
    );
    finger.lift(&mut phone);
    assert_eq!(text_field(&mut phone).text(), "abqc");
}

fn cancel(phone: &mut Phone, finger: Finger) {
    let _ = finger;
    phone.app.on_touch_cancel(FINGER.0, FINGER.1);
    phone.paint();
}

#[test]
fn a_cancelled_touch_cancels_a_backspace_slide() {
    let _clock = crate::clock::scoped_virtual(None);
    let mut phone = android("one two three", 13);
    let mut finger = Finger::down(&mut phone, backspace());
    finger.slide_by(&mut phone, -2.0 * DELETE_WORD_STEP - 1.0, 0.0);
    assert_eq!(field_state(&mut phone), ("two three".into(), 4));
    cancel(&mut phone, finger);
    assert_eq!(field_state(&mut phone), (String::new(), 13));
    assert_eq!(text_field(&mut phone).text(), "one two three");
}

#[test]
fn a_cancelled_touch_cancels_the_trackpad() {
    let _clock = crate::clock::scoped_virtual(None);
    let mut phone = iphone_area("abc");
    let mut finger = Finger::down(&mut phone, space());
    wait(&mut phone, TRACKPAD_HOLD + Duration::from_millis(100));
    finger.slide_by(&mut phone, 2.0 * TRACKPAD_CHAR_STEP + 1.0, 0.0);
    cancel(&mut phone, finger);
    assert!(!test_hook::trackpad_active());
    let area = text_area(&mut phone);
    assert_eq!(area.char_index_to_insert_before(), 0, "the caret went back");
    assert_eq!(area.text(), "abc");
}

#[test]
fn a_cancelled_touch_ends_a_space_bar_slide_where_it_got_to() {
    let _clock = crate::clock::scoped_virtual(None);
    let mut phone = android("abc", 0);
    let mut finger = Finger::down(&mut phone, space());
    finger.slide_by(&mut phone, 2.0 * SPACE_STEP + 1.0, 0.0);
    cancel(&mut phone, finger);
    assert_eq!(field_state(&mut phone), (String::new(), 2));
    assert_eq!(text_field(&mut phone).text(), "abc", "no space");
}

#[test]
fn a_held_backspace_repeat_is_in_the_field_before_it_is_laid_out() {
    let _clock = crate::clock::scoped_virtual(None);
    let mut phone = android("abc", 3);
    let _finger = Finger::down(&mut phone, backspace());
    wait(&mut phone, Duration::from_millis(400));
    crate::clock::advance(Duration::from_millis(60));
    // Layout alone, as a frame starts: the repeat is due and is delivered.
    let size = phone.size;
    phone.app.layout(size);
    assert_eq!(text_field(&mut phone).text(), "ab");
}
