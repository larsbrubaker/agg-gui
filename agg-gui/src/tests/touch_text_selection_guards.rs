//! Touch text selection in a [`TextField`] at its edges (harness in
//! `touch_text_selection.rs`): a password is selected whole and never
//! leaves the field by any copy path; a long press picks the character
//! under the finger, never the one after it; handles move by whole
//! user-perceived characters and a long press on an emoji takes just the
//! emoji; a swipe that starts on a toolbar button presses nothing; a mouse
//! click on a finger's leftovers is swallowed; read-only and disabled
//! fields; Paste only with something to paste; the caret handle's fade.

use super::touch_text_selection::{
    button, char_at, font, handle, phone, selection, shows, Phone, FIELD, FINGER, H, TEXT,
};
use crate::geometry::Point;
use crate::input_profile::InputProfile;
use crate::widgets::text_context_menu::TextMenuAction;
use crate::widgets::touch_selection::{probe, text_units, HandleKind, TouchSelectable};
use crate::{Key, Modifiers, MouseButton, TextField};

fn field_phone(field: TextField) -> Phone {
    Phone::new(InputProfile::MobileAndroid, FIELD, Box::new(field))
}

/// The window point `frac` of the way across character `byte`.
fn across(p: &mut Phone, byte: usize, frac: f64) -> Point {
    let w = p.widget::<TextField>();
    let here = w.ts_caret_line(byte);
    let next = w.ts_caret_line(byte + 1);
    let local = Point::new(here.x + (next.x - here.x) * frac, here.middle().y);
    p.window(local)
}

/// The window point midway between the carets at `a` and `b`.
fn between(p: &mut Phone, a: usize, b: usize) -> Point {
    let w = p.widget::<TextField>();
    let (la, lb) = (w.ts_caret_line(a), w.ts_caret_line(b));
    let local = Point::new((la.x + lb.x) * 0.5, la.middle().y);
    p.window(local)
}

fn long_press_and_lift(p: &mut Phone, at: Point) {
    p.long_press(at);
    FINGER.up(&mut p.app);
    p.frame();
}

fn key(p: &mut Phone, key: Key, mods: Modifiers) {
    p.app.on_key_down(key, mods);
}

#[test]
fn a_password_is_selected_whole_and_its_toolbar_cannot_copy_it() {
    let field = TextField::new(font())
        .with_text("hunter two")
        .with_font_size(16.0)
        .with_password_mode(true);
    let mut p = field_phone(field);
    let at = char_at::<TextField>(&mut p, 2);
    long_press_and_lift(&mut p, at);
    assert_eq!(selection(&mut p), (0, 10), "all of it, not a word");
    assert_eq!(
        probe::buttons(p.widget::<TextField>()),
        vec![TextMenuAction::Paste],
        "no Cut, no Copy (and everything is already selected)"
    );
}

#[test]
fn no_shortcut_or_menu_copies_or_cuts_a_password() {
    let field = TextField::new(font())
        .with_text("hunter two")
        .with_font_size(16.0)
        .with_password_mode(true);
    let mut p = field_phone(field);
    let at = char_at::<TextField>(&mut p, 2);
    p.tap(at);
    let ctrl = Modifiers {
        ctrl: true,
        ..Modifiers::default()
    };
    let shift = Modifiers {
        shift: true,
        ..Modifiers::default()
    };
    key(&mut p, Key::Char('a'), ctrl);
    crate::clipboard::set_text("SENTINEL");
    key(&mut p, Key::Char('c'), ctrl);
    key(&mut p, Key::Insert, ctrl);
    key(&mut p, Key::Char('x'), ctrl);
    key(&mut p, Key::Delete, shift);
    assert_eq!(crate::clipboard::get_text().as_deref(), Some("SENTINEL"));
    assert_eq!(
        p.widget::<TextField>().text(),
        "hunter two",
        "nothing was cut"
    );
    // The right-click menu offers neither.
    let at = char_at::<TextField>(&mut p, 2);
    p.app
        .on_mouse_down(at.x, H - at.y, MouseButton::Right, Modifiers::default());
    let f = p.widget::<TextField>();
    assert_eq!(f.menu_action_enabled(TextMenuAction::Copy), Some(false));
    assert_eq!(f.menu_action_enabled(TextMenuAction::Cut), Some(false));
    assert_eq!(f.menu_action_enabled(TextMenuAction::Paste), Some(true));
}

#[test]
fn a_long_press_picks_the_character_under_the_finger_not_the_one_after() {
    let mut p = phone(InputProfile::MobileAndroid);
    let at = across(&mut p, 4, 0.8); // the right of the "o" of "hello"
    long_press_and_lift(&mut p, at);
    assert_eq!(selection(&mut p), (0, 5), "\"hello\", not the space");
    drop(p);
    let mut p = phone(InputProfile::MobileAndroid);
    let at = across(&mut p, 20, 0.8); // the right of the final "d"
    long_press_and_lift(&mut p, at);
    assert_eq!(selection(&mut p), (16, 21), "\"world\"");
    assert_eq!(shows(&mut p), (true, false, true));
    assert_eq!(TEXT.len(), 21);
}

#[test]
fn handles_move_by_whole_characters_and_an_emoji_is_its_own_word() {
    let thumbs = "👍🏽";
    let flag = "🇨🇦";
    let text = format!("hi {thumbs} yo {flag} cafe\u{0301}");
    let field = TextField::new(font())
        .with_text(text.clone())
        .with_font_size(16.0);
    let mut p = field_phone(field);
    let bounds = text_units::boundaries(&text);
    // A long press on the emoji selects just the emoji.
    let at = between(&mut p, 3, 3 + thumbs.len());
    let wide = between(&mut p, 0, 3).x;
    assert!(at.x > wide, "the emoji has width in the test font");
    long_press_and_lift(&mut p, at);
    let emoji = (3, 3 + thumbs.len());
    assert_eq!(selection(&mut p), emoji);
    // The end handle, dragged by small steps, only ever stops on cluster
    // boundaries (never inside the skin tone or the flag).
    for step in 1..12 {
        let from = handle::<TextField>(&mut p, HandleKind::End);
        p.drag(from, Point::new(from.x + 11.0 + step as f64, from.y));
        let (lo, hi) = selection(&mut p);
        assert!(bounds.contains(&lo) && bounds.contains(&hi), "{lo}..{hi}");
    }
    // The start handle dragged onto the end stops one whole cluster short.
    let from = handle::<TextField>(&mut p, HandleKind::Start);
    p.drag(from, Point::new(from.x + 400.0, from.y));
    let (lo, hi) = selection(&mut p);
    assert_eq!(lo, text_units::prev(&text, hi));
    // The accent written as a combining mark stays with its word.
    drop(p);
    let field = TextField::new(font())
        .with_text(text.clone())
        .with_font_size(16.0);
    let mut p = field_phone(field);
    let cafe = text.find("cafe").expect("cafe");
    let at = char_at::<TextField>(&mut p, cafe + 1);
    long_press_and_lift(&mut p, at);
    assert_eq!(selection(&mut p), (cafe, text.len()));
}

#[test]
fn a_swipe_that_starts_on_a_toolbar_button_presses_nothing() {
    let mut p = phone(InputProfile::MobileAndroid);
    let at = char_at::<TextField>(&mut p, 8);
    long_press_and_lift(&mut p, at);
    let cut = button::<TextField>(&mut p, TextMenuAction::Cut);
    crate::clock::advance(std::time::Duration::from_secs(1));
    FINGER.down(&mut p.app, cut.x, cut.y);
    FINGER.move_to(&mut p.app, cut.x + 40.0, cut.y);
    FINGER.up(&mut p.app);
    p.frame();
    assert_eq!(p.widget::<TextField>().text(), TEXT, "Cut did not run");
    assert_eq!(selection(&mut p), (6, 11));
    // A tap on it still cuts.
    let cut = button::<TextField>(&mut p, TextMenuAction::Cut);
    p.tap(cut);
    assert_eq!(p.widget::<TextField>().text(), "hello  new world");
}

#[test]
fn a_mouse_click_on_a_fingers_leftovers_is_swallowed() {
    let mut p = phone(InputProfile::MobileAndroid);
    let at = char_at::<TextField>(&mut p, 8);
    long_press_and_lift(&mut p, at);
    let copy = button::<TextField>(&mut p, TextMenuAction::Copy);
    crate::clipboard::set_text("SENTINEL");
    let m = Modifiers::default();
    p.app
        .on_mouse_down(copy.x, H - copy.y, MouseButton::Left, m);
    p.app.on_mouse_up(copy.x, H - copy.y, MouseButton::Left, m);
    assert_eq!(selection(&mut p), (6, 11), "the caret did not jump");
    assert_eq!(shows(&mut p), (false, false, false), "the leftovers went");
    assert_eq!(crate::clipboard::get_text().as_deref(), Some("SENTINEL"));
}

#[test]
fn a_read_only_field_selects_and_copies_but_offers_no_edits_or_caret_handle() {
    let field = TextField::new(font())
        .with_text(TEXT)
        .with_font_size(16.0)
        .with_read_only(true);
    let mut p = field_phone(field);
    let at = char_at::<TextField>(&mut p, 8);
    long_press_and_lift(&mut p, at);
    assert_eq!(selection(&mut p), (6, 11));
    assert_eq!(
        probe::buttons(p.widget::<TextField>()),
        vec![TextMenuAction::Copy, TextMenuAction::SelectAll]
    );
    let copy = button::<TextField>(&mut p, TextMenuAction::Copy);
    p.tap(copy);
    assert_eq!(crate::clipboard::get_text().as_deref(), Some("brave"));
    let at = char_at::<TextField>(&mut p, 2);
    p.tap(at);
    assert_eq!(shows(&mut p), (false, false, false), "no caret handle");
}

#[test]
fn paste_is_offered_only_with_text_on_the_clipboard() {
    let mut p = phone(InputProfile::MobileAndroid);
    crate::clipboard::set_text("");
    let at = char_at::<TextField>(&mut p, 8);
    long_press_and_lift(&mut p, at);
    assert!(!probe::buttons(p.widget::<TextField>()).contains(&TextMenuAction::Paste));
}

#[test]
fn a_field_that_becomes_disabled_drops_its_handles() {
    let mut p = phone(InputProfile::MobileAndroid);
    let at = char_at::<TextField>(&mut p, 8);
    long_press_and_lift(&mut p, at);
    p.widget::<TextField>().set_enabled(false);
    p.frame();
    assert_eq!(shows(&mut p), (false, false, false));
    assert!(probe::handle_center(p.widget::<TextField>(), HandleKind::End).is_none());
}

#[test]
fn the_caret_handle_schedules_its_own_fade() {
    let mut p = phone(InputProfile::MobileAndroid);
    let at = char_at::<TextField>(&mut p, 2);
    p.tap(at);
    let until = p
        .widget::<TextField>()
        .touch_selection_state()
        .deadline()
        .expect("a fade is due");
    assert_eq!(
        until,
        crate::clock::now() + crate::widgets::touch_selection::CARET_HANDLE_TIME
    );
    crate::clock::advance(crate::widgets::touch_selection::CARET_HANDLE_TIME);
    assert!(p.widget::<TextField>().touch_selection_state().needs_draw());
    p.frame();
    let ts = p.widget::<TextField>().touch_selection_state();
    assert!(
        !ts.needs_draw() && ts.deadline().is_none(),
        "faded and idle"
    );
}

#[test]
fn in_a_browser_paste_is_always_offered_and_reads_the_system_clipboard_on_tap() {
    let reads = crate::clipboard::system_paste::simulate();
    let mut p = phone(InputProfile::MobileAndroid);
    // The page's own clipboard is empty: nothing was copied in the app.
    crate::clipboard::set_text("");
    let at = char_at::<TextField>(&mut p, 8);
    long_press_and_lift(&mut p, at);
    let paste = button::<TextField>(&mut p, TextMenuAction::Paste);
    p.tap(paste);
    assert_eq!(
        reads.pending(),
        1,
        "the tap started a system clipboard read"
    );
    assert_eq!(p.widget::<TextField>().text(), TEXT, "nothing yet");
    reads.resolve(Some("bold"));
    p.frame();
    assert_eq!(p.widget::<TextField>().text(), "hello bold new world");
    // A refused read pastes nothing.
    let at = char_at::<TextField>(&mut p, 2);
    long_press_and_lift(&mut p, at);
    let paste = button::<TextField>(&mut p, TextMenuAction::Paste);
    p.tap(paste);
    reads.resolve(None);
    p.frame();
    assert_eq!(p.widget::<TextField>().text(), "hello bold new world");
}
