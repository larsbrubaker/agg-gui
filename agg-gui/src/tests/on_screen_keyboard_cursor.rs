//! Moving the caret from the on-screen keyboard
//! (`widgets/on_screen_keyboard/cursor_slide.rs`), driven through the real
//! `App` with touch events on the virtual UI clock.
//!
//! On Android (Gboard) a finger sliding along the space bar moves the caret a
//! character a step, with a haptic tick each, and the slide types no space; a
//! press that only wobbles still types one. Sliding left from Backspace
//! selects whole words, which the lift deletes, and a held Backspace still
//! repeats. On the iPhone a held space bar turns the keyboard into a trackpad:
//! the letters fade, the finger moves the caret by character and by line, and
//! the lift puts the keys back. A slide also ends a dictation, as any key does.
//! Helpers come from `on_screen_keyboard_touch.rs`.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use super::on_screen_keyboard_touch::{center, char_key, font, key_of, Phone};
use crate::geometry::{Point, Rect};
use crate::input_profile::InputProfile;
use crate::touch_state::{TouchDeviceId, TouchId};
use crate::widgets::on_screen_keyboard::cursor_slide::{
    DELETE_WORD_STEP, ENGAGE_DISTANCE, SPACE_STEP, TRACKPAD_CHAR_STEP, TRACKPAD_HOLD,
    TRACKPAD_LINE_STEP,
};
use crate::widgets::on_screen_keyboard::{self as osk, test_hook};
use crate::widgets::{TextArea, TextField};
use crate::{Size, Widget};

pub(super) const IPHONE: Size = Size {
    width: 390.0,
    height: 844.0,
};

pub(super) const FINGER: (TouchDeviceId, TouchId) = (TouchDeviceId(0), TouchId(1));

/// A finger on `phone`'s screen, in the keyboard's Y-up logical coordinates.
pub(super) struct Finger {
    pub(super) at: Point,
}

/// Y-up logical `p` as the Y-down screen point (CSS px) a touch arrives at.
pub(super) fn screen(phone: &Phone, p: Point) -> (f64, f64) {
    let zoom = crate::ux_scale::ux_scale();
    (p.x * zoom, phone.size.height - p.y * zoom)
}

impl Finger {
    pub(super) fn down(phone: &mut Phone, at: Point) -> Finger {
        let (x, y) = screen(phone, at);
        phone.app.on_touch_start(FINGER.0, FINGER.1, x, y, None);
        Finger { at }
    }

    /// Slide to `to` in 2-point moves, as a real finger reports.
    pub(super) fn slide_to(&mut self, phone: &mut Phone, to: Point) {
        let (dx, dy) = (to.x - self.at.x, to.y - self.at.y);
        let n = (dx.abs().max(dy.abs()) / 2.0).ceil().max(1.0) as usize;
        for i in 1..=n {
            let t = i as f64 / n as f64;
            let p = Point::new(self.at.x + dx * t, self.at.y + dy * t);
            let (x, y) = screen(phone, p);
            phone.app.on_touch_move(FINGER.0, FINGER.1, x, y, None);
        }
        self.at = to;
    }

    pub(super) fn slide_by(&mut self, phone: &mut Phone, dx: f64, dy: f64) {
        let to = Point::new(self.at.x + dx, self.at.y + dy);
        self.slide_to(phone, to);
    }

    pub(super) fn lift(self, phone: &mut Phone) {
        phone.app.on_touch_end(FINGER.0, FINGER.1);
        phone.paint();
    }
}

/// Paint a frame every 50 ms for `held`, as the app would while a finger rests.
pub(super) fn wait(phone: &mut Phone, held: Duration) {
    let mut t = Duration::ZERO;
    while t < held {
        let step = Duration::from_millis(50).min(held - t);
        crate::clock::advance(step);
        t += step;
        phone.paint();
    }
}

pub(super) fn text_field(phone: &mut Phone) -> &mut TextField {
    phone
        .app
        .root_mut()
        .as_any_mut()
        .and_then(|a| a.downcast_mut::<TextField>())
        .expect("the root is the field")
}

pub(super) fn text_area(phone: &mut Phone) -> &mut TextArea {
    phone
        .app
        .root_mut()
        .as_any_mut()
        .and_then(|a| a.downcast_mut::<TextArea>())
        .expect("the root is the text area")
}

/// An Android phone typing into a field holding `text`, its caret before
/// character `caret`.
pub(super) fn android(text: &str, caret: usize) -> Phone {
    let root: Box<dyn Widget> = Box::new(TextField::new(font()).with_text(text));
    let mut phone = Phone::new(root, false);
    text_field(&mut phone).set_cursor_position(caret);
    phone
}

pub(super) fn space() -> Point {
    center(key_of("Space").rect)
}

/// Counts the keyboard's haptic ticks from now on.
pub(super) fn count_ticks() -> Rc<Cell<usize>> {
    let ticks = Rc::new(Cell::new(0));
    let counter = Rc::clone(&ticks);
    osk::on_haptic_tick(move || counter.set(counter.get() + 1));
    ticks
}

#[test]
fn sliding_along_the_space_bar_moves_the_caret_a_character_a_step() {
    let _clock = crate::clock::scoped_virtual(None);
    let mut phone = android("hello world", 0);
    let ticks = count_ticks();
    let mut finger = Finger::down(&mut phone, space());
    finger.slide_by(&mut phone, 3.0 * SPACE_STEP + 2.0, 1.0);
    // The caret moves while the finger slides, not when it lifts.
    assert_eq!(text_field(&mut phone).cursor_pos(), 3);
    assert_eq!(ticks.get(), 3, "a haptic tick each step");
    // Back two steps (counted from the last step, so a finger resting on a
    // step's edge doesn't jitter), then on past the space bar's end.
    finger.slide_by(&mut phone, -2.0 * SPACE_STEP - 2.0, 0.0);
    assert_eq!(text_field(&mut phone).cursor_pos(), 1);
    finger.slide_by(&mut phone, 6.0 * SPACE_STEP, 30.0);
    assert_eq!(text_field(&mut phone).cursor_pos(), 7);
    finger.lift(&mut phone);
    let field = text_field(&mut phone);
    assert_eq!(field.text(), "hello world", "the slide types no space");
    assert_eq!(field.cursor_pos(), 7);
}

#[test]
fn the_steps_are_measured_on_screen_whatever_the_ux_zoom() {
    let _clock = crate::clock::scoped_virtual(None);
    let mut phone = android("hello world", 0);
    // The keyboard keeps the phone's own size under the app's zoom, and so
    // do its steps: three steps of screen travel move three characters.
    let zoom = 1.7;
    crate::ux_scale::set_ux_scale(zoom);
    phone.paint();
    let mut finger = Finger::down(&mut phone, space());
    finger.slide_by(&mut phone, (3.0 * SPACE_STEP + 2.0) / zoom, 0.0);
    let moved = text_field(&mut phone).cursor_pos();
    finger.lift(&mut phone);
    crate::ux_scale::set_ux_scale(1.0);
    assert_eq!(moved, 3);
    drop(phone); // Releases the input-profile lock for the next phone.
                 // A wobble that is short on screen still types its space.
    let mut phone = android("ab", 0);
    crate::ux_scale::set_ux_scale(zoom);
    phone.paint();
    let mut finger = Finger::down(&mut phone, space());
    finger.slide_by(&mut phone, (ENGAGE_DISTANCE - 2.0) / zoom, 0.0);
    finger.lift(&mut phone);
    let text = text_field(&mut phone).text();
    crate::ux_scale::set_ux_scale(1.0);
    assert_eq!(text, " ab");
}

#[test]
fn a_press_on_the_space_bar_that_only_wobbles_still_types_a_space() {
    let _clock = crate::clock::scoped_virtual(None);
    let mut phone = android("hello", 5);
    let ticks = count_ticks();
    let mut finger = Finger::down(&mut phone, space());
    let wobble = ENGAGE_DISTANCE - 2.0;
    finger.slide_by(&mut phone, wobble, 2.0);
    finger.slide_by(&mut phone, -2.0 * wobble, -2.0);
    finger.slide_by(&mut phone, wobble, 0.0);
    finger.lift(&mut phone);
    assert_eq!(text_field(&mut phone).text(), "hello ");
    assert_eq!(ticks.get(), 0);
}

#[test]
fn a_slide_that_comes_back_to_where_it_began_types_no_space() {
    let _clock = crate::clock::scoped_virtual(None);
    let mut phone = android("hello", 2);
    let start = space();
    let mut finger = Finger::down(&mut phone, start);
    finger.slide_by(&mut phone, -SPACE_STEP - 1.0, 0.0);
    assert_eq!(text_field(&mut phone).cursor_pos(), 1);
    finger.slide_to(&mut phone, start);
    finger.lift(&mut phone);
    let field = text_field(&mut phone);
    assert_eq!(field.text(), "hello", "no space after a slide");
    assert_eq!(field.cursor_pos(), 2, "back where the caret began");
}

#[test]
fn a_space_bar_slide_ends_a_dictation_as_a_key_does() {
    use super::on_screen_keyboard_voice::dictation_rig;
    let (mut rig, voice, cell) = dictation_rig();
    rig.start_dictation();
    voice.say("Hello", " there");
    rig.paint();
    let at = space();
    let height = super::on_screen_keyboard_voice::PHONE.height;
    rig.app
        .on_touch_start(FINGER.0, FINGER.1, at.x, height - at.y, None);
    for i in 1..=8 {
        let x = at.x - i as f64 * 2.0;
        rig.app
            .on_touch_move(FINGER.0, FINGER.1, x, height - at.y, None);
    }
    assert!(!osk::is_dictating(), "the slide ended the dictation");
    assert_eq!(voice.count("cancel"), 1);
    rig.app.on_touch_end(FINGER.0, FINGER.1);
    assert_eq!(*cell.borrow(), "Hello there", "the tail kept, no space");
    assert_eq!(rig.field().cursor_pos(), "Hello there".len() - 1);
}

/// A finger held still on Backspace deletes while it is down, at the repeat
/// rate, not all at once when it lifts.
#[test]
fn a_finger_held_still_on_backspace_deletes_while_it_is_down() {
    let _clock = crate::clock::scoped_virtual(None);
    let mut phone = android("abcdefghijkl", 12);
    let finger = Finger::down(&mut phone, center(key_of("Backspace").rect));
    let left = |phone: &mut Phone| text_field(phone).text().len();
    wait(&mut phone, Duration::from_millis(400));
    assert_eq!(left(&mut phone), 12, "nothing before the repeat delay");
    // The first repeat at 450 ms, then one every 70 ms, which frames 50 ms
    // apart see every other frame: 450, 550, ..., 950.
    wait(&mut phone, Duration::from_millis(100));
    assert_eq!(left(&mut phone), 11, "the first repeat, finger still down");
    wait(&mut phone, Duration::from_millis(500));
    assert_eq!(left(&mut phone), 6, "one every 100 ms, finger still down");
    finger.lift(&mut phone);
    assert_eq!(left(&mut phone), 6, "the lift adds no extra Backspace");
}

#[test]
fn a_letter_held_still_types_once() {
    let _clock = crate::clock::scoped_virtual(None);
    let mut phone = android("", 0);
    let finger = Finger::down(&mut phone, center(char_key('b').rect));
    wait(&mut phone, Duration::from_millis(1500));
    assert_eq!(text_field(&mut phone).text(), "", "nothing while held");
    finger.lift(&mut phone);
    assert_eq!(text_field(&mut phone).text(), "b");
}

#[test]
fn a_backspace_that_has_started_repeating_selects_no_words() {
    let _clock = crate::clock::scoped_virtual(None);
    let mut phone = android("abcdefghijkl", 12);
    let mut finger = Finger::down(&mut phone, center(key_of("Backspace").rect));
    // Two repeats (at 450 and 550 ms), then the finger slides left.
    wait(&mut phone, Duration::from_millis(600));
    finger.slide_by(&mut phone, -3.0 * DELETE_WORD_STEP, 0.0);
    assert_eq!(text_field(&mut phone).selection(), "", "no word selection");
    finger.lift(&mut phone);
    // As before the slide gesture: the finger slid onto a letter, which the
    // lift types.
    let text = text_field(&mut phone).text();
    assert!(text.starts_with("abcdefghij"), "{text}");
    assert!(text.len() <= 11, "{text}");
}

#[test]
fn sliding_left_from_backspace_selects_words_and_the_lift_deletes_them() {
    let _clock = crate::clock::scoped_virtual(None);
    let mut phone = android("one two three", 13);
    let mut finger = Finger::down(&mut phone, center(key_of("Backspace").rect));
    finger.slide_by(&mut phone, -2.0 * DELETE_WORD_STEP - 1.0, -3.0);
    assert_eq!(text_field(&mut phone).selection(), "two three");
    // Back one step gives a word back.
    finger.slide_by(&mut phone, DELETE_WORD_STEP + 1.0, 0.0);
    assert_eq!(text_field(&mut phone).selection(), "three");
    finger.lift(&mut phone);
    assert_eq!(text_field(&mut phone).text(), "one two ");
}

#[test]
fn a_backspace_slide_taken_back_deletes_nothing() {
    let _clock = crate::clock::scoped_virtual(None);
    let mut phone = android("one two", 7);
    let start = center(key_of("Backspace").rect);
    let mut finger = Finger::down(&mut phone, start);
    finger.slide_by(&mut phone, -DELETE_WORD_STEP - 1.0, 0.0);
    assert_eq!(text_field(&mut phone).selection(), "two");
    finger.slide_to(&mut phone, start);
    finger.lift(&mut phone);
    assert_eq!(text_field(&mut phone).text(), "one two");
}

/// An iPhone typing into a text area holding `text`, its caret at the start.
pub(super) fn iphone_area(text: &str) -> Phone {
    let root: Box<dyn Widget> = Box::new(TextArea::new(font()).with_text(text));
    let mut phone = Phone::with(root, false, InputProfile::MobileIOS, IPHONE);
    // The labels draw in the app's UI font.
    crate::font_settings::set_system_font(Some(font()));
    let area = text_area(&mut phone);
    area.set_char_index_to_insert_before(0);
    area.set_selection_index_to_start_before(0);
    phone
}

/// How many pixels in the middle of `rect` stand out from its face: a
/// label's ink.
pub(super) fn ink(fb: &crate::Framebuffer, rect: Rect) -> usize {
    let w = fb.width() as usize;
    let px = fb.pixels();
    let at = |x: f64, y: f64| {
        let i = ((y as usize) * w + x as usize) * 4;
        [px[i] as i32, px[i + 1] as i32, px[i + 2] as i32]
    };
    let face = at(rect.x + 3.0, rect.y + rect.height / 2.0);
    let mut count = 0;
    let mut y = rect.y + rect.height * 0.2;
    while y < rect.y + rect.height * 0.8 {
        let mut x = rect.x + rect.width * 0.2;
        while x < rect.x + rect.width * 0.8 {
            let p = at(x, y);
            if (0..3).any(|i| (p[i] - face[i]).abs() >= 96) {
                count += 1;
            }
            x += 1.0;
        }
        y += 1.0;
    }
    count
}

#[test]
fn holding_the_iphone_space_bar_turns_the_keyboard_into_a_trackpad() {
    let _clock = crate::clock::scoped_virtual(None);
    let mut phone = iphone_area("abc\ndef\nghi");
    let q = char_key('q').rect;
    assert!(ink(&phone.paint(), q) > 0, "the letters show");
    let mut finger = Finger::down(&mut phone, space());
    wait(&mut phone, TRACKPAD_HOLD - Duration::from_millis(100));
    assert!(!test_hook::trackpad_active(), "not yet");
    wait(&mut phone, Duration::from_millis(400));
    assert!(test_hook::trackpad_active());
    assert_eq!(ink(&phone.paint(), q), 0, "the letters faded");

    // Right two characters, then down a line (the finger moves down: Y-up).
    finger.slide_by(&mut phone, 2.0 * TRACKPAD_CHAR_STEP + 1.0, 0.0);
    assert_eq!(text_area(&mut phone).char_index_to_insert_before(), 2);
    finger.slide_by(&mut phone, 0.0, -TRACKPAD_LINE_STEP - 1.0);
    assert_eq!(text_area(&mut phone).char_index_to_insert_before(), 6);
    // The whole keyboard is the trackpad: up over the letters and back left.
    finger.slide_by(
        &mut phone,
        -TRACKPAD_CHAR_STEP - 1.0,
        TRACKPAD_LINE_STEP + 1.0,
    );
    assert_eq!(text_area(&mut phone).char_index_to_insert_before(), 1);

    finger.lift(&mut phone);
    assert!(!test_hook::trackpad_active(), "the lift ends it");
    assert!(ink(&phone.paint(), q) > 0, "the letters are back");
    let area = text_area(&mut phone);
    assert_eq!(area.text(), "abc\ndef\nghi", "no space typed");
    assert_eq!(area.char_index_to_insert_before(), 1);
}

#[test]
fn a_tap_or_a_quick_slide_on_the_iphone_space_bar_types_a_space() {
    let _clock = crate::clock::scoped_virtual(None);
    let mut phone = iphone_area("ab");
    let finger = Finger::down(&mut phone, space());
    wait(&mut phone, Duration::from_millis(150));
    finger.lift(&mut phone);
    assert_eq!(text_area(&mut phone).text(), " ab");
    // A finger that moves before the hold is up types, and never becomes a
    // trackpad however long it then rests.
    let mut finger = Finger::down(&mut phone, space());
    finger.slide_by(&mut phone, 3.0 * TRACKPAD_CHAR_STEP, 0.0);
    wait(&mut phone, TRACKPAD_HOLD * 2);
    assert!(!test_hook::trackpad_active());
    finger.lift(&mut phone);
    let area = text_area(&mut phone);
    assert_eq!(area.text(), "  ab");
    assert_eq!(area.char_index_to_insert_before(), 2);
}
