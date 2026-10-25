//! Touch text selection in a real [`TextField`], driven through `App`'s
//! touch entry points on a 412 x 800 phone at the Android (and iOS) input
//! profile, with the long press timed on the virtual UI clock: a long press
//! selects the word under the finger, sliding on extends by words, the
//! handles drag by character and never cross, the floating toolbar shows,
//! hides and runs its buttons, a tap clears the selection and leaves
//! Android's caret handle, which drags the caret and fades. A mouse keeps
//! the desktop behaviour. The `TextArea` cases are in
//! `touch_text_area_selection.rs`, sharing [`Phone`] from here.

use std::sync::{Arc, MutexGuard};

use super::touch_pan_fixtures::{reset, Board, Finger};
use crate::geometry::{Point, Rect, Size};
use crate::input_profile::{set_input_profile, InputProfile};
use crate::text::Font;
use crate::touch_emulation::LONG_PRESS_DELAY;
use crate::widgets::text_context_menu::TextMenuAction;
use crate::widgets::touch_selection::{probe, HandleKind, TouchSelectable, CARET_HANDLE_TIME};
use crate::{App, Framebuffer, GfxCtx, Modifiers, MouseButton, TextField, Widget};

pub(super) const W: f64 = 412.0;
pub(super) const H: f64 = 800.0;
pub(super) const FINGER: Finger = Finger { height: H };
pub(super) const TEXT: &str = "hello brave new world";
/// Where the field sits (Y-up window coordinates).
pub(super) const FIELD: Rect = Rect {
    x: 20.0,
    y: 700.0,
    width: 330.0,
    height: 40.0,
};

pub(super) fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(super::TEST_FONT).expect("font"))
}

/// A phone-sized app at `profile` holding one widget at `rect`, on the
/// virtual clock, with this thread's own clipboard holding "clip"; puts the
/// profile back when dropped.
pub(super) struct Phone {
    _lock: MutexGuard<'static, ()>,
    _clipboard: crate::clipboard::SimulatedClipboard,
    pub(super) app: App,
    pub(super) rect: Rect,
    _clock: crate::clock::ClockGuard,
}

impl Phone {
    pub(super) fn new(profile: InputProfile, rect: Rect, widget: Box<dyn Widget>) -> Self {
        Self::with_root(
            profile,
            rect,
            Box::new(Board::new(W, H).place(rect, widget)),
        )
    }

    /// [`Phone::new`] with the whole tree given: `root`'s first child is the
    /// widget under test, at `rect`.
    pub(super) fn with_root(profile: InputProfile, rect: Rect, root: Box<dyn Widget>) -> Self {
        let lock = crate::input_profile::profile_test_lock();
        reset(1.0);
        set_input_profile(profile);
        crate::widgets::on_screen_keyboard::set_enabled(false);
        let clock = crate::clock::scoped_virtual(None);
        let clipboard = crate::clipboard::simulate();
        crate::clipboard::set_text("clip");
        let mut app = App::new(root);
        app.layout(Size::new(W, H));
        let mut phone = Phone {
            _lock: lock,
            _clipboard: clipboard,
            app,
            rect,
            _clock: clock,
        };
        phone.frame();
        phone
    }

    pub(super) fn frame(&mut self) {
        self.app.layout(Size::new(W, H));
        let mut fb = Framebuffer::new(W as u32, H as u32);
        let mut ctx = GfxCtx::new(&mut fb);
        self.app.paint(&mut ctx);
    }

    pub(super) fn widget<T: 'static>(&mut self) -> &mut T {
        self.app.root_mut().children_mut()[0]
            .as_any_mut()
            .and_then(|a| a.downcast_mut::<T>())
            .expect("the widget under test")
    }

    /// Window (Y-up) point of a widget-local one.
    pub(super) fn window(&self, local: Point) -> Point {
        Point::new(self.rect.x + local.x, self.rect.y + local.y)
    }

    /// Hold a finger at `at` until it long presses, then paint a frame.
    pub(super) fn long_press(&mut self, at: Point) {
        FINGER.down(&mut self.app, at.x, at.y);
        crate::clock::advance(LONG_PRESS_DELAY);
        self.frame();
    }

    /// A quick tap at `at`, well apart in time from any other.
    pub(super) fn tap(&mut self, at: Point) {
        crate::clock::advance(std::time::Duration::from_secs(1));
        FINGER.down(&mut self.app, at.x, at.y);
        FINGER.up(&mut self.app);
        self.frame();
    }

    /// A finger drag from `from` through `to` (past the tap tolerance).
    pub(super) fn drag(&mut self, from: Point, to: Point) {
        FINGER.down(&mut self.app, from.x, from.y);
        FINGER.move_to(&mut self.app, to.x, to.y);
        self.frame();
        FINGER.up(&mut self.app);
        self.frame();
    }
}

impl Drop for Phone {
    fn drop(&mut self) {
        set_input_profile(InputProfile::Desktop);
    }
}

/// The window point in the middle of character `byte` of a selectable
/// host, on its line.
pub(super) fn char_at<T: TouchSelectable + 'static>(phone: &mut Phone, byte: usize) -> Point {
    let w = phone.widget::<T>();
    let here = w.ts_caret_line(byte);
    let next = w.ts_caret_line(byte + 1);
    let local = Point::new((here.x + next.x) * 0.5, here.middle().y);
    phone.window(local)
}

/// The window point of the caret position before byte `byte`, on its line.
pub(super) fn caret_at<T: TouchSelectable + 'static>(phone: &mut Phone, byte: usize) -> Point {
    let local = phone.widget::<T>().ts_caret_line(byte).middle();
    phone.window(local)
}

/// The window point of a visible handle's grab area.
pub(super) fn handle<T: TouchSelectable + 'static>(phone: &mut Phone, kind: HandleKind) -> Point {
    let local = probe::handle_center(phone.widget::<T>(), kind).expect("the handle shows");
    phone.window(local)
}

/// The window point of a toolbar button.
pub(super) fn button<T: TouchSelectable + 'static>(
    phone: &mut Phone,
    action: TextMenuAction,
) -> Point {
    let local = probe::button_center(phone.widget::<T>(), action).expect("the button shows");
    phone.window(local)
}

pub(super) fn phone(profile: InputProfile) -> Phone {
    let field = TextField::new(font()).with_text(TEXT).with_font_size(16.0);
    Phone::new(profile, FIELD, Box::new(field))
}

pub(super) fn selection(p: &mut Phone) -> (usize, usize) {
    let (a, c) = p.widget::<TextField>().ts_anchor_cursor();
    (a.min(c), a.max(c))
}

pub(super) fn shows(p: &mut Phone) -> (bool, bool, bool) {
    let ts = p.widget::<TextField>().touch_selection_state();
    (
        ts.selection_handles_shown(),
        ts.caret_handle_shown(),
        ts.toolbar_shown(),
    )
}

#[test]
fn a_long_press_selects_the_word_under_the_finger_with_handles_and_then_the_toolbar() {
    let mut p = phone(InputProfile::MobileAndroid);
    let at = char_at::<TextField>(&mut p, 8); // the "a" of "brave"
    p.long_press(at);
    assert!(p.app.has_focus(), "the long press focuses the field");
    assert_eq!(selection(&mut p), (6, 11), "\"brave\" is selected");
    assert_eq!(p.widget::<TextField>().selection(), "brave");
    assert!(!p.widget::<TextField>().has_active_modal(), "no menu");
    assert_eq!(
        shows(&mut p),
        (true, false, false),
        "toolbar waits for the lift"
    );
    FINGER.up(&mut p.app);
    p.frame();
    assert_eq!(shows(&mut p), (true, false, true));
    assert_eq!(
        probe::buttons(p.widget::<TextField>()),
        vec![
            TextMenuAction::Cut,
            TextMenuAction::Copy,
            TextMenuAction::Paste,
            TextMenuAction::SelectAll
        ]
    );
    let bar = probe::toolbar_rect(p.widget::<TextField>()).expect("toolbar");
    assert!(bar.y > FIELD.height * 0.5, "above the selection: {bar:?}");
    assert!(handle::<TextField>(&mut p, HandleKind::Start).x < at.x);
    assert!(handle::<TextField>(&mut p, HandleKind::End).x > at.x);
}

#[test]
fn sliding_after_the_long_press_extends_the_selection_by_whole_words() {
    let mut p = phone(InputProfile::MobileAndroid);
    let at = char_at::<TextField>(&mut p, 8);
    p.long_press(at);
    let world = char_at::<TextField>(&mut p, 18);
    FINGER.move_to(&mut p.app, world.x, world.y);
    assert_eq!(selection(&mut p), (6, 21), "\"brave new world\"");
    assert!(!shows(&mut p).2, "no toolbar while sliding");
    let hello = char_at::<TextField>(&mut p, 1);
    FINGER.move_to(&mut p.app, hello.x, hello.y);
    assert_eq!(selection(&mut p), (0, 11), "\"hello brave\"");
    FINGER.up(&mut p.app);
    p.frame();
    assert_eq!(selection(&mut p), (0, 11));
    assert_eq!(shows(&mut p), (true, false, true));
}

#[test]
fn each_handle_drags_its_end_by_character_and_they_cannot_cross() {
    let mut p = phone(InputProfile::MobileAndroid);
    let at = char_at::<TextField>(&mut p, 8);
    p.long_press(at);
    FINGER.up(&mut p.app);
    p.frame();
    let step = char_at::<TextField>(&mut p, 1).x - char_at::<TextField>(&mut p, 0).x;
    // The end handle, two characters right: "brave n".
    let from = handle::<TextField>(&mut p, HandleKind::End);
    FINGER.down(&mut p.app, from.x, from.y);
    FINGER.move_to(&mut p.app, from.x + step * 2.0, from.y);
    assert_eq!(selection(&mut p), (6, 13));
    assert!(!shows(&mut p).2, "the toolbar hides while a handle moves");
    FINGER.up(&mut p.app);
    p.frame();
    assert_eq!(shows(&mut p), (true, false, true), "and returns on release");
    // The start handle, two characters left: "o brave n".
    let from = handle::<TextField>(&mut p, HandleKind::Start);
    p.drag(from, Point::new(from.x - step * 2.0, from.y));
    assert_eq!(selection(&mut p), (4, 13));
    // Dragged far past the end handle it stops one character short of it.
    let from = handle::<TextField>(&mut p, HandleKind::Start);
    p.drag(from, Point::new(from.x + step * 12.0, from.y));
    assert_eq!(
        selection(&mut p),
        (12, 13),
        "the start clamps before the end"
    );
    // And the end handle cannot pass the start either.
    let from = handle::<TextField>(&mut p, HandleKind::End);
    p.drag(from, Point::new(from.x - step * 10.0, from.y));
    assert_eq!(
        selection(&mut p),
        (12, 13),
        "the end clamps after the start"
    );
}

#[test]
fn a_handle_held_still_then_dragged_still_drags() {
    let mut p = phone(InputProfile::MobileAndroid);
    let at = char_at::<TextField>(&mut p, 8);
    p.long_press(at);
    FINGER.up(&mut p.app);
    p.frame();
    let step = char_at::<TextField>(&mut p, 1).x - char_at::<TextField>(&mut p, 0).x;
    let from = handle::<TextField>(&mut p, HandleKind::End);
    p.long_press(from);
    assert_eq!(
        selection(&mut p),
        (6, 11),
        "holding a handle selects nothing new"
    );
    FINGER.move_to(&mut p.app, from.x + step * 4.0, from.y);
    FINGER.up(&mut p.app);
    assert_eq!(selection(&mut p), (6, 15));
}

#[test]
fn a_tap_on_the_selection_toggles_the_toolbar_and_its_buttons_work() {
    let mut p = phone(InputProfile::MobileAndroid);
    let at = char_at::<TextField>(&mut p, 8);
    p.long_press(at);
    FINGER.up(&mut p.app);
    p.frame();
    let at = char_at::<TextField>(&mut p, 7);
    p.tap(at);
    assert_eq!(shows(&mut p), (true, false, false), "tap hides the toolbar");
    assert_eq!(selection(&mut p), (6, 11), "and keeps the selection");
    let at = char_at::<TextField>(&mut p, 9);
    p.tap(at);
    assert_eq!(shows(&mut p), (true, false, true), "tap shows it again");

    crate::clipboard::set_text("SENTINEL");
    let copy = button::<TextField>(&mut p, TextMenuAction::Copy);
    p.tap(copy);
    assert_eq!(crate::clipboard::get_text().as_deref(), Some("brave"));
    assert_eq!(
        selection(&mut p),
        (11, 11),
        "Android's Copy collapses to the end"
    );
    assert_eq!(shows(&mut p), (false, false, false));

    let at = char_at::<TextField>(&mut p, 13); // "new"

    p.long_press(at);
    FINGER.up(&mut p.app);
    p.frame();
    let cut = button::<TextField>(&mut p, TextMenuAction::Cut);
    p.tap(cut);
    assert_eq!(p.widget::<TextField>().text(), "hello brave  world");
    assert_eq!(crate::clipboard::get_text().as_deref(), Some("new"));

    let at = char_at::<TextField>(&mut p, 2); // "hello"

    p.long_press(at);
    FINGER.up(&mut p.app);
    p.frame();
    let all = button::<TextField>(&mut p, TextMenuAction::SelectAll);
    p.tap(all);
    assert_eq!(selection(&mut p), (0, 18));
    assert_eq!(
        shows(&mut p),
        (true, false, true),
        "select all keeps the toolbar"
    );
    assert!(!probe::buttons(p.widget::<TextField>()).contains(&TextMenuAction::SelectAll));
    crate::clipboard::set_text("X");
    let paste = button::<TextField>(&mut p, TextMenuAction::Paste);
    p.tap(paste);
    assert_eq!(p.widget::<TextField>().text(), "X");
}

#[test]
fn a_tap_elsewhere_clears_the_selection_and_places_the_caret_with_its_handle() {
    let mut p = phone(InputProfile::MobileAndroid);
    let at = char_at::<TextField>(&mut p, 8);
    p.long_press(at);
    FINGER.up(&mut p.app);
    p.frame();
    let at = char_at::<TextField>(&mut p, 17);
    p.tap(Point::new(at.x - 3.0, at.y)); // the left half of "o" in "world"
    assert_eq!(selection(&mut p), (17, 17));
    assert_eq!(
        shows(&mut p),
        (false, true, false),
        "Android's caret handle"
    );
    // The caret handle drags the caret by character.
    let step = char_at::<TextField>(&mut p, 1).x - char_at::<TextField>(&mut p, 0).x;
    let from = handle::<TextField>(&mut p, HandleKind::Caret);
    p.drag(from, Point::new(from.x - step * 3.0, from.y));
    assert_eq!(selection(&mut p), (14, 14));
    // Tapping it toggles the Paste toolbar.
    let from = handle::<TextField>(&mut p, HandleKind::Caret);
    p.tap(from);
    assert_eq!(
        probe::buttons(p.widget::<TextField>()),
        vec![TextMenuAction::Paste, TextMenuAction::SelectAll]
    );
    // It fades after a while...
    crate::clock::advance(CARET_HANDLE_TIME);
    p.frame();
    assert!(!shows(&mut p).1);
    // ...and on typing.
    let at = char_at::<TextField>(&mut p, 2);
    p.tap(at);
    assert!(shows(&mut p).1);
    p.app
        .on_key_down(crate::Key::Char('x'), Modifiers::default());
    assert_eq!(shows(&mut p), (false, false, false));
}

#[test]
fn a_long_press_past_the_text_places_the_caret_and_offers_paste() {
    let mut p = phone(InputProfile::MobileAndroid);
    let past = p.window(Point::new(FIELD.width - 20.0, FIELD.height * 0.5));
    p.long_press(past);
    FINGER.up(&mut p.app);
    p.frame();
    assert_eq!(selection(&mut p), (21, 21));
    assert_eq!(shows(&mut p), (false, true, true));
    assert_eq!(
        probe::buttons(p.widget::<TextField>()),
        vec![TextMenuAction::Paste, TextMenuAction::SelectAll]
    );
}

#[test]
fn ios_draws_no_caret_handle_and_keeps_the_selection_after_copy() {
    let mut p = phone(InputProfile::MobileIOS);
    let at = char_at::<TextField>(&mut p, 8);
    p.long_press(at);
    FINGER.up(&mut p.app);
    p.frame();
    assert_eq!(shows(&mut p), (true, false, true));
    // The toolbar clears the start handle's knob above the line.
    let bar = probe::toolbar_rect(p.widget::<TextField>()).expect("toolbar");
    let line_top = p.widget::<TextField>().ts_caret_line(6).top;
    let knob = crate::widgets::touch_selection::handles::KNOB_RADIUS * 2.0;
    assert!(
        bar.y >= line_top + knob,
        "{bar:?} over a line topped at {line_top}"
    );
    let copy = button::<TextField>(&mut p, TextMenuAction::Copy);
    p.tap(copy);
    assert_eq!(selection(&mut p), (6, 11));
    assert_eq!(shows(&mut p), (true, false, false));
    let at = caret_at::<TextField>(&mut p, 2);
    p.tap(at);
    assert_eq!(selection(&mut p), (2, 2));
    assert_eq!(
        shows(&mut p),
        (false, false, false),
        "no caret handle on iOS"
    );
}

#[test]
fn the_mouse_keeps_the_desktop_behaviour() {
    let mut p = phone(InputProfile::Desktop);
    let at = char_at::<TextField>(&mut p, 8);
    let (sx, sy) = (at.x, H - at.y);
    // A held mouse button is no long press: it places the caret.
    p.app
        .on_mouse_down(sx, sy, MouseButton::Left, Modifiers::default());
    crate::clock::advance(LONG_PRESS_DELAY * 2);
    p.frame();
    p.app
        .on_mouse_up(sx, sy, MouseButton::Left, Modifiers::default());
    let (lo, hi) = selection(&mut p);
    assert_eq!(lo, hi, "a caret, no word");
    assert_eq!(shows(&mut p), (false, false, false));
    // A mouse drag selects by character.
    let to = caret_at::<TextField>(&mut p, 13);
    p.app
        .on_mouse_down(sx, sy, MouseButton::Left, Modifiers::default());
    p.app.on_mouse_move(to.x, H - to.y);
    p.app
        .on_mouse_up(to.x, H - to.y, MouseButton::Left, Modifiers::default());
    assert_eq!(selection(&mut p), (lo, 13));
    assert_eq!(shows(&mut p), (false, false, false));
    // A right click still opens the context menu.
    p.app
        .on_mouse_down(sx, sy, MouseButton::Right, Modifiers::default());
    assert!(p.widget::<TextField>().has_active_modal());
}

#[test]
fn a_mouse_press_hides_what_a_finger_left() {
    let mut p = phone(InputProfile::MobileAndroid);
    let at = char_at::<TextField>(&mut p, 8);
    p.long_press(at);
    FINGER.up(&mut p.app);
    p.frame();
    let at = char_at::<TextField>(&mut p, 2);
    p.app
        .on_mouse_down(at.x, H - at.y, MouseButton::Left, Modifiers::default());
    p.app
        .on_mouse_up(at.x, H - at.y, MouseButton::Left, Modifiers::default());
    assert_eq!(shows(&mut p), (false, false, false));
}

#[test]
fn handles_and_toolbar_go_when_focus_leaves() {
    let mut p = phone(InputProfile::MobileAndroid);
    let at = char_at::<TextField>(&mut p, 8);
    p.long_press(at);
    FINGER.up(&mut p.app);
    p.frame();
    assert_eq!(shows(&mut p), (true, false, true));
    p.tap(Point::new(200.0, 300.0)); // the empty board below the field
    assert!(!p.app.has_focus());
    assert_eq!(shows(&mut p), (false, false, false));
    assert!(probe::handle_center(p.widget::<TextField>(), HandleKind::End).is_none());
}
