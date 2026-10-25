//! Long-press tests for [`super::TouchMouseEmu`], driven on the virtual UI
//! clock so every hold is timed exactly: a still finger becomes a right
//! click at its touch-down point after [`super::LONG_PRESS_DELAY`], and a
//! drag, a second finger or an early lift keep today's tap and drag rules.
//! A long press a widget holds on to ([`TouchMouseEmu::hold_long_press`])
//! keeps its right button down: the finger's moves reach the widget and
//! its lift releases the button.

use std::time::Duration;

use super::*;
use crate::touch_state::{TouchDeviceId, TouchId};

const DEV: TouchDeviceId = TouchDeviceId(0);
const F1: TouchId = TouchId(1);
const F2: TouchId = TouchId(2);

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

/// A fresh emulator at one device pixel per point on a virtual clock.
fn setup() -> (TouchMouseEmu, crate::clock::ClockGuard) {
    crate::set_device_scale(1.0);
    (TouchMouseEmu::new(), crate::clock::scoped_virtual(None))
}

fn right_click_at(x: f64, y: f64) -> Vec<EmuCmd> {
    vec![
        EmuCmd::MouseDown(x, y, MouseButton::Right),
        EmuCmd::MouseUp(x, y, MouseButton::Right),
    ]
}

fn has_press(cmds: &[EmuCmd], button: MouseButton) -> bool {
    cmds.iter()
        .any(|c| matches!(c, EmuCmd::MouseDown(_, _, b) if *b == button))
}

#[test]
fn holding_still_for_the_delay_right_clicks_at_the_touch_down_point() {
    let (mut emu, _clock) = setup();
    emu.on_start(DEV, F1, 100.0, 200.0);
    crate::clock::advance(ms(499));
    assert_eq!(emu.poll(), Vec::<EmuCmd>::new(), "not due before 500 ms");
    crate::clock::advance(ms(1));
    assert_eq!(emu.poll(), right_click_at(100.0, 200.0));
    assert_eq!(emu.take_long_press(), Some((100.0, 200.0)));
    assert_eq!(emu.poll(), Vec::<EmuCmd>::new(), "fires once");
}

#[test]
fn moving_past_the_slop_before_the_delay_is_a_drag_not_a_right_click() {
    let (mut emu, _clock) = setup();
    let mut all = emu.on_start(DEV, F1, 100.0, 100.0);
    crate::clock::advance(ms(200));
    all.extend(emu.on_move(DEV, F1, 111.0, 100.0));
    assert!(
        has_press(&all, MouseButton::Middle),
        "a drag began: {all:?}"
    );
    crate::clock::advance(ms(400));
    all.extend(emu.poll());
    all.extend(emu.on_end(DEV, F1));
    assert!(
        !has_press(&all, MouseButton::Right),
        "no right click: {all:?}"
    );
    assert_eq!(emu.take_long_press(), None);
}

#[test]
fn drifting_within_the_slop_still_long_presses() {
    let (mut emu, _clock) = setup();
    emu.on_start(DEV, F1, 100.0, 100.0);
    crate::clock::advance(ms(200));
    assert_eq!(
        emu.on_move(DEV, F1, 109.0, 100.0),
        vec![EmuCmd::MouseMove(109.0, 100.0)]
    );
    crate::clock::advance(ms(300));
    assert_eq!(emu.poll(), right_click_at(100.0, 100.0));
}

#[test]
fn a_second_finger_before_the_delay_cancels_the_long_press() {
    let (mut emu, _clock) = setup();
    let mut all = emu.on_start(DEV, F1, 100.0, 100.0);
    crate::clock::advance(ms(300));
    all.extend(emu.on_start(DEV, F2, 200.0, 100.0));
    crate::clock::advance(ms(400));
    all.extend(emu.poll());
    all.extend(emu.on_end(DEV, F2));
    all.extend(emu.on_end(DEV, F1));
    assert!(
        !has_press(&all, MouseButton::Right),
        "no right click: {all:?}"
    );
    assert!(
        !has_press(&all, MouseButton::Left),
        "no left click: {all:?}"
    );
}

#[test]
fn a_tap_shorter_than_the_delay_is_a_left_click() {
    let (mut emu, _clock) = setup();
    emu.on_start(DEV, F1, 40.0, 50.0);
    crate::clock::advance(ms(300));
    assert_eq!(emu.poll(), Vec::<EmuCmd>::new());
    assert_eq!(
        emu.on_end(DEV, F1),
        vec![
            EmuCmd::MouseDown(40.0, 50.0, MouseButton::Left),
            EmuCmd::MouseUp(40.0, 50.0, MouseButton::Left),
            EmuCmd::MouseLeave,
        ]
    );
}

#[test]
fn after_a_long_press_moving_and_lifting_emit_no_click_and_no_drag() {
    let (mut emu, _clock) = setup();
    emu.on_start(DEV, F1, 100.0, 100.0);
    crate::clock::advance(ms(500));
    assert_eq!(emu.poll(), right_click_at(100.0, 100.0));
    assert_eq!(emu.on_move(DEV, F1, 160.0, 100.0), Vec::<EmuCmd>::new());
    assert_eq!(emu.on_end(DEV, F1), vec![EmuCmd::MouseLeave]);
    // The next finger is a fresh gesture: a quick tap clicks again.
    emu.on_start(DEV, F2, 10.0, 10.0);
    assert!(has_press(&emu.on_end(DEV, F2), MouseButton::Left));
}

#[test]
fn a_hold_that_came_due_before_a_late_event_fires_instead_of_a_tap_or_drag() {
    // No frame polled between the deadline and the lift: still a long press.
    let (mut emu, _clock) = setup();
    emu.on_start(DEV, F1, 100.0, 100.0);
    crate::clock::advance(ms(700));
    assert_eq!(
        emu.on_end(DEV, F1),
        [right_click_at(100.0, 100.0), vec![EmuCmd::MouseLeave]].concat()
    );
    // Nor does a move past the slop that arrives late start a drag.
    emu.on_start(DEV, F1, 100.0, 100.0);
    crate::clock::advance(ms(600));
    assert_eq!(
        emu.on_move(DEV, F1, 150.0, 100.0),
        right_click_at(100.0, 100.0)
    );
}

#[test]
fn the_cue_appears_after_its_delay_and_fills_to_the_deadline() {
    let (mut emu, _clock) = setup();
    assert_eq!(emu.long_press_deadline(), None);
    let down = crate::clock::now();
    emu.on_start(DEV, F1, 30.0, 40.0);
    assert_eq!(emu.long_press_deadline(), Some(down + LONG_PRESS_DELAY));
    crate::clock::advance(ms(149));
    assert_eq!(emu.long_press_progress(), None, "no cue for a quick tap");
    crate::clock::advance(ms(1));
    assert_eq!(emu.long_press_progress(), Some(((30.0, 40.0), 0.0)));
    crate::clock::advance(ms(175));
    let (_, half) = emu.long_press_progress().expect("cue");
    assert!((half - 0.5).abs() < 1e-9, "half way: {half}");
    emu.on_end(DEV, F1);
    assert_eq!(emu.long_press_deadline(), None);
    assert_eq!(emu.long_press_progress(), None);
}

#[test]
fn a_held_long_press_moves_the_pointer_and_releases_the_right_button_on_lift() {
    let (mut emu, _clock) = setup();
    emu.on_start(DEV, F1, 100.0, 100.0);
    assert!(!emu.hold_long_press(), "nothing to hold before it fires");
    crate::clock::advance(ms(500));
    assert_eq!(emu.poll(), right_click_at(100.0, 100.0));
    assert!(emu.hold_long_press());
    assert_eq!(
        emu.on_move(DEV, F1, 160.0, 104.0),
        vec![EmuCmd::MouseMove(160.0, 104.0)]
    );
    assert_eq!(
        emu.on_end(DEV, F1),
        vec![
            EmuCmd::MouseUp(160.0, 104.0, MouseButton::Right),
            EmuCmd::MouseLeave,
        ]
    );
    // The next finger is a fresh gesture that holds nothing.
    emu.on_start(DEV, F2, 10.0, 10.0);
    assert_eq!(
        emu.on_move(DEV, F2, 12.0, 10.0),
        vec![EmuCmd::MouseMove(12.0, 10.0)]
    );
    assert!(has_press(&emu.on_end(DEV, F2), MouseButton::Left));
}

#[test]
fn a_held_long_press_is_cancelled_by_a_second_finger_or_a_platform_cancel() {
    let (mut emu, _clock) = setup();
    emu.on_start(DEV, F1, 100.0, 100.0);
    crate::clock::advance(ms(500));
    emu.poll();
    assert!(emu.hold_long_press());
    assert_eq!(
        emu.on_start(DEV, F2, 200.0, 100.0),
        vec![EmuCmd::CancelPress]
    );
    assert_eq!(emu.on_move(DEV, F1, 150.0, 100.0), Vec::<EmuCmd>::new());
    assert_eq!(emu.on_end(DEV, F1), vec![EmuCmd::MouseLeave]);
    emu.on_end(DEV, F2);

    emu.on_start(DEV, F1, 100.0, 100.0);
    crate::clock::advance(ms(500));
    emu.poll();
    assert!(emu.hold_long_press());
    assert_eq!(
        emu.on_cancel(DEV, F1),
        vec![EmuCmd::CancelPress, EmuCmd::MouseLeave]
    );
}

#[test]
fn a_long_press_that_fires_on_the_lift_cannot_be_held() {
    let (mut emu, _clock) = setup();
    emu.on_start(DEV, F1, 100.0, 100.0);
    crate::clock::advance(ms(700));
    let cmds = emu.on_end(DEV, F1);
    assert_eq!(
        cmds,
        [right_click_at(100.0, 100.0), vec![EmuCmd::MouseLeave]].concat()
    );
    assert!(!emu.hold_long_press(), "the finger has already lifted");
}
