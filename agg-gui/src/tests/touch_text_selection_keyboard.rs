//! Touch selection with the on-screen keyboard up: a long press on an
//! unfocused `TextArea` low on the screen focuses it, the keyboard opens and
//! lifts the layout under the still-held finger, and the word drag does not
//! jump lines because the content moved; and a selection made by touch is
//! replaced by dictated text.

use std::sync::MutexGuard;

use super::on_screen_keyboard_voice::{bound_field, FakeVoice, Rig};
use super::touch_pan_fixtures::{reset, Board, Finger};
use crate::geometry::{Point, Rect, Size};
use crate::input_profile::{set_input_profile, InputProfile};
use crate::touch_emulation::LONG_PRESS_DELAY;
use crate::widgets::on_screen_keyboard::{self as osk, test_hook, VoiceState};
use crate::widgets::touch_selection::TouchSelectable;
use crate::{App, Framebuffer, GfxCtx, TextArea};

const PHONE: Size = Size {
    width: 412.0,
    height: 800.0,
};
const FINGER: Finger = Finger {
    height: PHONE.height,
};

/// The profile lock, the keyboard on, and everything put back on drop.
struct KeyboardOn {
    _lock: MutexGuard<'static, ()>,
    _clock: crate::clock::ClockGuard,
}

impl KeyboardOn {
    fn new() -> Self {
        let lock = crate::input_profile::profile_test_lock();
        reset(1.0);
        osk::dismiss();
        test_hook::reset();
        osk::events::clear();
        crate::widget::keyboard_scroll::reset_lift_for_test();
        set_input_profile(InputProfile::MobileAndroid);
        osk::set_enabled(true);
        KeyboardOn {
            _lock: lock,
            _clock: crate::clock::scoped_virtual(None),
        }
    }
}

impl Drop for KeyboardOn {
    fn drop(&mut self) {
        osk::set_enabled(false);
        test_hook::reset();
        osk::events::clear();
        crate::widget::keyboard_scroll::reset_lift_for_test();
        set_input_profile(InputProfile::Desktop);
    }
}

fn frame(app: &mut App) {
    app.layout(PHONE);
    let mut fb = Framebuffer::new(PHONE.width as u32, PHONE.height as u32);
    let mut ctx = GfxCtx::new(&mut fb);
    app.paint(&mut ctx);
}

fn area(app: &mut App) -> &mut TextArea {
    app.root_mut().children_mut()[0]
        .as_any_mut()
        .and_then(|a| a.downcast_mut::<TextArea>())
        .expect("the area")
}

#[test]
fn the_keyboard_lifting_the_layout_under_a_held_word_drag_does_not_move_the_selection() {
    let _on = KeyboardOn::new();
    let text = "alpha beta gamma\ndelta epsilon zeta\neta theta iota\nkappa lambda mu";
    let rect = Rect::new(20.0, 40.0, 360.0, 160.0);
    let field = TextArea::new(super::touch_text_selection::font())
        .with_text(text)
        .with_font_size(16.0);
    let mut app = App::new(Box::new(
        Board::new(PHONE.width, PHONE.height).place(rect, Box::new(field)),
    ));
    frame(&mut app);
    let word = area(&mut app).ts_caret_line(7).middle(); // "beta"
    let at = Point::new(rect.x + word.x, rect.y + word.y);
    FINGER.down(&mut app, at.x, at.y);
    crate::clock::advance(LONG_PRESS_DELAY);
    frame(&mut app);
    assert!(app.has_focus(), "the long press focused the area");
    let (a, c) = area(&mut app).ts_anchor_cursor();
    assert_eq!((a.min(c), a.max(c)), (6, 10));
    // The keyboard opens and the layout slides up under the held finger.
    test_hook::force_visible();
    for _ in 0..40 {
        crate::clock::advance(std::time::Duration::from_millis(25));
        frame(&mut app);
    }
    let lift = crate::widget::keyboard_scroll::current_lift();
    assert!(lift > 20.0, "the keyboard lifted the layout: {lift}");
    // The finger has not moved over the text it pressed: a small slide on
    // the same word keeps the selection.
    FINGER.move_to(&mut app, at.x + 2.0, at.y);
    let (a, c) = area(&mut app).ts_anchor_cursor();
    assert_eq!(
        (a.min(c), a.max(c)),
        (6, 10),
        "no jump to the line the lift brought under it"
    );
    FINGER.up(&mut app);
}

#[test]
fn dictation_replaces_a_selection_made_by_touch() {
    let voice = FakeVoice::new(VoiceState::Ready);
    let (field, cell) = bound_field("hello brave new world");
    let mut rig = Rig::new(field, InputProfile::MobileAndroid, Some(&voice));
    let _clock = crate::clock::scoped_virtual(None);
    let f = rig.field();
    let (here, next) = (f.ts_caret_line(8), f.ts_caret_line(9));
    let at = Point::new((here.x + next.x) * 0.5, here.middle().y);
    FINGER.down(&mut rig.app, at.x, at.y);
    crate::clock::advance(LONG_PRESS_DELAY);
    rig.paint();
    FINGER.up(&mut rig.app);
    rig.paint();
    assert_eq!(rig.field().selection(), "brave");
    rig.start_dictation();
    voice.say("bold", "");
    rig.paint();
    assert_eq!(*cell.borrow(), "hello bold new world");
}
