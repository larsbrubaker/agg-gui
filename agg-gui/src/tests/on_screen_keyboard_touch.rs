//! Touch and look tests for the on-screen keyboard: every point of the panel
//! types a key (gaps, edges, the strip below the keys), a sliding finger
//! types the key it lifts on, letters show the case Shift will type, the
//! Enter key shows the field's action, the hide chevron sits below the keys,
//! and the keys draw on Gboard's proportions in light and dark.
//!
//! Companion to `tests/on_screen_keyboard.rs` (focus, layers, lift); these
//! drive the keyboard's own pointer handlers on a painted Android keyboard.

use std::sync::Arc;

use crate::geometry::{Point, Rect};
use crate::input_profile::{set_input_profile, InputProfile};
use crate::text::Font;
use crate::widgets::on_screen_keyboard::state::with_state_ref;
use crate::widgets::on_screen_keyboard::{
    self as osk, drain_synthetic_keys, key::PaintedKey, test_hook, EnterAction, KeyAction, KeyCap,
    KeyGlyph, KeyboardLayer,
};
use crate::widgets::{TextArea, TextField};
use crate::{App, Key, Modifiers, MouseButton, Size, Widget};

use super::TEST_FONT;

const PHONE: Size = Size {
    width: 412.0,
    height: 800.0,
};

/// Holds the input-profile lock for a test and puts the shared keyboard
/// state back when it ends.
pub(super) struct Phone {
    _lock: std::sync::MutexGuard<'static, ()>,
    pub(super) app: App,
    pub(super) size: Size,
}

impl Phone {
    /// A phone-sized app whose only widget is `root`, focused, with the
    /// Android keyboard up and painted once.
    pub(super) fn new(root: Box<dyn Widget>, dark: bool) -> Self {
        Self::with(root, dark, InputProfile::MobileAndroid, PHONE)
    }

    /// [`Phone::new`] on the `profile` keyboard in a `size` viewport.
    pub(super) fn with(
        root: Box<dyn Widget>,
        dark: bool,
        profile: InputProfile,
        size: Size,
    ) -> Self {
        let lock = crate::input_profile::profile_test_lock();
        osk::dismiss();
        test_hook::reset();
        osk::events::clear();
        crate::widget::keyboard_scroll::reset_lift_for_test();
        crate::ux_scale::set_ux_scale(1.0);
        crate::set_device_scale(1.0);
        set_input_profile(profile);
        crate::theme::set_visuals(if dark {
            crate::theme::Visuals::dark()
        } else {
            crate::theme::Visuals::light()
        });
        osk::set_enabled(true);
        let mut app = App::new(root);
        app.layout(size);
        app.on_key_down(Key::Tab, Modifiers::default());
        assert!(app.has_focus(), "the field takes focus");
        test_hook::force_visible();
        let mut phone = Phone {
            _lock: lock,
            app,
            size,
        };
        phone.paint();
        phone
    }

    pub(super) fn paint(&mut self) -> crate::Framebuffer {
        self.app.layout(self.size);
        let mut fb = crate::Framebuffer::new(self.size.width as u32, self.size.height as u32);
        {
            let mut ctx = crate::GfxCtx::new(&mut fb);
            self.app.paint(&mut ctx);
        }
        fb
    }
}

impl Drop for Phone {
    fn drop(&mut self) {
        osk::set_enabled(false);
        test_hook::reset();
        osk::events::clear();
        crate::font_settings::set_system_font(None);
        crate::theme::set_visuals(crate::theme::Visuals::light());
        set_input_profile(InputProfile::Desktop);
    }
}

pub(super) fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(TEST_FONT).unwrap())
}

pub(super) fn field() -> Box<dyn Widget> {
    Box::new(TextField::new(font()))
}

pub(super) fn keys() -> Vec<PaintedKey> {
    with_state_ref(|s| s.last_painted_keys.clone())
}

pub(super) fn panel_height() -> f64 {
    with_state_ref(|s| s.last_panel_height.expect("the keyboard painted"))
}

pub(super) fn key_of(action: &str) -> PaintedKey {
    keys()
        .into_iter()
        .find(|k| format!("{:?}", k.action) == action)
        .unwrap_or_else(|| panic!("the keyboard shows {action}"))
}

pub(super) fn char_key(c: char) -> PaintedKey {
    key_of(&format!("{:?}", KeyAction::Char(c)))
}

/// What a touch at `pos` (Y-up) hits, as the key's action text.
pub(super) fn owner(pos: Point) -> String {
    let i = test_hook::key_at(pos).unwrap_or_else(|| panic!("{pos:?} belongs to a key"));
    format!("{:?}", keys()[i].action)
}

/// Press at `down`, slide through `moves`, lift at `up` (all Y-up).
pub(super) fn touch(down: Point, moves: &[Point], up: Point) -> Vec<Key> {
    let m = Modifiers::default();
    assert!(osk::handle_software_keyboard_mouse_down(
        down,
        MouseButton::Left,
        m
    ));
    for p in moves {
        osk::handle_software_keyboard_mouse_move(*p);
    }
    osk::handle_software_keyboard_mouse_up(up, MouseButton::Left, m);
    drain_synthetic_keys().into_iter().map(|(k, _)| k).collect()
}

pub(super) fn center(r: Rect) -> Point {
    Point::new(r.x + r.width / 2.0, r.y + r.height / 2.0)
}

#[test]
fn every_point_of_the_keyboard_belongs_to_exactly_one_key() {
    let _phone = Phone::new(field(), false);
    let keys = keys();
    let h = panel_height();
    // The hit areas tile the panel: they cover its area exactly and never
    // overlap.
    let area: f64 = keys.iter().map(|k| k.hit.width * k.hit.height).sum();
    assert!(
        (area - PHONE.width * h).abs() < 1e-6,
        "hit areas cover {area}, the panel is {}",
        PHONE.width * h
    );
    for (i, a) in keys.iter().enumerate() {
        assert!(
            a.hit.x <= a.rect.x
                && a.hit.y <= a.rect.y
                && a.hit.x + a.hit.width >= a.rect.x + a.rect.width
                && a.hit.y + a.hit.height >= a.rect.y + a.rect.height,
            "{:?}: the hit area holds the drawn key",
            a.action
        );
        for b in &keys[i + 1..] {
            let w = (a.hit.x + a.hit.width).min(b.hit.x + b.hit.width) - a.hit.x.max(b.hit.x);
            let hh = (a.hit.y + a.hit.height).min(b.hit.y + b.hit.height) - a.hit.y.max(b.hit.y);
            assert!(
                w <= 1e-9 || hh <= 1e-9,
                "{:?} and {:?} overlap",
                a.action,
                b.action
            );
        }
    }
    // And a sweep finds no point that types nothing.
    let mut y = 0.25;
    while y < h {
        let mut x = 0.25;
        while x < PHONE.width {
            assert!(test_hook::key_at(Point::new(x, y)).is_some(), "({x}, {y})");
            x += 1.5;
        }
        y += 1.5;
    }
}

#[test]
fn a_touch_in_a_gap_or_at_an_edge_types_the_nearest_key() {
    let _phone = Phone::new(field(), false);
    let (e, r) = (char_key('e'), char_key('r'));
    let gap_left = e.rect.x + e.rect.width;
    let gap = r.rect.x - gap_left;
    assert!(gap > 2.0, "keys have a gap between them");
    let y = center(e.rect).y;
    assert_eq!(owner(Point::new(gap_left + gap * 0.25, y)), "Char('e')");
    assert_eq!(owner(Point::new(gap_left + gap * 0.75, y)), "Char('r')");
    // Between rows: just under 'e' is still 'e'; just over 'd' is 'd'.
    let d = char_key('d');
    let row_gap = e.rect.y - (d.rect.y + d.rect.height);
    assert!(row_gap > 2.0, "rows have a gap between them");
    let x = center(e.rect).x;
    assert_eq!(owner(Point::new(x, e.rect.y - row_gap * 0.25)), "Char('e')");
    let dx = center(d.rect).x;
    assert_eq!(
        owner(Point::new(dx, e.rect.y - row_gap * 0.75)),
        "Char('d')"
    );
    // Edges: the side margins, the home row's half-key insets and the top.
    let qy = center(char_key('q').rect).y;
    assert_eq!(owner(Point::new(0.5, qy)), "Char('q')");
    assert_eq!(owner(Point::new(PHONE.width - 0.5, qy)), "Char('p')");
    let ay = center(char_key('a').rect).y;
    assert_eq!(owner(Point::new(0.5, ay)), "Char('a')");
    assert_eq!(owner(Point::new(PHONE.width - 0.5, ay)), "Char('l')");
    let five = center(char_key('5').rect).x;
    assert_eq!(owner(Point::new(five, panel_height() - 0.5)), "Char('5')");
    // The strip below the keys: the chevron at the left, the bottom row
    // elsewhere (a low tap on the spacebar is still a space).
    assert_eq!(owner(Point::new(1.0, 1.0)), "Dismiss");
    let space = key_of("Space");
    assert_eq!(owner(Point::new(center(space.rect).x, 0.5)), "Space");
    // A tap in a gap types the key, through the real pointer handlers.
    let p = Point::new(gap_left + gap * 0.25, y);
    assert!(matches!(touch(p, &[], p)[..], [Key::Char('e')]));
}

#[test]
fn a_finger_sliding_across_the_keys_types_the_key_it_lifts_on() {
    let _phone = Phone::new(field(), false);
    let (q, w) = (center(char_key('q').rect), center(char_key('w').rect));
    let m = Modifiers::default();
    osk::handle_software_keyboard_mouse_down(q, MouseButton::Left, m);
    let pressed = |c: char| {
        let i = with_state_ref(|s| s.pressed_key_index).expect("a key is pressed");
        assert_eq!(format!("{:?}", keys()[i].action), format!("Char('{c}')"));
    };
    pressed('q');
    osk::handle_software_keyboard_mouse_move(w);
    pressed('w');
    osk::handle_software_keyboard_mouse_up(w, MouseButton::Left, m);
    let typed: Vec<Key> = drain_synthetic_keys().into_iter().map(|(k, _)| k).collect();
    assert!(matches!(typed[..], [Key::Char('w')]), "{typed:?}");
    // Sliding off the top of the keyboard and lifting types nothing.
    let above = Point::new(q.x, panel_height() + 40.0);
    assert!(touch(q, &[above], above).is_empty());
}

pub(super) fn letter_caps() -> String {
    keys()
        .iter()
        .filter_map(|k| match (&k.action, &k.cap) {
            (KeyAction::Char(c), KeyCap::Text(t)) if c.is_alphabetic() => Some(t.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn letters_show_the_case_shift_will_type() {
    let mut phone = Phone::new(field(), false);
    // A plain one-line field opens in lower case, even when empty.
    assert_eq!(test_hook::current_layer(), KeyboardLayer::Letters);
    assert_eq!(letter_caps(), "qwertyuiopasdfghjklzxcvbnm");
    let shift = center(key_of("Switch(Shifted)").rect);
    touch(shift, &[], shift);
    phone.paint();
    assert_eq!(letter_caps(), "QWERTYUIOPASDFGHJKLZXCVBNM");
    // One capital, then back to lower case.
    let a = center(char_key('A').rect);
    assert!(matches!(touch(a, &[], a)[..], [Key::Char('A')]));
    phone.paint();
    assert_eq!(letter_caps(), "qwertyuiopasdfghjklzxcvbnm");
    // A double tap locks capitals until Shift is tapped again.
    test_hook::simulate_shift_tap();
    test_hook::simulate_shift_tap();
    assert!(test_hook::caps_lock());
    phone.paint();
    for c in ['A', 'B'] {
        let k = center(char_key(c).rect);
        touch(k, &[], k);
        phone.paint();
    }
    assert_eq!(letter_caps(), "QWERTYUIOPASDFGHJKLZXCVBNM");
}

#[test]
fn a_field_that_holds_sentences_opens_with_a_capital() {
    let _phone = Phone::new(
        Box::new(TextField::new(font()).with_auto_capitalize(true)),
        false,
    );
    assert_eq!(test_hook::current_layer(), KeyboardLayer::Shifted);
    assert_eq!(letter_caps(), "QWERTYUIOPASDFGHJKLZXCVBNM");
}

fn enter_glyph() -> String {
    match key_of("Enter").cap {
        KeyCap::Glyph(g) => format!("{g:?}"),
        other => panic!("Enter shows an icon, not {other:?}"),
    }
}

#[test]
fn the_enter_key_shows_what_enter_does_in_the_field() {
    {
        let _phone = Phone::new(field(), false);
        assert_eq!(enter_glyph(), format!("{:?}", KeyGlyph::Done));
    }
    for (action, glyph) in [
        (EnterAction::Search, KeyGlyph::Search),
        (EnterAction::Next, KeyGlyph::Next),
        (EnterAction::Go, KeyGlyph::Go),
        (EnterAction::Send, KeyGlyph::Send),
        (EnterAction::Newline, KeyGlyph::Return),
    ] {
        let root = Box::new(TextField::new(font()).with_enter_action(action));
        let _phone = Phone::new(root, false);
        assert_eq!(enter_glyph(), format!("{glyph:?}"), "{action:?}");
    }
    let _phone = Phone::new(Box::new(TextArea::new(font())), false);
    assert_eq!(enter_glyph(), format!("{:?}", KeyGlyph::Return));
}

#[test]
fn the_hide_chevron_sits_below_the_keys_and_puts_the_keyboard_away() {
    let _phone = Phone::new(field(), false);
    let chevron = key_of("Dismiss");
    let lowest_key = keys()
        .iter()
        .filter(|k| !matches!(k.action, KeyAction::Dismiss))
        .map(|k| k.rect.y)
        .fold(f64::INFINITY, f64::min);
    assert!(
        chevron.rect.y + chevron.rect.height <= lowest_key,
        "the chevron is below the bottom row"
    );
    assert_eq!(
        keys().iter().filter(|k| k.rect.y == lowest_key).count(),
        5,
        "the bottom row is ?123, comma, space, period, Enter"
    );
    let p = center(chevron.rect);
    touch(p, &[], p);
    assert!(
        osk::take_dismiss_request(),
        "the chevron hides the keyboard"
    );
}

#[test]
fn the_keyboard_reports_the_height_it_paints() {
    let _phone = Phone::new(field(), false);
    assert_eq!(osk::target_panel_height(PHONE.width), panel_height());
}

/// The keys draw on Gboard's proportions: 15 dp side margins, keys about
/// 32.5 dp wide on 6 dp gaps and 45 dp tall, the home row offset by half a
/// key; in light and dark the key faces stand apart from the tray.
#[test]
fn keys_draw_with_gboard_proportions_in_light_and_dark() {
    for dark in [false, true] {
        let mut phone = Phone::new(field(), dark);
        let fb = phone.paint();
        let (q, w, a) = (char_key('q').rect, char_key('w').rect, char_key('a').rect);
        assert!((q.x - 15.0).abs() < 0.01, "side margin {}", q.x);
        assert!((q.width - 32.4).abs() < 0.5, "key width {}", q.width);
        assert!(((w.x - q.x - q.width) - 6.0).abs() < 0.01, "key gap");
        assert_eq!(q.height, 45.0);
        assert!(
            ((a.x - q.x) - (q.width + 6.0) / 2.0).abs() < 0.01,
            "half-key offset"
        );
        let px = |x: f64, y: f64| {
            let i = ((y as usize) * fb.width() as usize + x as usize) * 4;
            let p = fb.pixels();
            [p[i] as i32, p[i + 1] as i32, p[i + 2] as i32]
        };
        let face = px(q.x + 4.0, q.y + 4.0);
        let tray = px(q.x + q.width + 3.0, q.y + q.height / 2.0);
        let diff = (0..3).map(|i| (face[i] - tray[i]).abs()).sum::<i32>();
        assert!(diff >= 24, "dark={dark}: key {face:?} vs tray {tray:?}");
    }
}
