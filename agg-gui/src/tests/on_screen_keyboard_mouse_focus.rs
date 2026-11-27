//! The on-screen keyboard rises for a finger, not for a mouse: on a page that
//! reported a phone (so the keyboard is enabled), a text field focused by a
//! mouse click gets no keyboard and no lift, while a tap on it raises the
//! keyboard; a mouse click that moves focus to another field puts a raised
//! keyboard away; a field a tapped button focuses still gets it, as the
//! latest press was the finger's; a tap on a field that already has focus
//! with no keyboard up (the mouse focused it) raises it; a pen's press (the
//! shell's `ForwarderEvent::PenDown`) counts as a finger's for this; and with
//! `set_rises_for_mouse` on, a mouse click raises it too
//! (`widget/keyboard_scroll.rs` `notify_focus_change`,
//! `touch_emulation::last_press_raises_on_screen_keyboard`).
//!
//! Drives the `App`'s own mouse and touch entry points, so the press noting,
//! the touch emulation and the focus change are the production ones.

use std::sync::MutexGuard;

use super::touch_pan_fixtures::{reset, screen, Board, Finger};
use crate::geometry::{Rect, Size};
use crate::input_profile::{set_input_profile, InputProfile};
use crate::shell_input::{ClickCount, ForwarderEvent, InputForwarder};
use crate::widgets::on_screen_keyboard::{self as osk, test_hook};
use crate::widgets::{Button, TextField};
use crate::{App, Framebuffer, GfxCtx, Modifiers, MouseButton, Widget};

const PHONE: Size = Size {
    width: 412.0,
    height: 800.0,
};
const FINGER: Finger = Finger {
    height: PHONE.height,
};
/// Two fields low on the screen, where a raised keyboard would cover them.
const LOW: Rect = Rect {
    x: 20.0,
    y: 40.0,
    width: 300.0,
    height: 40.0,
};
const LOWER_RIGHT: Rect = Rect {
    x: 20.0,
    y: 100.0,
    width: 300.0,
    height: 40.0,
};
const BUTTON: Rect = Rect {
    x: 20.0,
    y: 600.0,
    width: 200.0,
    height: 40.0,
};
const FIELD_ID: crate::focus::FocusId = 4242;

/// The profile lock, an Android phone profile with the keyboard enabled, and
/// everything put back on drop.
struct KeyboardOn {
    _lock: MutexGuard<'static, ()>,
}

impl KeyboardOn {
    fn new() -> Self {
        let lock = crate::input_profile::profile_test_lock();
        reset(1.0);
        osk::dismiss();
        test_hook::reset();
        osk::events::clear();
        crate::widget::keyboard_scroll::reset_lift_for_test();
        crate::touch_emulation::long_press::set_last_press_was_touch_for_testing(false);
        set_input_profile(InputProfile::MobileAndroid);
        osk::set_enabled(true);
        KeyboardOn { _lock: lock }
    }
}

impl Drop for KeyboardOn {
    fn drop(&mut self) {
        osk::set_enabled(false);
        test_hook::reset();
        osk::events::clear();
        crate::widget::keyboard_scroll::reset_lift_for_test();
        crate::touch_emulation::long_press::set_last_press_was_touch_for_testing(false);
        set_input_profile(InputProfile::Desktop);
    }
}

fn field(id: crate::focus::FocusId) -> Box<dyn Widget> {
    Box::new(TextField::new(super::touch_text_selection::font()).with_focus_id(id))
}

fn frame(app: &mut App) {
    app.layout(PHONE);
    let mut fb = Framebuffer::new(PHONE.width as u32, PHONE.height as u32);
    let mut ctx = GfxCtx::new(&mut fb);
    app.paint(&mut ctx);
}

/// A left click with the mouse at the middle of `rect` (Y-up).
fn click(app: &mut App, rect: Rect) {
    let (x, y) = screen(
        PHONE.height,
        rect.x + rect.width / 2.0,
        rect.y + rect.height / 2.0,
    );
    app.on_mouse_down(x, y, MouseButton::Left, Modifiers::default());
    app.on_mouse_up(x, y, MouseButton::Left, Modifiers::default());
    frame(app);
}

/// A finger's tap at the middle of `rect` (Y-up).
fn tap(app: &mut App, rect: Rect) {
    FINGER.down(app, rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
    FINGER.up(app);
    frame(app);
}

fn two_fields() -> App {
    let board = Board::new(PHONE.width, PHONE.height)
        .place(LOW, field(1))
        .place(LOWER_RIGHT, field(2));
    let mut app = App::new(Box::new(board));
    frame(&mut app);
    app
}

/// Whether the field at `index` of the board holds focus.
fn focused(app: &App, index: usize) -> bool {
    app.focused_widget_type_name() == Some("TextField") && app.focused_path() == Some(&[index][..])
}

#[test]
fn a_mouse_click_on_a_field_focuses_it_without_raising_the_keyboard_or_lifting() {
    let _on = KeyboardOn::new();
    let mut app = two_fields();
    click(&mut app, LOW);
    assert!(focused(&app, 0), "the click focused the field");
    assert!(!osk::is_raised(), "no keyboard for a mouse click");
    for _ in 0..10 {
        frame(&mut app);
    }
    assert!(!osk::is_visible(), "the keyboard never slides up");
    assert_eq!(
        crate::widget::keyboard_scroll::lift_target_for_test(),
        0.0,
        "nothing lifts the field clear of a keyboard that is not there"
    );
}

#[test]
fn a_tap_on_a_field_raises_the_keyboard() {
    let _on = KeyboardOn::new();
    let mut app = two_fields();
    tap(&mut app, LOW);
    assert!(focused(&app, 0), "the tap focused the field");
    assert!(osk::is_raised(), "a finger's tap raises the keyboard");
    assert!(
        crate::widget::keyboard_scroll::lift_target_for_test() > 0.0,
        "the low field is lifted clear of the keyboard"
    );
}

#[test]
fn a_mouse_click_moving_focus_to_another_field_puts_the_keyboard_away() {
    let _on = KeyboardOn::new();
    let mut app = two_fields();
    tap(&mut app, LOW);
    assert!(osk::is_raised(), "the tap raised the keyboard");
    // Reset the lift so the second field is where the click lands.
    crate::widget::keyboard_scroll::reset_lift_for_test();
    frame(&mut app);
    click(&mut app, LOWER_RIGHT);
    assert!(focused(&app, 1), "the click moved focus to the other field");
    assert!(
        !osk::is_raised(),
        "a mouse user types on a physical keyboard"
    );
}

#[test]
fn a_field_focused_by_a_tapped_button_raises_the_keyboard_and_a_clicked_one_does_not() {
    let _on = KeyboardOn::new();
    let button = Button::new("Rename", super::touch_text_selection::font())
        .on_click(|| crate::focus::request_focus(FIELD_ID));
    let board = Board::new(PHONE.width, PHONE.height)
        .place(LOW, field(FIELD_ID))
        .place(BUTTON, Box::new(button));
    let mut app = App::new(Box::new(board));
    frame(&mut app);

    click(&mut app, BUTTON);
    frame(&mut app);
    assert!(focused(&app, 0), "the clicked button focused the field");
    assert!(!osk::is_raised(), "no keyboard after a mouse click");

    // The same with a finger.
    tap(&mut app, BUTTON);
    frame(&mut app);
    assert!(focused(&app, 0), "the tapped button focused the field");
    assert!(osk::is_raised(), "the latest press was a finger's");
}

#[test]
fn a_tap_on_a_field_the_mouse_focused_raises_the_keyboard() {
    let _on = KeyboardOn::new();
    let mut app = two_fields();
    click(&mut app, LOW);
    assert!(
        focused(&app, 0) && !osk::is_raised(),
        "focused, no keyboard"
    );
    tap(&mut app, LOW);
    assert!(focused(&app, 0), "the field keeps focus");
    assert!(
        osk::is_raised(),
        "the finger on the field raises the keyboard"
    );
    assert!(
        crate::widget::keyboard_scroll::lift_target_for_test() > 0.0,
        "the low field is lifted clear of the keyboard"
    );
}

/// A stylus's press and release at the middle of `rect` (Y-up), as the web
/// shell forwards a `pointerType` of `"pen"`.
fn pen_tap(app: &mut App, forwarder: &mut InputForwarder, rect: Rect) {
    let (x, y) = screen(
        PHONE.height,
        rect.x + rect.width / 2.0,
        rect.y + rect.height / 2.0,
    );
    let (at, modifiers) = (Some((x, y)), Some(Modifiers::default()));
    forwarder.platform(
        app,
        ForwarderEvent::PenDown {
            at,
            button: MouseButton::Left,
            modifiers,
            clicks: ClickCount::Auto,
        },
    );
    forwarder.platform(
        app,
        ForwarderEvent::MouseUp {
            at,
            button: MouseButton::Left,
            modifiers,
        },
    );
    frame(app);
}

#[test]
fn a_pen_tap_on_a_field_raises_the_keyboard() {
    let _on = KeyboardOn::new();
    let mut app = two_fields();
    let mut forwarder = InputForwarder::new();
    pen_tap(&mut app, &mut forwarder, LOW);
    assert!(focused(&app, 0), "the pen focused the field");
    assert!(
        crate::touch_emulation::last_press_was_pen()
            && !crate::touch_emulation::last_press_was_touch(),
        "a pen's press, not a finger's"
    );
    assert!(osk::is_raised(), "a pen on the screen raises the keyboard");
}

#[test]
fn a_pen_tap_on_a_field_the_mouse_focused_raises_the_keyboard() {
    let _on = KeyboardOn::new();
    let mut app = two_fields();
    click(&mut app, LOW);
    assert!(
        focused(&app, 0) && !osk::is_raised(),
        "focused, no keyboard"
    );
    let mut forwarder = InputForwarder::new();
    pen_tap(&mut app, &mut forwarder, LOW);
    assert!(osk::is_raised(), "the pen on the field raises the keyboard");
}

#[test]
fn with_the_keyboard_rising_for_the_mouse_a_click_raises_it() {
    let _on = KeyboardOn::new();
    osk::set_rises_for_mouse(true);
    let mut app = two_fields();
    click(&mut app, LOW);
    let raised = osk::is_raised();
    osk::set_rises_for_mouse(false);
    assert!(raised, "a bench driven with a mouse gets the keyboard");
}
