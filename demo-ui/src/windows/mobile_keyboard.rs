//! Mobile keyboard demo.
//!
//! Lets the user toggle the global [`InputProfile`](agg_gui::input_profile::InputProfile)
//! between Desktop, iPhone, and Android and watch the on-screen software
//! keyboard slide up under a focused TextField. The same demo doubles as
//! the test bench for the keyboard during development.
//!
//! Switching profiles flips a few globals:
//! - [`agg_gui::input_profile::set_input_profile`] — picks the visual
//!   style (iOS light-gray chrome vs. Android Material dark vs. neutral
//!   fallback).
//! - [`agg_gui::widgets::on_screen_keyboard::set_enabled`] — turns the
//!   keyboard module on (mobile profiles) or off (desktop).
//! - [`agg_gui::widgets::on_screen_keyboard::set_rises_for_mouse`] — on
//!   with a mobile profile. An app raises the keyboard only for a field a
//!   finger or a pen focused (a mouse user has a physical keyboard); this
//!   bench is usually driven with a mouse, so it asks for the keyboard
//!   whatever pressed.
//!
//! When the user taps or clicks the demo's TextField with a mobile profile
//! the keyboard raises because TextField overrides
//! `Widget::accepts_text_input`. Picking "Desktop" both disables the
//! keyboard and dismisses it.
//!
//! The demo also exposes a per-field "Input mode" radio (Text /
//! Numeric).  Picking Numeric shares a single
//! [`KeyboardInputMode`](agg_gui::widgets::on_screen_keyboard::KeyboardInputMode)
//! cell with the first text field; the next time that field receives
//! focus, the keyboard opens directly on the digit pad — the same
//! convention as iOS `numberPad` / HTML `<input type="number">`.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::input_profile::{current_input_profile, set_input_profile, InputProfile};
use agg_gui::widgets::on_screen_keyboard::{self, KeyboardInputMode};
use agg_gui::{FlexColumn, Font, Label, RadioGroup, ScrollView, SizedBox, TextField, Widget};

/// Returns the title used to look this demo up from the dispatcher in
/// `content.rs` and to label it in the sidebar. Kept centralised so the
/// `DEMOS` table entry and the dispatcher match without typo risk.
pub const TITLE: &str = "\u{F11C} Mobile Keyboard";

/// Build the Mobile Keyboard demo widget tree.
pub fn mobile_keyboard(font: Arc<Font>) -> Box<dyn Widget> {
    let initial = match current_input_profile() {
        InputProfile::Desktop => 0,
        InputProfile::MobileIOS => 1,
        InputProfile::MobileAndroid => 2,
        InputProfile::MobileOther => 0,
    };

    // Tight, even vertical rhythm: an 8 px inter-item gap (down from 12)
    // and 12 px padding keeps the whole demo visible without scrolling at
    // the default window height.  All text colours follow the active theme
    // (see the `Label` calls below) so the demo stays readable in light
    // mode, not just dark.
    let mut col = FlexColumn::new()
        .with_gap(8.0)
        .with_padding(12.0)
        .with_panel_bg();

    col.push(
        Box::new(Label::new("Input profile", Arc::clone(&font)).with_font_size(13.0)),
        0.0,
    );

    let profiles: Vec<&str> = vec!["Desktop (no keyboard)", "iPhone", "Android"];
    let radio = RadioGroup::new(profiles, initial, Arc::clone(&font))
        .with_font_size(13.0)
        .on_change(apply_profile_choice);
    col.push(Box::new(radio), 0.0);

    col.push(
        Box::new(
            Label::new(
                "Picking iPhone or Android flips the global agg-gui input \
                 profile and enables the on-screen keyboard. Click or tap \
                 the field below and the keyboard slides up and types in it.",
                Arc::clone(&font),
            )
            .with_font_size(11.0)
            .with_dim(true)
            .with_wrap(true),
        ),
        0.0,
    );

    // Shared cell driving the first text field's keyboard mode. The
    // RadioGroup below mutates it on selection; the TextField re-reads
    // it on every focus event, so picking "Numeric" then tapping the
    // field raises the digit pad instead of the letter row.
    let primary_mode = Rc::new(Cell::new(KeyboardInputMode::Text));

    col.push(
        Box::new(Label::new("First field input mode", Arc::clone(&font)).with_font_size(13.0)),
        0.0,
    );
    let mode_radio = {
        let cell = Rc::clone(&primary_mode);
        RadioGroup::new(
            vec!["Text (letters)", "Numeric (digit pad)"],
            0,
            Arc::clone(&font),
        )
        .with_font_size(13.0)
        .on_change(move |idx| apply_mode_choice(&cell, idx))
    };
    col.push(Box::new(mode_radio), 0.0);

    col.push(
        Box::new(
            Label::new(
                "Picking Numeric routes the first field below to the \
                 numbers layer the next time it gains focus. Tap the \
                 field again after switching to see the new layer.",
                Arc::clone(&font),
            )
            .with_font_size(11.0)
            .with_dim(true)
            .with_wrap(true),
        ),
        0.0,
    );

    col.push(
        Box::new(
            Label::new("Type here (mode follows radio)", Arc::clone(&font)).with_font_size(12.0),
        ),
        0.0,
    );
    col.push(
        Box::new(
            SizedBox::new().with_height(34.0).with_child(Box::new(
                TextField::new(Arc::clone(&font))
                    .with_font_size(14.0)
                    .with_placeholder("Tap or click to focus, then type / tap keys…")
                    .with_keyboard_mode_cell(Rc::clone(&primary_mode)),
            )),
        ),
        0.0,
    );

    col.push(
        Box::new(
            Label::new("Numeric-only field (digit pad)", Arc::clone(&font)).with_font_size(12.0),
        ),
        0.0,
    );
    col.push(
        Box::new(
            SizedBox::new().with_height(34.0).with_child(Box::new(
                TextField::new(Arc::clone(&font))
                    .with_font_size(14.0)
                    .with_placeholder("Always opens on the numbers layer")
                    .with_keyboard_mode(KeyboardInputMode::Numeric),
            )),
        ),
        0.0,
    );

    col.push(
        Box::new(
            Label::new(
                "Tip: try Shift, the 123 / ABC mode switch, and the \
                 down-chevron in the bottom-left of the keyboard to dismiss \
                 without changing focus.",
                Arc::clone(&font),
            )
            .with_font_size(10.0)
            .with_dim(true)
            .with_wrap(true),
        ),
        0.0,
    );

    // Wrap in a ScrollView so the window content stays usable even when
    // the keyboard slides up and covers the bottom half — once the
    // scroll-into-view milestone lands, this will auto-shift to keep
    // the focused field visible.
    Box::new(ScrollView::new(Box::new(col)))
}

/// Map the radio-button index to an [`InputProfile`] and apply it
/// globally, with the keyboard raised for mouse clicks while a mobile
/// profile is on. Index `0` (Desktop) also dismisses any currently-raised
/// keyboard so the demo doesn't leave it hanging.
fn apply_profile_choice(idx: usize) {
    let profile = match idx {
        1 => InputProfile::MobileIOS,
        2 => InputProfile::MobileAndroid,
        _ => InputProfile::Desktop,
    };
    set_input_profile(profile);
    keyboard_for_profile(profile);
    agg_gui::animation::request_draw();
}

/// The keyboard settings for `profile`: on for a phone, raised for a mouse
/// click too (the bench is driven with a mouse; in an app only a finger or
/// a pen raises it), and off and put away on the desktop.
fn keyboard_for_profile(profile: InputProfile) {
    let enable = profile.is_mobile_touch();
    on_screen_keyboard::set_enabled(enable);
    on_screen_keyboard::set_rises_for_mouse(enable);
    if !enable {
        on_screen_keyboard::dismiss();
    }
}

/// Stamp the radio-button index into the shared keyboard-mode cell.
/// The bound TextField re-reads the cell on each focus event, so a
/// re-focus (tap the field again) is enough to swap the keyboard
/// layer.  Kept out of the radio's closure so the mapping is testable
/// in isolation.
fn apply_mode_choice(cell: &Rc<Cell<KeyboardInputMode>>, idx: usize) {
    let mode = match idx {
        1 => KeyboardInputMode::Numeric,
        _ => KeyboardInputMode::Text,
    };
    cell.set(mode);
    agg_gui::animation::request_draw();
}

#[cfg(test)]
mod tests {
    use super::*;
    use agg_gui::{App, Modifiers, MouseButton, Size};

    const TEST_FONT: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");

    /// With a phone picked, a mouse click on the bench's field raises the
    /// keyboard; with the desktop picked, it doesn't. The keyboard's state is
    /// per thread, and the profile itself is left alone.
    #[test]
    fn a_mouse_click_on_the_field_raises_the_keyboard_with_a_phone_picked() {
        let font = Arc::new(Font::from_slice(TEST_FONT).expect("test font must load"));
        let size = Size::new(300.0, 60.0);
        let mut app = App::new(Box::new(TextField::new(font)));
        app.layout(size);
        let click = |app: &mut App| {
            app.on_mouse_down(150.0, 30.0, MouseButton::Left, Modifiers::default());
            app.on_mouse_up(150.0, 30.0, MouseButton::Left, Modifiers::default());
        };
        let keyboard_up = || {
            on_screen_keyboard::state::with_state_ref(|s| {
                s.enabled && s.text_input_focused && s.slide.target() > 0.5
            })
        };
        keyboard_for_profile(InputProfile::MobileAndroid);
        click(&mut app);
        assert!(app.has_focus() && keyboard_up(), "the click raises it");
        keyboard_for_profile(InputProfile::Desktop);
        assert!(!keyboard_up(), "the desktop puts it away");
        assert!(!on_screen_keyboard::rises_for_mouse());
    }
}
