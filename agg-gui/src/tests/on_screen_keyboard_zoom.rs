//! The on-screen keyboard under the app's UX zoom and through the touch
//! entry points a phone uses.
//!
//! The keyboard keeps the phone keyboard's own size (Gboard's 45 dp keys)
//! whatever zoom the app's widgets use, and every letter draws at one size.
//! A finger on the keyboard presses its key the moment it lands, so the
//! pressed key and its preview show while the finger is down (and for a
//! moment after a quick tap), and holding opens the alternates, all driven
//! through `App::on_touch_*` the way the touch shells call it.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use super::on_screen_keyboard_touch::{center, char_key, field, font, keys, Phone};
use crate::geometry::Point;
use crate::input_profile::InputProfile;
use crate::touch_state::{TouchDeviceId, TouchId};
use crate::widgets::on_screen_keyboard::alternates::LONG_PRESS_DELAY;
use crate::widgets::on_screen_keyboard::state::with_state_ref;
use crate::widgets::on_screen_keyboard::{test_hook, KeyAction};
use crate::widgets::TextField;
use crate::Size;

/// A Pixel's viewport in CSS pixels.
const PIXEL: Size = Size {
    width: 412.0,
    height: 915.0,
};

#[test]
fn keys_keep_the_phone_keyboards_size_under_the_ux_zoom() {
    let mut phone = Phone::with(field(), false, InputProfile::MobileAndroid, PIXEL);
    crate::font_settings::set_system_font(Some(super::on_screen_keyboard_touch::font()));
    for zoom in [1.0, 1.7] {
        crate::ux_scale::set_ux_scale(zoom);
        phone.paint();
        let q = char_key('q').rect;
        assert!(
            (q.height * zoom - 45.0).abs() < 0.01,
            "zoom {zoom}: keys are {} CSS px tall",
            q.height * zoom
        );
        assert!(
            (q.x * zoom - 15.0).abs() < 0.01,
            "zoom {zoom}: side margin {}",
            q.x * zoom
        );
        let sizes: Vec<f64> = keys()
            .iter()
            .filter(|k| matches!(k.action, KeyAction::Char(c) if c.is_alphabetic()))
            .filter_map(test_hook::cap_font_size)
            .collect();
        assert_eq!(sizes.len(), 26);
        assert!(
            sizes.iter().all(|s| *s == sizes[0]),
            "zoom {zoom}: every letter draws at one size: {sizes:?}"
        );
        assert!((sizes[0] * zoom - 24.0).abs() < 0.01);
    }
    crate::ux_scale::set_ux_scale(1.0);
}

const DEV: TouchDeviceId = TouchDeviceId(0);
const FINGER: TouchId = TouchId(1);

/// Y-up `p` as the Y-down screen point a touch arrives at.
fn screen(phone: &Phone, p: Point) -> (f64, f64) {
    (p.x, phone.size.height - p.y)
}

fn pressed() -> Option<String> {
    with_state_ref(|s| s.pressed_key_index).map(|i| format!("{:?}", keys()[i].action))
}

/// The colour a framebuffer shows at Y-up `(x, y)`.
fn pixel(fb: &crate::Framebuffer, x: f64, y: f64) -> [u8; 3] {
    let i = ((y as usize) * fb.width() as usize + x as usize) * 4;
    let p = fb.pixels();
    [p[i], p[i + 1], p[i + 2]]
}

#[test]
fn a_finger_on_a_key_shows_it_pressed_with_its_preview_while_it_is_down() {
    let _clock = crate::clock::scoped_virtual(None);
    let text = Rc::new(RefCell::new(String::new()));
    let root = Box::new(TextField::new(font()).with_text_cell(text.clone()));
    let mut phone = Phone::new(root, false);
    let h = char_key('h').rect;
    // Just above the key, in the gap between rows: the tray, until the
    // preview bubble covers it.
    let probe = (center(h).x, h.y + h.height + 3.0);
    let before = pixel(&phone.paint(), probe.0, probe.1);
    let (x, y) = screen(&phone, center(h));
    phone.app.on_touch_start(DEV, FINGER, x, y, None);
    assert_eq!(
        pressed().as_deref(),
        Some("Char('h')"),
        "pressed on landing"
    );
    let held = pixel(&phone.paint(), probe.0, probe.1);
    assert_ne!(held, before, "the preview shows while the finger is down");
    crate::clock::advance(Duration::from_millis(40));
    phone.app.on_touch_end(DEV, FINGER);
    assert_eq!(*text.borrow(), "h", "the lift types the key once");
    // A quick tap still flashes the preview a moment after the lift.
    let after_lift = pixel(&phone.paint(), probe.0, probe.1);
    assert_eq!(after_lift, held, "the preview lingers after a quick tap");
    crate::clock::advance(Duration::from_millis(150));
    let later = pixel(&phone.paint(), probe.0, probe.1);
    assert_eq!(later, before, "then it goes");
}

#[test]
fn holding_a_finger_on_a_key_opens_its_alternates_and_sliding_picks_one() {
    let _clock = crate::clock::scoped_virtual(None);
    let text = Rc::new(RefCell::new(String::new()));
    let root = Box::new(TextField::new(font()).with_text_cell(text.clone()));
    let mut phone = Phone::new(root, false);
    let e = center(char_key('e').rect);
    let (x, y) = screen(&phone, e);
    phone.app.on_touch_start(DEV, FINGER, x, y, None);
    for _ in 0..10 {
        crate::clock::advance(Duration::from_millis(50));
        phone.app.layout(phone.size);
        phone.paint();
    }
    assert!(LONG_PRESS_DELAY < Duration::from_millis(500));
    let cell = with_state_ref(|s| {
        let p = s.alternates.as_ref().expect("the alternates opened");
        center(p.cells[3])
    });
    let (cx, cy) = screen(&phone, cell);
    phone.app.on_touch_move(DEV, FINGER, cx, cy, None);
    phone.app.on_touch_end(DEV, FINGER);
    assert_eq!(*text.borrow(), "ë");
    // The app's own long press (a right click) never fired on the keyboard.
    assert!(pressed().is_none());
}

#[test]
fn a_finger_sliding_across_the_keys_types_the_key_it_lifts_on() {
    let text = Rc::new(RefCell::new(String::new()));
    let root = Box::new(TextField::new(font()).with_text_cell(text.clone()));
    let mut phone = Phone::new(root, false);
    let (q, w) = (center(char_key('q').rect), center(char_key('w').rect));
    let (qx, qy) = screen(&phone, q);
    let (wx, wy) = screen(&phone, w);
    phone.app.on_touch_start(DEV, FINGER, qx, qy, None);
    phone.app.on_touch_move(DEV, FINGER, wx, wy, None);
    assert_eq!(pressed().as_deref(), Some("Char('w')"));
    phone.app.on_touch_end(DEV, FINGER);
    assert_eq!(*text.borrow(), "w");
    // A cancelled touch types nothing.
    phone.app.on_touch_start(DEV, FINGER, qx, qy, None);
    phone.app.on_touch_cancel(DEV, FINGER);
    assert_eq!(*text.borrow(), "w");
    assert!(pressed().is_none());
}
