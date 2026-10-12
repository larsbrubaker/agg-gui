//! agg-sharp `TextEditCaretTests` (Tests/Agg.Tests/Agg.UI/TextEditCaretTests.cs):
//! the insert caret of a single-line [`TextField`] goes on blinking for as
//! long as the field holds the keyboard, is drawn at the weight of the display
//! it is on, an uncoloured field still draws its caret and its selection, and
//! the field brings the text under the caret back into view in both
//! directions.
//!
//! The pictures are painted through the production `paint_subtree` path
//! (body, then the caret overlay) into a software `GfxCtx` cleared to white,
//! with the field placed at (20, 20) in a 120 x 80 image as in C#. C#'s
//! `InternalTextEditWidget` is the frameless inner field, so the field here
//! is built `with_frame(false)`: the only thing that can paint in an empty
//! field is then the caret. One Rust test per C# `[Arguments]` row.
//!
//! Translation notes:
//! - The blink is computed from the UI clock (`crate::clock`), not driven by a
//!   chain of idle callbacks, so "pumping" is advancing the virtual clock;
//!   agg-gui blinks at 500 ms on / 500 ms off where C# used 600 ms, and the
//!   1400 ms window still covers a full cycle.
//! - agg-gui lays out in logical units and the App paints through
//!   `ctx.scale(device_scale)`; the device-scale test paints the same way into
//!   a device-pixel image and measures the caret there.
//! - C#'s `TopLeftOffset.X` is the field's horizontal scroll, `scroll_x`
//!   (positive where C#'s offset is negative; only "zero or not" is asserted).
//! - The three ThemedTextEditWidget frame-fit tests
//!   (`AFrameAskedToFitDoesNotPushItsFieldOffItsOwnLeftEdge`,
//!   `AFieldDoesNotKeepTheWidthAStretchGaveIt`,
//!   `ALabelRefreshedWhileStretchedDoesNotBecomeTheReservedWidth`) pin
//!   `ThemedTextEditWidget`'s frame + inner field + `TrailingLabel` composite,
//!   which agg-gui does not have: a `TextField` is one widget whose text area
//!   is its bounds minus its insets. That composite is MatterCAD's length box
//!   (`mattercad-app` `AxisBox`), where those tests belong.

use std::sync::Arc;
use std::time::Duration;

use super::*;
use crate::framebuffer::Framebuffer;
use crate::gfx_ctx::GfxCtx;
use crate::widget::{paint_subtree, Widget};

const FONT_BYTES: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");

const IMAGE_WIDTH: u32 = 120;
const IMAGE_HEIGHT: u32 = 80;

/// Where the field is placed in the image, so the caret has white on every
/// side of it to be measured against.
const FIELD_ORIGIN: (f64, f64) = (20.0, 20.0);

/// The field's laid-out size inside the image (logical units).
const FIELD_SIZE: Size = Size {
    width: 80.0,
    height: 40.0,
};

/// What the user does part way through a blink cycle, each of which restarts
/// the blink.
#[derive(Clone, Copy, PartialEq)]
enum MidCycleEvent {
    Nothing,
    ReFocus,
    KeyDown,
}

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
}

/// Holds the input-profile lock with grayscale text and device scale 1 for the
/// duration of `body`, putting the shared settings back afterwards.
fn with_fixed_settings<T>(body: impl FnOnce() -> T) -> T {
    let _profile = crate::input_profile::profile_test_lock();
    crate::device_scale::set_device_scale(1.0);
    crate::font_settings::set_lcd_enabled(false);
    let result = body();
    crate::font_settings::clear_lcd_enabled_override();
    crate::device_scale::set_device_scale(1.0);
    result
}

/// C#'s `FieldInAContainer`: a frameless field holding `text`, laid out at
/// [`FIELD_SIZE`], left exactly as constructed (agg-gui's colours come from the
/// visuals unless a theme overrides them, so there is no `setTextColor` step).
fn field_in_a_container(text: &str) -> TextField {
    let mut f = TextField::new(font())
        .with_font_size(12.0)
        .with_frame(false)
        .with_text(text);
    f.set_bounds(Rect::new(0.0, 0.0, FIELD_SIZE.width, FIELD_SIZE.height));
    f.layout(FIELD_SIZE);
    f.set_bounds(Rect::new(0.0, 0.0, FIELD_SIZE.width, FIELD_SIZE.height));
    f
}

/// C#'s `FocusedEmptyField`: an empty field holding the keyboard.
fn focused_empty_field() -> TextField {
    let mut f = field_in_a_container("");
    f.on_event(&Event::FocusGained);
    f
}

/// C#'s `Render`: the field painted at [`FIELD_ORIGIN`] in a white image the
/// size of the container times the device scale.
fn render(f: &mut TextField) -> Framebuffer {
    let s = crate::device_scale::device_scale();
    let w = (IMAGE_WIDTH as f64 * s).ceil() as u32;
    let h = (IMAGE_HEIGHT as f64 * s).ceil() as u32;
    let mut fb = Framebuffer::new(w, h);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::rgba(1.0, 1.0, 1.0, 1.0));
        ctx.scale(s, s);
        ctx.translate(FIELD_ORIGIN.0, FIELD_ORIGIN.1);
        paint_subtree(f, &mut ctx);
    }
    fb
}

fn pixel(fb: &Framebuffer, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * fb.width() + x) * 4) as usize;
    let p = fb.pixels();
    [p[i], p[i + 1], p[i + 2], p[i + 3]]
}

/// A device-pixel rectangle, inclusive-exclusive, as C#'s `RectangleInt`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct PaintedRect {
    left: u32,
    bottom: u32,
    right: u32,
    top: u32,
}

impl PaintedRect {
    fn width(&self) -> u32 {
        self.right - self.left
    }
    fn height(&self) -> u32 {
        self.top - self.bottom
    }
}

/// C#'s `CaretBounds`: the bounds of everything that is not white, which for
/// an empty field is the caret and nothing else. Empty means the caret is in
/// the off half of its blink.
fn caret_bounds(f: &mut TextField) -> PaintedRect {
    let fb = render(f);
    let (mut left, mut bottom, mut right, mut top) = (u32::MAX, u32::MAX, 0, 0);
    let mut any = false;
    for y in 0..fb.height() {
        for x in 0..fb.width() {
            if pixel(&fb, x, y) != [255, 255, 255, 255] {
                any = true;
                left = left.min(x);
                bottom = bottom.min(y);
                right = right.max(x + 1);
                top = top.max(y + 1);
            }
        }
    }
    if any {
        PaintedRect {
            left,
            bottom,
            right,
            top,
        }
    } else {
        PaintedRect::default()
    }
}

fn pixels_that_differ(first: &Framebuffer, second: &Framebuffer) -> usize {
    let mut differing = 0;
    for y in 0..first.height() {
        for x in 0..first.width() {
            if pixel(first, x, y) != pixel(second, x, y) {
                differing += 1;
            }
        }
    }
    differing
}

/// C#'s `CaretStatesOverAFullCycle`: whether the caret is painted, once every
/// tenth of a second across a full on-and-off cycle.
fn caret_states_over_a_full_cycle(f: &mut TextField) -> Vec<bool> {
    let cycle = Duration::from_millis(1400);
    let sample_every = Duration::from_millis(100);
    let mut seen = Vec::new();
    let mut elapsed = Duration::ZERO;
    while elapsed < cycle {
        seen.push(caret_bounds(f).width() > 0);
        crate::clock::advance(sample_every);
        elapsed += sample_every;
    }
    seen
}

fn the_caret_blinks_for_as_long_as_the_field_holds_the_keyboard(mid_cycle: MidCycleEvent) {
    with_fixed_settings(|| {
        crate::clock::start_virtual();
        let mut f = focused_empty_field();

        if mid_cycle != MidCycleEvent::Nothing {
            // C#'s PumpFor(200 ms), painting as a running app does.
            for _ in 0..10 {
                render(&mut f);
                crate::clock::advance(Duration::from_millis(20));
            }
            if mid_cycle == MidCycleEvent::ReFocus {
                // what a reparent does - popping the editor out into a window
                // of its own hands the keyboard back to the field it took it from
                f.on_event(&Event::FocusLost);
                f.on_event(&Event::FocusGained);
            } else {
                // any key the user presses restarts the blink; Home is one that
                // leaves the field empty, so the only thing that can paint in it
                // is still the caret
                f.on_event(&Event::KeyDown {
                    key: Key::Home,
                    modifiers: Modifiers::default(),
                });
            }
        }

        let seen = caret_states_over_a_full_cycle(&mut f);
        crate::clock::use_real();

        assert!(
            seen.iter().any(|&showing| showing),
            "the caret has to be drawn at some point in the cycle for the user to see where they are typing: {seen:?}"
        );
        assert!(
            seen.iter().any(|&showing| !showing),
            "a caret that is never off is not blinking; it is a line the field has been left holding: {seen:?}"
        );
    });
}

#[test]
fn the_caret_blinks_for_as_long_as_the_field_holds_the_keyboard_nothing() {
    the_caret_blinks_for_as_long_as_the_field_holds_the_keyboard(MidCycleEvent::Nothing);
}

#[test]
fn the_caret_blinks_for_as_long_as_the_field_holds_the_keyboard_re_focus() {
    the_caret_blinks_for_as_long_as_the_field_holds_the_keyboard(MidCycleEvent::ReFocus);
}

#[test]
fn the_caret_blinks_for_as_long_as_the_field_holds_the_keyboard_key_down() {
    the_caret_blinks_for_as_long_as_the_field_holds_the_keyboard(MidCycleEvent::KeyDown);
}

/// C#'s `CaretBoundsAtDeviceScale`.
fn caret_bounds_at_device_scale(device_scale: f64) -> PaintedRect {
    crate::clock::start_virtual();
    crate::device_scale::set_device_scale(device_scale);
    let mut f = focused_empty_field();
    let bounds = caret_bounds(&mut f);
    crate::device_scale::set_device_scale(1.0);
    crate::clock::use_real();
    bounds
}

#[test]
fn the_caret_grows_with_device_scale() {
    with_fixed_settings(|| {
        let at_one = caret_bounds_at_device_scale(1.0);
        let at_two = caret_bounds_at_device_scale(2.0);

        assert!(at_one.width() > 0, "the caret paints at scale 1");
        assert_eq!(
            at_two.width(),
            at_one.width() * 2,
            "the caret is {} hardware pixels wide at scale 1, so it has to be twice that at scale 2 rather than the {} a fixed pixel width leaves it",
            at_one.width(),
            at_two.width()
        );
        assert_eq!(
            at_two.height(),
            at_one.height() * 2,
            "the caret spans the text, and the text doubled ({at_one:?} vs {at_two:?})"
        );
    });
}

#[test]
fn a_field_nobody_has_coloured_still_draws_a_caret() {
    with_fixed_settings(|| {
        crate::clock::start_virtual();
        let mut f = focused_empty_field();
        assert!(f.theme.cursor_color.is_none(), "the field was never themed");
        let width = caret_bounds(&mut f).width();
        crate::clock::use_real();
        assert!(
            width > 0,
            "the caret has to be painted in a field that was never themed, not left transparent"
        );
    });
}

#[test]
fn a_field_nobody_has_coloured_still_draws_its_selection() {
    with_fixed_settings(|| {
        // Deliberately not focused, so the caret cannot paint and cannot
        // blink: the only thing that can differ is the selection.
        let mut f = field_in_a_container("abc");
        assert!(f.theme.selection_bg_unfocused.is_none(), "never themed");

        let without_selection = render(&mut f);

        // set_cursor_position clears any selection, so it has to come first
        f.set_cursor_position(3);
        f.edit.borrow_mut().anchor = 0;
        f.layout(FIELD_SIZE);

        let with_selection = render(&mut f);

        assert!(
            pixels_that_differ(&without_selection, &with_selection) > 0,
            "selecting text in a field that was never themed has to paint a band behind it, not leave the field looking exactly as it did"
        );
    });
}

/// C#'s `NarrowFieldHoldingWideText`: a single-line field 20 wide holding a
/// value several times that width, laid out so its scrolling is live, and
/// holding the keyboard. C#'s `TextEditWidget` has no inset of its own.
fn narrow_field_holding_wide_text() -> (TextField, &'static str) {
    let text = "2.73109";
    let mut f = TextField::new(font())
        .with_font_size(12.0)
        .with_padding(0.0)
        .with_text(text);
    let size = Size::new(20.0, 16.0);
    f.layout(size);
    f.set_bounds(Rect::new(FIELD_ORIGIN.0, FIELD_ORIGIN.1, 20.0, 16.0));
    f.on_event(&Event::FocusGained);
    (f, text)
}

fn key(f: &mut TextField, key: Key) {
    f.on_event(&Event::KeyDown {
        key,
        modifiers: Modifiers::default(),
    });
}

fn the_field_follows_the_caret_back_to_the_front_of_its_value(with_key: Key) {
    let (mut f, text) = narrow_field_holding_wide_text();

    key(&mut f, Key::End);

    let scrolled_right = f.scroll_x;
    assert_ne!(
        scrolled_right, 0.0,
        "a caret walked off the right edge has to pull the view after it"
    );

    if with_key == Key::Home {
        key(&mut f, Key::Home);
    } else {
        for _ in 0..text.chars().count() {
            key(&mut f, Key::ArrowLeft);
        }
    }

    assert_eq!(f.cursor_pos(), 0, "the caret is at the front of the text");
    assert_eq!(
        f.scroll_x, 0.0,
        "the view has to come back to the front of the value rather than stay at {scrolled_right}"
    );
}

#[test]
fn the_field_follows_the_caret_back_to_the_front_of_its_value_home() {
    the_field_follows_the_caret_back_to_the_front_of_its_value(Key::Home);
}

#[test]
fn the_field_follows_the_caret_back_to_the_front_of_its_value_left() {
    the_field_follows_the_caret_back_to_the_front_of_its_value(Key::ArrowLeft);
}
