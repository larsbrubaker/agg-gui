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
//!
//! When the user taps the demo's TextField with `Enabled = true` the
//! keyboard auto-raises because TextField overrides
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
        .on_change(|idx| apply_profile_choice(idx));
    col.push(Box::new(radio), 0.0);

    col.push(
        Box::new(
            Label::new(
                "Picking iPhone or Android flips the global agg-gui input \
                 profile and enables the on-screen keyboard. Tap the field \
                 below — the keyboard slides up and types into it.",
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
        Box::new(Label::new("Type here (mode follows radio)", Arc::clone(&font)).with_font_size(12.0)),
        0.0,
    );
    col.push(
        Box::new(
            SizedBox::new().with_height(34.0).with_child(Box::new(
                TextField::new(Arc::clone(&font))
                    .with_font_size(14.0)
                    .with_placeholder("Tap to focus, then type / tap keys…")
                    .with_keyboard_mode_cell(Rc::clone(&primary_mode)),
            )),
        ),
        0.0,
    );

    col.push(
        Box::new(Label::new("Numeric-only field (digit pad)", Arc::clone(&font)).with_font_size(12.0)),
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
/// globally. Index `0` (Desktop) also dismisses any currently-raised
/// keyboard so the demo doesn't leave it hanging.
fn apply_profile_choice(idx: usize) {
    let profile = match idx {
        1 => InputProfile::MobileIOS,
        2 => InputProfile::MobileAndroid,
        _ => InputProfile::Desktop,
    };
    set_input_profile(profile);
    let enable = profile.is_mobile_touch();
    on_screen_keyboard::set_enabled(enable);
    if !enable {
        on_screen_keyboard::dismiss();
    }
    agg_gui::animation::request_draw();
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
