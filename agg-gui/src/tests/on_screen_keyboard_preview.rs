//! The on-screen keyboard's key preview: when it goes, and its shape.
//!
//! A tapped letter stays lit with its preview for [`PRESS_LINGER`] after the
//! lift, and a held letter opens its alternates after [`LONG_PRESS_DELAY`].
//! Both are timed changes with no input behind them, so the app has to ask
//! the host for a frame at that moment: a reactive host (the web shell, the
//! desktop shell) sleeps until its next scheduled draw, and without one the
//! preview stayed up until some unrelated repaint — about a second on a
//! Pixel. These tests drive the real `App` touch entry points on the
//! virtual clock and serve frames the way a reactive host does: only when
//! `App::wants_draw` says so or `App::next_draw_deadline` comes due.
//!
//! On the Android (Gboard) keyboard the single-letter preview is a circle
//! about 1.4 key widths across, centred over the key, as Gboard draws it.
//! Companion to `on_screen_keyboard_zoom.rs` (the touch entry points) and
//! `on_screen_keyboard_longpress.rs` (the alternates).

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use web_time::Instant;

use super::on_screen_keyboard_touch::{center, char_key, font, Phone};
use crate::geometry::Point;
use crate::input_profile::InputProfile;
use crate::touch_state::{TouchDeviceId, TouchId};
use crate::widgets::on_screen_keyboard::alternates::LONG_PRESS_DELAY;
use crate::widgets::on_screen_keyboard::state::with_state_ref;
use crate::widgets::on_screen_keyboard::style::Style;
use crate::widgets::TextField;
use crate::Size;

/// A Pixel's viewport in CSS pixels.
const PIXEL: Size = Size {
    width: 412.0,
    height: 915.0,
};

/// How long a lifted key stays lit with its preview.
const LINGER: Duration = Duration::from_millis(100);

const DEV: TouchDeviceId = TouchDeviceId(0);
const FINGER: TouchId = TouchId(1);

fn phone_with_text() -> (Phone, Rc<RefCell<String>>) {
    let text = Rc::new(RefCell::new(String::new()));
    let root = Box::new(TextField::new(font()).with_text_cell(text.clone()));
    let mut phone = Phone::with(root, false, InputProfile::MobileAndroid, PIXEL);
    // Let the keyboard's slide and the content's lift above it finish, so
    // the app is idle before the first touch.
    for _ in 0..40 {
        if !phone.app.wants_draw() {
            return (phone, text);
        }
        crate::clock::advance(Duration::from_millis(50));
        phone.paint();
    }
    panic!("the app settles once the keyboard is up");
}

/// Y-up `p` as the Y-down screen point a touch arrives at.
fn screen(phone: &Phone, p: Point) -> (f64, f64) {
    (p.x, phone.size.height - p.y)
}

/// The colour a framebuffer shows at Y-up `(x, y)`.
fn pixel(fb: &crate::Framebuffer, x: f64, y: f64) -> [u8; 3] {
    let i = ((y as usize) * fb.width() as usize + x as usize) * 4;
    let p = fb.pixels();
    [p[i], p[i + 1], p[i + 2]]
}

/// Serve the frames a reactive host would: while the app wants a frame,
/// paint one. Returns the last frame painted, if any.
fn serve_pending_frames(phone: &mut Phone) -> Option<crate::Framebuffer> {
    let mut last = None;
    for _ in 0..8 {
        if !phone.app.wants_draw() {
            return last;
        }
        last = Some(phone.paint());
    }
    panic!("the app keeps asking for frames while nothing changes");
}

/// Sleep the way a reactive host does, to the app's next scheduled draw,
/// and serve the frames it then asks for. Returns the time it woke and the
/// frame it painted there.
fn sleep_to_next_wake(phone: &mut Phone) -> (Instant, crate::Framebuffer) {
    assert!(!phone.app.wants_draw(), "the app is idle before it sleeps");
    let wake = phone
        .app
        .next_draw_deadline()
        .expect("the app has a wake scheduled");
    let now = crate::clock::now();
    assert!(wake > now, "the scheduled wake is in the future");
    crate::clock::advance(wake - now);
    let frame = serve_pending_frames(phone).expect("the wake asks for a frame");
    (wake, frame)
}

#[test]
fn a_tapped_keys_preview_goes_100_ms_after_the_lift_and_the_app_wakes_for_it() {
    let _clock = crate::clock::scoped_virtual(None);
    let (mut phone, text) = phone_with_text();
    let h = char_key('h').rect;
    // Just above the key, in the gap between rows: the tray, until the
    // preview bubble covers it.
    let probe = (center(h).x, h.y + h.height + 3.0);
    let idle = pixel(&phone.paint(), probe.0, probe.1);
    let (x, y) = screen(&phone, center(h));
    phone.app.on_touch_start(DEV, FINGER, x, y, None);
    let held = serve_pending_frames(&mut phone).expect("the press asks for a frame");
    let held = pixel(&held, probe.0, probe.1);
    assert_ne!(held, idle, "the preview shows while the finger is down");
    crate::clock::advance(Duration::from_millis(30));
    phone.app.on_touch_end(DEV, FINGER);
    let lifted_at = crate::clock::now();
    assert_eq!(*text.borrow(), "h", "the lift types the key once");
    let after = serve_pending_frames(&mut phone).expect("the lift asks for a frame");
    assert_eq!(pixel(&after, probe.0, probe.1), held, "the preview lingers");
    // The app is idle now, so the host sleeps from one scheduled wake to
    // the next (the caret's blink among them). The preview is still up in
    // every frame before the linger ends, and the app wakes exactly when
    // it does, with the preview gone.
    let (gone_at, _) = loop {
        let (woke, frame) = sleep_to_next_wake(&mut phone);
        if pixel(&frame, probe.0, probe.1) == idle {
            break (woke, frame);
        }
        assert!(
            woke < lifted_at + LINGER,
            "still up {:?} after the lift",
            woke - lifted_at
        );
        assert_eq!(pixel(&frame, probe.0, probe.1), held);
    };
    assert_eq!(
        gone_at - lifted_at,
        LINGER,
        "the app wakes when the preview is due to go"
    );
}

#[test]
fn a_frame_just_before_the_linger_ends_still_shows_the_preview() {
    let _clock = crate::clock::scoped_virtual(None);
    let (mut phone, _text) = phone_with_text();
    let h = char_key('h').rect;
    let probe = (center(h).x, h.y + h.height + 3.0);
    let idle = pixel(&phone.paint(), probe.0, probe.1);
    let (x, y) = screen(&phone, center(h));
    phone.app.on_touch_start(DEV, FINGER, x, y, None);
    let held = pixel(&phone.paint(), probe.0, probe.1);
    phone.app.on_touch_end(DEV, FINGER);
    phone.paint();
    crate::clock::advance(LINGER - Duration::from_millis(1));
    assert_eq!(pixel(&phone.paint(), probe.0, probe.1), held, "at 99 ms");
    crate::clock::advance(Duration::from_millis(1));
    assert_eq!(pixel(&phone.paint(), probe.0, probe.1), idle, "at 100 ms");
}

#[test]
fn a_held_key_wakes_the_app_the_moment_its_alternates_are_due() {
    let _clock = crate::clock::scoped_virtual(None);
    let (mut phone, _text) = phone_with_text();
    let (x, y) = screen(&phone, center(char_key('e').rect));
    phone.app.on_touch_start(DEV, FINGER, x, y, None);
    let pressed_at = crate::clock::now();
    serve_pending_frames(&mut phone).expect("the press asks for a frame");
    // A still finger moves nothing, so the host sleeps between wakes.
    let opened_at = loop {
        let (woke, _) = sleep_to_next_wake(&mut phone);
        if with_state_ref(|s| s.alternates.is_some()) {
            break woke;
        }
        assert!(
            woke < pressed_at + LONG_PRESS_DELAY,
            "not open {:?} after the press",
            woke - pressed_at
        );
    };
    assert_eq!(
        opened_at - pressed_at,
        LONG_PRESS_DELAY,
        "the alternates open on time"
    );
    phone.app.on_touch_end(DEV, FINGER);
}

/// The Android preview's circle as Gboard draws it over `key`: 1.4 key
/// widths across, centred on the key, its bottom half a row gap above it.
fn gboard_circle(key: crate::geometry::Rect, style: &Style) -> (Point, f64) {
    let r = key.width * 1.4 / 2.0;
    let c = Point::new(
        key.x + key.width / 2.0,
        key.y + key.height + style.key_v_gap * 0.5 + r,
    );
    (c, r)
}

#[test]
fn the_android_preview_of_a_letter_is_a_circle() {
    let _clock = crate::clock::scoped_virtual(None);
    let (mut phone, _text) = phone_with_text();
    let style = Style::for_profile(InputProfile::MobileAndroid);
    let g = char_key('g').rect;
    let idle = phone.paint();
    let (x, y) = screen(&phone, center(g));
    phone.app.on_touch_start(DEV, FINGER, x, y, None);
    let held = phone.paint();
    let (c, r) = gboard_circle(g, &style);
    let face = [style.preview_bg.r, style.preview_bg.g, style.preview_bg.b]
        .map(|v| (v * 255.0).round() as u8);
    let at = |angle_deg: f64, k: f64| {
        let a = angle_deg.to_radians();
        (c.x + a.cos() * r * k, c.y + a.sin() * r * k)
    };
    // Every way out from the centre, the face reaches nearly the radius...
    for deg in (0..360).step_by(45) {
        let (px, py) = at(deg as f64, 0.88);
        assert_eq!(pixel(&held, px, py), face, "inside the circle at {deg}°");
    }
    // ...and stops short of the corners of its square, and above its top
    // (below, the bubble sits on the pressed key and its shadow).
    for deg in [45.0, 90.0, 135.0] {
        let (px, py) = at(deg, 1.2);
        assert_eq!(
            pixel(&held, px, py),
            pixel(&idle, px, py),
            "outside the circle at {deg}°"
        );
    }
    phone.app.on_touch_cancel(DEV, FINGER);
}
