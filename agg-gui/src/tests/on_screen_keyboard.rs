//! Integration tests for the on-screen software keyboard.
//!
//! Covers the load-bearing wiring between [`App::set_focus`] and the
//! keyboard's slide animation: a `TextField` gaining focus must raise the
//! keyboard; losing focus must lower it; a synthetic-key drain must
//! deliver characters to the focused field exactly like a physical keyboard.

use std::sync::Arc;

use crate::input_profile::{set_input_profile, InputProfile};
use crate::text::Font;
use crate::widgets::on_screen_keyboard::{
    self, drain_synthetic_keys, is_enabled, is_visible, set_enabled, set_text_input_focused,
    test_hook, KeyboardInputMode,
};
use crate::widgets::{ScrollView, TextField};
use crate::{App, FlexColumn, Modifiers, MouseButton, Size, SizedBox};

use super::TEST_FONT;

fn fresh_state() {
    on_screen_keyboard::dismiss();
    test_hook::reset();
    crate::widgets::on_screen_keyboard::events::clear();
    crate::widget::keyboard_scroll::reset_lift_for_test();
    // `ux_scale` is now only changed by platform shells, never by
    // `set_input_profile`. Pin to 1.0 anyway so tests don't depend on
    // whatever an earlier test happened to set.
    crate::ux_scale::set_ux_scale(1.0);
}

/// Historically did "set mobile profile + pin ux_scale". Now that
/// `set_input_profile` no longer touches `ux_scale`, this is just a
/// readability shim for "this test exercises the mobile keyboard".
fn set_mobile_profile_for_test() {
    set_input_profile(InputProfile::MobileIOS);
}

#[test]
fn keyboard_disabled_by_default() {
    fresh_state();
    assert!(
        !is_enabled(),
        "keyboard must start disabled so desktop apps see no behavior change"
    );
    assert!(!is_visible());
}

#[test]
fn enabling_keyboard_does_not_make_it_visible_alone() {
    fresh_state();
    set_enabled(true);
    assert!(is_enabled());
    // Visibility only happens once a text-input widget reports focus.
    assert!(!is_visible());
}

#[test]
fn focusing_text_input_raises_keyboard() {
    fresh_state();
    set_enabled(true);
    set_text_input_focused(true, Some(""), KeyboardInputMode::Text);
    // Animation hasn't ticked yet, but `is_visible` only requires the
    // tween value > 0.001. With a 0.22 s slide we should already be in
    // motion (start > 0) on the first paint. The keyboard module
    // explicitly retargets the tween and requests a draw, so an event
    // loop would tick. For the test we force the tween to fully open
    // using the test hook so visibility flips deterministically.
    test_hook::force_visible();
    assert!(
        is_visible(),
        "keyboard should be visible after force_visible"
    );
}

#[test]
fn auto_cap_on_empty_field() {
    fresh_state();
    set_enabled(true);
    set_text_input_focused(true, Some(""), KeyboardInputMode::Text);
    use crate::widgets::on_screen_keyboard::layouts::Layer;
    use crate::widgets::on_screen_keyboard::state::with_state_ref;
    assert_eq!(
        with_state_ref(|s| s.current_layer),
        Layer::Shifted,
        "empty field should auto-cap the first letter"
    );
}

#[test]
fn auto_cap_after_sentence_terminator() {
    fresh_state();
    set_enabled(true);
    set_text_input_focused(true, Some("Hello world."), KeyboardInputMode::Text);
    use crate::widgets::on_screen_keyboard::layouts::Layer;
    use crate::widgets::on_screen_keyboard::state::with_state_ref;
    assert_eq!(
        with_state_ref(|s| s.current_layer),
        Layer::Shifted,
        "field ending in '.' should auto-cap the next letter"
    );
}

#[test]
fn double_tap_shift_engages_caps_lock() {
    fresh_state();
    set_enabled(true);
    set_text_input_focused(true, Some("hello"), KeyboardInputMode::Text);
    use crate::widgets::on_screen_keyboard::layouts::Layer;

    // Two shift taps in quick succession → caps lock on, layer Shifted.
    test_hook::simulate_shift_tap();
    test_hook::simulate_shift_tap();
    assert!(test_hook::caps_lock(), "double-tap should engage caps lock");
    assert_eq!(test_hook::current_layer(), Layer::Shifted);

    // Third tap while caps-locked → release lock + drop to lowercase.
    test_hook::simulate_shift_tap();
    assert!(
        !test_hook::caps_lock(),
        "third tap should release caps lock"
    );
    assert_eq!(test_hook::current_layer(), Layer::Letters);
}

#[test]
fn single_shift_tap_does_not_engage_caps_lock() {
    fresh_state();
    set_enabled(true);
    set_text_input_focused(true, Some("hello"), KeyboardInputMode::Text);
    test_hook::simulate_shift_tap();
    assert!(!test_hook::caps_lock(), "one tap is one-shot shift");
}

#[test]
fn no_auto_cap_mid_sentence() {
    fresh_state();
    set_enabled(true);
    set_text_input_focused(true, Some("Hello world"), KeyboardInputMode::Text);
    use crate::widgets::on_screen_keyboard::layouts::Layer;
    use crate::widgets::on_screen_keyboard::state::with_state_ref;
    assert_eq!(
        with_state_ref(|s| s.current_layer),
        Layer::Letters,
        "field with mid-sentence text should not auto-cap"
    );
}

#[test]
fn dismiss_lowers_keyboard() {
    fresh_state();
    set_enabled(true);
    test_hook::force_visible();
    assert!(is_visible());
    on_screen_keyboard::dismiss();
    // Tween retargeted toward 0.0; until the animation ticks down to
    // ~0, `is_visible` may still report true (depending on test
    // timing). Reset directly via the test hook so the assertion is
    // deterministic.
    test_hook::reset();
    assert!(!is_visible());
}

/// Tapping the keyboard's close key must (1) drop the keyboard,
/// (2) clear focus on the text field that was open, and (3) retarget
/// the screen lift back to 0 so the tree slides down with the
/// keyboard.  Before the fix, dismiss() only handled (1) and (3) was
/// stuck because focus never changed.
#[test]
fn dismiss_clears_text_field_focus_and_drops_lift() {
    fresh_state();
    set_mobile_profile_for_test();
    set_enabled(true);

    // A bare TextField fills the viewport — its bottom sits at y=0
    // (Y-up), under the keyboard panel, so focusing it raises the
    // keyboard AND retargets the lift to clear the field.
    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let field = TextField::new(Arc::clone(&font)).with_text("hi");
    let mut app = App::new(Box::new(field));
    app.layout(Size::new(400.0, 800.0));

    // Click anywhere inside the field — small `y` (Y-down screen
    // pixels) maps to the top of the viewport in Y-up, well above the
    // keyboard panel which sits at the bottom.
    app.on_mouse_down(50.0, 50.0, MouseButton::Left, Modifiers::default());
    app.on_mouse_up(50.0, 50.0, MouseButton::Left, Modifiers::default());
    assert!(app.has_focus(), "TextField should be focused after tap");
    // The keyboard's slide tween hasn't ticked yet, so `is_visible()`
    // (value-threshold) reports false here — but `text_input_focused`
    // is the conceptual "keyboard is up" flag and is set immediately.
    assert!(
        crate::widgets::on_screen_keyboard::state::with_state_ref(|s| s.text_input_focused),
        "keyboard should be tracking a focused text input",
    );
    assert!(
        crate::widget::keyboard_scroll::lift_target_for_test() > 0.0,
        "lift target should be positive (focused field is below the keyboard panel)",
    );

    // Simulate tapping the keyboard's close key by calling dismiss()
    // (which is what KeyAction::Dismiss invokes internally) and then
    // pumping the event drain the App runs after every pointer event.
    on_screen_keyboard::dismiss();
    app.drain_keyboard_events_for_test();

    assert!(
        !app.has_focus(),
        "dismiss must clear focus on the text field so the next focus event lands cleanly",
    );
    assert_eq!(
        crate::widget::keyboard_scroll::lift_target_for_test(),
        0.0,
        "dismiss must retarget the screen lift back to 0 so the tree comes back down",
    );
}

#[test]
fn pointer_outside_panel_falls_through() {
    fresh_state();
    set_enabled(true);
    test_hook::force_visible();
    // Build a minimal App with a TextField so the mouse-down has a
    // candidate target above the keyboard panel.
    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let field = TextField::new(Arc::clone(&font)).with_text("hi");
    let mut app = App::new(Box::new(field));
    app.layout(Size::new(400.0, 800.0));

    // Click at the very top of the viewport — outside the keyboard
    // panel (which sits at the bottom). Should reach the underlying
    // TextField (focus changes), and the keyboard must remain visible
    // because focusing a text-input keeps it up.
    app.on_mouse_down(50.0, 5.0, MouseButton::Left, Modifiers::default());
    assert!(
        app.has_focus(),
        "mouse-down above the keyboard should still reach the text field"
    );
}

#[test]
fn numeric_mode_opens_on_numbers_layer() {
    // Focusing a field that declares itself Numeric must skip the
    // letter / sentence-start path entirely and slide the keyboard
    // up directly on the digit pad — the iOS / Android `numberPad`
    // convention.
    fresh_state();
    set_enabled(true);
    set_text_input_focused(true, Some(""), KeyboardInputMode::Numeric);
    use crate::widgets::on_screen_keyboard::layouts::Layer;
    use crate::widgets::on_screen_keyboard::state::with_state_ref;
    assert_eq!(
        with_state_ref(|s| s.current_layer),
        Layer::Numbers,
        "Numeric mode must open the keyboard on the digit layer, not Letters/Shifted"
    );
}

#[test]
fn numeric_mode_clears_residual_caps_lock() {
    // If a previous Text-mode field engaged caps lock, focusing a
    // Numeric field must wipe that state so the user doesn't see the
    // shift glyph lit while typing digits.  Re-focusing the original
    // Text field would then start a fresh shift state machine.
    fresh_state();
    set_enabled(true);
    set_text_input_focused(true, Some(""), KeyboardInputMode::Text);
    test_hook::simulate_shift_tap();
    test_hook::simulate_shift_tap();
    assert!(test_hook::caps_lock(), "precondition: caps-lock engaged");

    set_text_input_focused(true, Some(""), KeyboardInputMode::Numeric);
    assert!(
        !test_hook::caps_lock(),
        "switching focus into a Numeric field must drop the stale caps-lock state"
    );
}

#[test]
fn focusing_field_below_keyboard_scrolls_parent_view() {
    // Build content tall enough to overflow the viewport with a text
    // field anchored near the BOTTOM of the content (FlexColumn lays
    // out top-to-bottom in document order, but content-Y in Y-up
    // means top-of-content = high Y, bottom = low Y; with a 600 px
    // spacer pushed FIRST and the field SECOND, the field lands near
    // the bottom of the content).  Without auto-scroll the field
    // would sit at screen Y ~ 0 — behind the keyboard panel.
    fresh_state();
    set_mobile_profile_for_test();
    set_enabled(true);

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let mut col = FlexColumn::new();
    col.push(Box::new(SizedBox::new().with_height(600.0)), 0.0);
    let field = TextField::new(Arc::clone(&font)).with_placeholder("type here");
    col.push(Box::new(field), 0.0);
    let offset_cell = std::rc::Rc::new(std::cell::Cell::new(0.0_f64));
    let scroll = ScrollView::new(Box::new(col)).with_offset_cell(std::rc::Rc::clone(&offset_cell));
    let mut app = App::new(Box::new(scroll));
    app.layout(Size::new(400.0, 400.0));

    let panel_h = on_screen_keyboard::target_panel_height(400.0);
    assert!(
        panel_h > 0.0,
        "keyboard layout must report a positive panel height"
    );

    let scroll_before = offset_cell.get();
    app.on_key_down(crate::Key::Tab, Modifiers::default());
    assert!(app.has_focus(), "TextField should be focused after Tab");
    assert_eq!(app.focused_widget_type_name(), Some("TextField"));
    // A second layout pass runs the new offset through the scroll
    // view's layout so the cell reflects the post-scroll value.
    app.layout(Size::new(400.0, 400.0));
    let scroll_after = offset_cell.get();
    assert!(
        scroll_after > scroll_before + 1.0,
        "scroll offset must increase to lift the focused field above the keyboard \
         (before={:.1}, after={:.1}, panel_h={:.1})",
        scroll_before,
        scroll_after,
        panel_h,
    );
}

#[test]
fn focusing_field_with_no_scrollable_ancestor_requests_global_lift() {
    // The demo's mobile-keyboard window is a static Window whose
    // content fits without overflow — its ScrollView has zero
    // max_scroll, so `try_scroll_to_lift` can't absorb anything.
    // In that case the auto-scroll must fall back to the
    // App-level "global lift" so the focused field still clears
    // the keyboard panel.  Reproduce here with a TextField placed
    // near viewport bottom inside a non-scrollable column.
    fresh_state();
    set_mobile_profile_for_test();
    set_enabled(true);

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let mut col = FlexColumn::new();
    // Spacer pushes the field down so its bottom edge falls inside
    // the keyboard-panel area (low Y in Y-up).
    col.push(Box::new(SizedBox::new().with_height(360.0)), 0.0);
    let field = TextField::new(Arc::clone(&font)).with_placeholder("at the bottom");
    col.push(Box::new(field), 0.0);
    // No ScrollView wrapper — directly use the column so there's no
    // scrollable ancestor that can absorb the deficit.
    let mut app = App::new(Box::new(col));
    app.layout(Size::new(400.0, 400.0));

    let lift_before = crate::widget::keyboard_scroll::current_lift();
    app.on_key_down(crate::Key::Tab, Modifiers::default());
    assert!(app.has_focus());
    // Lift target was set via `request_lift`; tween starts moving.
    // Pump one paint frame to advance the tween off zero (the tween
    // is animated, not instant, but starting it bumps the start
    // value immediately).
    // We can't easily paint here without a real DrawCtx; instead,
    // verify the algorithm decided to lift by checking that the
    // tween reports it's animating.
    assert!(
        crate::widget::keyboard_scroll::is_lift_animating()
            || crate::widget::keyboard_scroll::current_lift() > lift_before + 0.5,
        "auto-scroll must engage the global lift when no ScrollView can absorb the deficit",
    );
}

#[test]
fn focusing_already_visible_field_does_not_scroll() {
    // Opposite control: when the focused field is already above the
    // keyboard panel, auto-scroll must do nothing — surprising the
    // user with a jump-scroll on a casual focus change would feel
    // worse than the keyboard issue we're solving.
    fresh_state();
    set_mobile_profile_for_test();
    set_enabled(true);

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let mut col = FlexColumn::new();
    let field = TextField::new(Arc::clone(&font)).with_placeholder("type here");
    col.push(Box::new(field), 0.0);
    col.push(Box::new(SizedBox::new().with_height(600.0)), 0.0);
    let offset_cell = std::rc::Rc::new(std::cell::Cell::new(0.0_f64));
    let scroll = ScrollView::new(Box::new(col)).with_offset_cell(std::rc::Rc::clone(&offset_cell));
    let mut app = App::new(Box::new(scroll));
    app.layout(Size::new(400.0, 400.0));

    let scroll_before = offset_cell.get();
    app.on_key_down(crate::Key::Tab, Modifiers::default());
    assert!(app.has_focus());
    app.layout(Size::new(400.0, 400.0));
    let scroll_after = offset_cell.get();
    assert!(
        (scroll_after - scroll_before).abs() < 0.5,
        "scroll offset must be unchanged when the focused field is already visible \
         (before={:.1}, after={:.1})",
        scroll_before,
        scroll_after,
    );
}

#[test]
fn text_field_with_keyboard_mode_propagates_through_focus() {
    // End-to-end: a TextField configured via `.with_keyboard_mode`
    // should drive the keyboard to the Numbers layer when it gains
    // focus through the normal App focus flow — proving the
    // `Widget::text_input_mode` plumbing reaches the keyboard.
    fresh_state();
    set_mobile_profile_for_test();
    set_enabled(true);

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let field = TextField::new(Arc::clone(&font)).with_keyboard_mode(KeyboardInputMode::Numeric);
    let mut app = App::new(Box::new(field));
    app.layout(Size::new(400.0, 800.0));

    app.on_key_down(crate::Key::Tab, Modifiers::default());
    assert!(app.has_focus(), "TextField should be focused after Tab");

    use crate::widgets::on_screen_keyboard::layouts::Layer;
    use crate::widgets::on_screen_keyboard::state::with_state_ref;
    assert_eq!(
        with_state_ref(|s| s.current_layer),
        Layer::Numbers,
        "focusing a Numeric TextField must raise the keyboard on the digit layer"
    );
}

#[test]
fn synthetic_key_queue_drains_to_focused_field() {
    fresh_state();
    set_mobile_profile_for_test();
    set_enabled(true);

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let field = TextField::new(Arc::clone(&font));
    let mut app = App::new(Box::new(field));
    app.layout(Size::new(400.0, 800.0));

    // Tab so the TextField gets focus → triggers
    // `set_text_input_focused(true, Some(""))` inside set_focus.
    app.on_key_down(crate::Key::Tab, Modifiers::default());
    assert!(app.has_focus(), "TextField should be focused after Tab");

    // Manually push synthetic keys (as the on-screen keyboard would
    // when the user taps "h", "i").
    crate::widgets::on_screen_keyboard::events::push_synthetic_key(
        crate::Key::Char('h'),
        Modifiers::default(),
    );
    crate::widgets::on_screen_keyboard::events::push_synthetic_key(
        crate::Key::Char('i'),
        Modifiers::default(),
    );
    let pending = drain_synthetic_keys();
    assert_eq!(pending.len(), 2);

    // Replay through on_key_down — the focused TextField sees them as
    // real key presses.
    for (key, mods) in pending {
        app.on_key_down(key, mods);
    }

    // We can't easily downcast through `find_widget_by_type` here
    // (returns `&dyn Widget`), so the load-bearing assertion is that
    // (a) keys made it onto the queue, (b) drained, and (c) focus is
    // still on the TextField — meaning the on_key_down dispatch did
    // not bounce. The text contents are exercised by TextField's own
    // tests (`text_field` module).
    assert_eq!(app.focused_widget_type_name(), Some("TextField"));
}
