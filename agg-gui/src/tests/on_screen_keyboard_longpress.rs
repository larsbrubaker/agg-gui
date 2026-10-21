//! Long-press alternates and the iPhone keyboard.
//!
//! Holding a key with alternates opens them after the hold time; sliding
//! along the row moves the highlight; lifting types it (in the case Shift
//! gives) or, away from the row, nothing. Keys without alternates type once
//! however long they are held, and Backspace still repeats. On the
//! `MobileIOS` profile the keyboard is the iPhone's: no number row,
//! `123 · space · return`, Enter labelled with a word, the emoji key below
//! the keys, every point still belonging to a key, and the exact height
//! reported. Driven on the virtual UI clock through the keyboard's pointer
//! handlers, painting through the real `App` (helpers from
//! `on_screen_keyboard_touch.rs`).

use std::time::Duration;

use super::on_screen_keyboard_touch::{
    center, char_key, field, font, key_of, keys, panel_height, Phone,
};
use crate::geometry::Point;
use crate::input_profile::InputProfile;
use crate::widgets::on_screen_keyboard::alternates::LONG_PRESS_DELAY;
use crate::widgets::on_screen_keyboard::state::with_state_ref;
use crate::widgets::on_screen_keyboard::{
    self as osk, drain_synthetic_keys, test_hook, EnterAction, KeyAction, KeyCap,
};
use crate::widgets::TextField;
use crate::{Key, Modifiers, MouseButton, Size};

const IPHONE: Size = Size {
    width: 390.0,
    height: 844.0,
};

fn down(p: Point) {
    assert!(osk::handle_software_keyboard_mouse_down(
        p,
        MouseButton::Left,
        Modifiers::default()
    ));
}

fn up(p: Point) -> Vec<Key> {
    osk::handle_software_keyboard_mouse_up(p, MouseButton::Left, Modifiers::default());
    drain_synthetic_keys().into_iter().map(|(k, _)| k).collect()
}

/// Hold `p` for `held`, painting a frame every 50 ms as the app would.
fn hold(phone: &mut Phone, p: Point, held: Duration) {
    down(p);
    let mut t = Duration::ZERO;
    while t < held {
        let step = Duration::from_millis(50).min(held - t);
        crate::clock::advance(step);
        t += step;
        phone.paint();
    }
}

/// The open popup's characters and highlight.
fn popup() -> Option<(String, Option<usize>)> {
    with_state_ref(|s| {
        s.alternates
            .as_ref()
            .map(|p| (p.chars.iter().collect(), p.highlighted))
    })
}

fn popup_cell(i: usize) -> Point {
    with_state_ref(|s| center(s.alternates.as_ref().expect("popup open").cells[i]))
}

#[test]
fn holding_a_letter_opens_its_alternates_and_lifting_types_the_default() {
    let _clock = crate::clock::scoped_virtual(None);
    let mut phone = Phone::new(field(), false);
    let e = center(char_key('e').rect);
    hold(&mut phone, e, LONG_PRESS_DELAY - Duration::from_millis(30));
    assert_eq!(popup(), None, "not yet");
    hold_more(&mut phone, Duration::from_millis(60));
    assert_eq!(popup(), Some(("êéèëē".into(), Some(1))));
    assert!(matches!(up(e)[..], [Key::Char('é')]));
    assert_eq!(popup(), None, "lifting closes the popup");
}

fn hold_more(phone: &mut Phone, by: Duration) {
    crate::clock::advance(by);
    phone.paint();
}

#[test]
fn sliding_along_the_alternates_moves_the_highlight() {
    let _clock = crate::clock::scoped_virtual(None);
    let mut phone = Phone::new(field(), false);
    let e = center(char_key('e').rect);
    hold(&mut phone, e, LONG_PRESS_DELAY + Duration::from_millis(20));
    osk::handle_software_keyboard_mouse_move(popup_cell(3));
    assert_eq!(popup().unwrap().1, Some(3));
    let ee = popup_cell(0);
    osk::handle_software_keyboard_mouse_move(ee);
    assert_eq!(popup().unwrap().1, Some(0));
    phone.paint();
    assert!(matches!(up(ee)[..], [Key::Char('ê')]));
}

#[test]
fn alternates_follow_shift() {
    let _clock = crate::clock::scoped_virtual(None);
    let mut phone = Phone::new(field(), false);
    test_hook::simulate_shift_tap();
    phone.paint();
    let e = center(char_key('E').rect);
    hold(&mut phone, e, LONG_PRESS_DELAY + Duration::from_millis(20));
    assert_eq!(popup(), Some(("ÊÉÈËĒ".into(), Some(1))));
    assert!(matches!(up(e)[..], [Key::Char('É')]));
    // The capital was one-shot, as for a tapped letter.
    assert_eq!(
        test_hook::current_layer(),
        crate::widgets::on_screen_keyboard::KeyboardLayer::Letters
    );
}

#[test]
fn lifting_away_from_the_alternates_types_nothing() {
    let _clock = crate::clock::scoped_virtual(None);
    let mut phone = Phone::new(field(), false);
    let a = center(char_key('a').rect);
    hold(&mut phone, a, LONG_PRESS_DELAY + Duration::from_millis(20));
    assert!(popup().is_some());
    let far = Point::new(a.x, panel_height() + 200.0);
    osk::handle_software_keyboard_mouse_move(far);
    assert_eq!(popup().unwrap().1, None);
    assert!(up(far).is_empty());
    assert_eq!(popup(), None);
}

#[test]
fn a_key_without_alternates_types_once_however_long_it_is_held() {
    let _clock = crate::clock::scoped_virtual(None);
    let mut phone = Phone::new(field(), false);
    let b = center(char_key('b').rect);
    hold(&mut phone, b, Duration::from_millis(1500));
    assert_eq!(popup(), None);
    assert!(drain_synthetic_keys().is_empty(), "nothing repeats");
    assert!(matches!(up(b)[..], [Key::Char('b')]));
    // Backspace still repeats while held.
    let bs = center(key_of("Backspace").rect);
    hold(&mut phone, bs, Duration::from_millis(1000));
    let repeated = drain_synthetic_keys();
    assert!(repeated.len() >= 3, "Backspace repeats: {}", repeated.len());
    assert!(repeated.iter().all(|(k, _)| matches!(k, Key::Backspace)));
    assert!(up(bs).is_empty(), "the lift adds no extra Backspace");
}

fn iphone(enter: EnterAction, dark: bool) -> Phone {
    let root = Box::new(TextField::new(font()).with_enter_action(enter));
    Phone::with(root, dark, InputProfile::MobileIOS, IPHONE)
}

fn cap_text(cap: &KeyCap) -> String {
    match cap {
        KeyCap::Text(t) => t.clone(),
        KeyCap::Glyph(g) => format!("{g:?}"),
    }
}

#[test]
fn the_iphone_keyboard_has_no_number_row_and_its_own_bottom_row() {
    let _phone = iphone(EnterAction::Newline, false);
    let ks = keys();
    assert!(
        !ks.iter().any(|k| matches!(k.action, KeyAction::Char('1'))),
        "no number row"
    );
    let top = ks
        .iter()
        .map(|k| k.rect.y)
        .fold(f64::NEG_INFINITY, f64::max);
    let top_row: String = ks
        .iter()
        .filter(|k| k.rect.y == top)
        .map(|k| cap_text(&k.cap))
        .collect();
    assert_eq!(top_row, "qwertyuiop");
    let lowest = ks
        .iter()
        .filter(|k| !matches!(k.cap, KeyCap::Glyph(_)) || k.rect.width > 40.0)
        .map(|k| k.rect.y)
        .fold(f64::INFINITY, f64::min);
    let bottom: Vec<String> = ks
        .iter()
        .filter(|k| k.rect.y == lowest)
        .map(|k| cap_text(&k.cap))
        .collect();
    assert_eq!(bottom, ["123", "space", "return"]);
    // The emoji key sits below the keys; there is no hide key.
    let emoji = ks
        .iter()
        .find(|k| cap_text(&k.cap) == "Emoji")
        .expect("emoji key");
    assert!(emoji.rect.y + emoji.rect.height <= lowest);
    assert!(!ks.iter().any(|k| matches!(k.action, KeyAction::Dismiss)));
    // iPhone proportions: 3 pt margins, keys on 6 pt gaps, 42 pt tall.
    let (q, w) = (char_key('q').rect, char_key('w').rect);
    assert!((q.x - 3.0).abs() < 0.01, "side margin {}", q.x);
    assert!(((w.x - q.x - q.width) - 6.0).abs() < 0.01, "key gap");
    assert!((q.width - 33.0).abs() < 0.01, "key width {}", q.width);
    assert_eq!(q.height, 42.0);
}

#[test]
fn the_iphone_return_key_names_what_enter_does() {
    for (action, word) in [
        (EnterAction::Newline, "return"),
        (EnterAction::Done, "done"),
        (EnterAction::Search, "search"),
        (EnterAction::Go, "go"),
        (EnterAction::Next, "next"),
        (EnterAction::Send, "send"),
    ] {
        let _phone = iphone(action, false);
        assert_eq!(cap_text(&key_of("Enter").cap), word, "{action:?}");
    }
}

#[test]
fn every_point_of_the_iphone_keyboard_belongs_to_a_key() {
    let _phone = iphone(EnterAction::Newline, false);
    let ks = keys();
    let h = panel_height();
    let area: f64 = ks.iter().map(|k| k.hit.width * k.hit.height).sum();
    assert!(
        (area - IPHONE.width * h).abs() < 1e-6,
        "hit areas cover {area}, the panel is {}",
        IPHONE.width * h
    );
    let mut y = 0.25;
    while y < h {
        let mut x = 0.25;
        while x < IPHONE.width {
            assert!(test_hook::key_at(Point::new(x, y)).is_some(), "({x}, {y})");
            x += 1.5;
        }
        y += 1.5;
    }
    // Between Shift and `z` (iOS's wider gap) the touch goes to the nearer.
    let (shift, z) = (key_of("Switch(Shifted)").rect, char_key('z').rect);
    let gap_left = shift.x + shift.width;
    let y = center(z).y;
    let owner = |x: f64| {
        format!(
            "{:?}",
            keys()[test_hook::key_at(Point::new(x, y)).unwrap()].action
        )
    };
    assert_eq!(owner(gap_left + 1.0), "Switch(Shifted)");
    assert_eq!(owner(z.x - 1.0), "Char('z')");
}

#[test]
fn the_iphone_keyboard_reports_the_height_it_paints_on_every_page() {
    let mut phone = iphone(EnterAction::Newline, false);
    let letters = panel_height();
    assert_eq!(osk::target_panel_height(IPHONE.width), letters);
    // 8 + 4 rows of 42 on 12 pt gaps + 4 + the 44 pt emoji row.
    assert_eq!(letters, 8.0 + 4.0 * 42.0 + 3.0 * 12.0 + 4.0 + 44.0);
    let p = center(key_of("Switch(Symbols)").rect);
    down(p);
    up(p);
    phone.paint();
    assert_eq!(panel_height(), letters, "the 123 page keeps the height");
    assert_eq!(osk::target_panel_height(IPHONE.width), letters);
}

#[test]
fn holding_e_on_the_iphone_offers_its_digit_and_accents() {
    let _clock = crate::clock::scoped_virtual(None);
    let mut phone = iphone(EnterAction::Newline, false);
    let e = center(char_key('e').rect);
    hold(&mut phone, e, LONG_PRESS_DELAY + Duration::from_millis(20));
    assert_eq!(popup(), Some(("3êéèëē".into(), Some(2))));
    let three = popup_cell(0);
    osk::handle_software_keyboard_mouse_move(three);
    assert!(matches!(up(three)[..], [Key::Char('3')]));
    // A top-row letter with no accents offers just its digit.
    let q = center(char_key('q').rect);
    hold(&mut phone, q, LONG_PRESS_DELAY + Duration::from_millis(20));
    assert_eq!(popup(), Some(("1".into(), Some(0))));
    assert!(matches!(up(q)[..], [Key::Char('1')]));
}
