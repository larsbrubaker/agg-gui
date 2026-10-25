//! Dictation through the on-screen keyboard's microphone key
//! (`widgets/on_screen_keyboard/voice_session.rs`), driven through the real
//! `App` with the asynchronous scripted provider from
//! `on_screen_keyboard_voice.rs`: a tap starts and stops it; committed text
//! and the pending tail reach a `TextField`, spaced from the text before
//! the caret; stop commits the tail, cancel drops it; typing, moving focus,
//! closing the keyboard or replacing the field ends it; a provider that
//! listens with no dictation is stopped; and the preparing, unavailable and
//! error states behave as the key shows them.

use std::cell::RefCell;
use std::rc::Rc;

use crate::geometry::Point;
use crate::input_profile::InputProfile;
use crate::widgets::on_screen_keyboard::{self as osk, test_hook, KeyAction, VoiceState};
use crate::widgets::TextField;
use crate::{FlexColumn, Key, Modifiers};

use super::on_screen_keyboard_touch::font;
use super::on_screen_keyboard_voice::{
    bound_field, dictation_rig, key_where, mic_key, pixel, FakeVoice, Rig,
};

fn error(message: &str, retry: bool) -> VoiceState {
    VoiceState::Error {
        message: message.to_string(),
        retry,
    }
}

#[test]
fn a_tap_starts_listening_and_another_stops() {
    let (mut rig, voice, _) = dictation_rig();
    let ready = rig.paint();
    rig.tap_mic();
    assert_eq!(voice.calls(), vec!["prepare", "start"]);
    assert!(osk::is_dictating());
    assert!(
        !osk::needs_draw(),
        "no frames while the start is pending (a permission prompt, say)"
    );

    let listening = rig.paint();
    assert!(osk::needs_draw(), "the listening key animates");
    assert_eq!(test_hook::voice_status().as_deref(), Some("Speak now"));
    // The lit key: the rim of its face turns the accent colour.
    let mic = mic_key().unwrap();
    let rim = Point::new(mic.rect.x + 3.0, mic.rect.y + mic.rect.height / 2.0);
    assert_ne!(pixel(&ready, rim), pixel(&listening, rim));

    rig.tap_mic();
    assert_eq!(voice.calls(), vec!["prepare", "start", "stop"]);
    rig.paint();
    assert_eq!(voice.state(), VoiceState::Finishing);
    assert!(osk::is_dictating(), "still finishing");
    voice.later(VoiceState::Ready);
    rig.paint();
    assert!(!osk::is_dictating(), "the provider finished");
    assert!(!osk::needs_draw());
}

#[test]
fn committed_and_pending_text_reach_the_text_field() {
    let (mut rig, voice, cell) = dictation_rig();
    rig.start_dictation();
    voice.say("Hello", " wor");
    rig.paint();
    assert_eq!(*cell.borrow(), "Hello");
    assert_eq!(rig.preedit().as_deref(), Some(" wor"), "the tail shows");

    voice.say("", " world is");
    rig.paint();
    assert_eq!(*cell.borrow(), "Hello");
    assert_eq!(rig.preedit().as_deref(), Some(" world is"));

    // Two updates between polls arrive as one.
    voice.say(" world", " is");
    voice.say(" is", " it");
    rig.paint();
    assert_eq!(*cell.borrow(), "Hello world is");
    assert_eq!(rig.preedit().as_deref(), Some(" it"));
}

#[test]
fn the_first_text_is_spaced_from_a_word_before_the_caret() {
    for (before, after) in [
        ("", "Hello there"),
        ("Say", "Say Hello there"),
        ("Say ", "Say Hello there"),
        ("Done.", "Done. Hello there"),
    ] {
        let voice = FakeVoice::new(VoiceState::Ready);
        let (field, cell) = bound_field(before);
        let mut rig = Rig::new(field, InputProfile::MobileAndroid, Some(&voice));
        rig.app.on_key_down(Key::End, Modifiers::default());
        rig.start_dictation();
        // The tail shows with the space before it.
        voice.say("", "Hello");
        rig.paint();
        let tail = rig.preedit().unwrap_or_default();
        assert_eq!(format!("{before}{tail}"), after.trim_end_matches(" there"));
        // The provider spaces its own updates; the keyboard only the first.
        voice.say("Hello", " there");
        rig.paint();
        voice.say(" there", "");
        rig.paint();
        assert_eq!(*cell.borrow(), after, "{before:?}");
    }
}

#[test]
fn sentence_start_follows_the_caret_where_the_field_capitalises() {
    // A one-line field doesn't capitalise (the keyboard opens in lower
    // case): never a sentence start.
    for before in ["", "Done."] {
        let voice = FakeVoice::new(VoiceState::Ready);
        let (field, _) = bound_field(before);
        let mut rig = Rig::new(field, InputProfile::MobileAndroid, Some(&voice));
        rig.app.on_key_down(Key::End, Modifiers::default());
        rig.start_dictation();
        assert!(!voice.0.borrow().contexts[0].sentence_start, "{before:?}");
    }
    // A text area does: by the text before the caret.
    for (before, start) in [("", true), ("Done.", true), ("Say", false)] {
        let voice = FakeVoice::new(VoiceState::Ready);
        let area = crate::widgets::TextArea::new(font()).with_text(before);
        let mut rig = Rig::new(Box::new(area), InputProfile::MobileAndroid, Some(&voice));
        rig.app.on_key_down(Key::End, Modifiers::default());
        rig.start_dictation();
        assert_eq!(
            voice.0.borrow().contexts[0].sentence_start,
            start,
            "{before:?}"
        );
    }
}

#[test]
fn a_hardware_key_ends_the_dictation_after_its_tail() {
    let (mut rig, voice, cell) = dictation_rig();
    rig.start_dictation();
    voice.say("Hello", " there");
    rig.paint();
    // A modifier alone (Shift held for the next letter) doesn't.
    rig.app
        .on_key_down(Key::Other("Shift".into()), Modifiers::default());
    assert!(osk::is_dictating(), "a modifier alone keeps it going");
    let shift = Modifiers {
        shift: true,
        ..Modifiers::default()
    };
    rig.app.on_key_down(Key::Char('X'), shift);
    assert!(!osk::is_dictating());
    assert_eq!(voice.count("cancel"), 1);
    assert_eq!(*cell.borrow(), "Hello thereX", "the key after the tail");
    assert_eq!(rig.preedit(), None);
}

#[test]
fn text_from_the_platform_ends_the_dictation_after_its_tail() {
    let (mut rig, voice, cell) = dictation_rig();
    rig.start_dictation();
    voice.say("Hello", " there");
    rig.paint();
    assert!(rig.app.on_text_input("!"));
    assert!(!osk::is_dictating());
    assert_eq!(*cell.borrow(), "Hello there!");
}

#[test]
fn stop_commits_the_pending_tail() {
    let (mut rig, voice, cell) = dictation_rig();
    rig.start_dictation();
    voice.say("Hello", " there");
    rig.paint();
    rig.tap_mic();
    rig.paint();
    // The provider finishes without confirming the tail: what the user saw
    // stays.
    voice.later(VoiceState::Ready);
    rig.paint();
    assert_eq!(*cell.borrow(), "Hello there");
    assert_eq!(rig.preedit(), None);
    assert!(!osk::is_dictating());
}

#[test]
fn a_tail_the_provider_commits_on_stop_is_inserted_once() {
    let (mut rig, voice, cell) = dictation_rig();
    rig.start_dictation();
    voice.say("Hello", " there");
    rig.paint();
    rig.tap_mic();
    rig.paint();
    voice.say(" there", "");
    voice.later(VoiceState::Ready);
    rig.paint();
    assert_eq!(*cell.borrow(), "Hello there");
    assert_eq!(rig.preedit(), None);
}

#[test]
fn cancel_discards_the_pending_tail() {
    let (mut rig, voice, cell) = dictation_rig();
    rig.start_dictation();
    voice.say("Hello", " there");
    rig.paint();
    osk::cancel_voice_input();
    rig.paint();
    assert_eq!(*cell.borrow(), "Hello");
    assert_eq!(rig.preedit(), None);
    assert_eq!(voice.count("cancel"), 1);
    assert!(!osk::is_dictating());
}

#[test]
fn text_polled_before_a_start_is_dropped() {
    let (mut rig, voice, cell) = dictation_rig();
    voice.say("stale", " words");
    rig.paint();
    assert_eq!(*cell.borrow(), "");
    rig.start_dictation();
    rig.paint();
    assert_eq!(*cell.borrow(), "");
    assert_eq!(rig.preedit(), None);
}

#[test]
fn typing_a_key_ends_the_dictation_after_its_tail() {
    let (mut rig, voice, cell) = dictation_rig();
    rig.start_dictation();
    voice.say("Hello", " there");
    rig.paint();
    rig.tap(key_where(|a| matches!(a, KeyAction::Char('x'))).rect);
    assert!(!osk::is_dictating());
    assert_eq!(voice.count("cancel"), 1);
    assert_eq!(*cell.borrow(), "Hello therex", "the key after the tail");
    assert_eq!(rig.preedit(), None);
    voice.say(" late", "");
    rig.paint();
    assert_eq!(*cell.borrow(), "Hello therex");
}

#[test]
fn moving_focus_ends_the_dictation_in_its_own_field() {
    let voice = FakeVoice::new(VoiceState::Ready);
    let first = Rc::new(RefCell::new(String::new()));
    let second = Rc::new(RefCell::new(String::new()));
    let root = FlexColumn::new()
        .add(Box::new(
            TextField::new(font()).with_text_cell(Rc::clone(&first)),
        ))
        .add(Box::new(
            TextField::new(font()).with_text_cell(Rc::clone(&second)),
        ));
    let mut rig = Rig::new(Box::new(root), InputProfile::MobileAndroid, Some(&voice));
    rig.start_dictation();
    voice.say("Hello", " there");
    rig.paint();

    rig.app.on_key_down(Key::Tab, Modifiers::default());
    assert!(!osk::is_dictating(), "listening stopped");
    assert_eq!(voice.count("cancel"), 1, "no late words follow focus");
    assert_eq!(*first.borrow(), "Hello there", "the shown tail stays put");

    voice.say(" late", "");
    rig.paint();
    assert_eq!(*second.borrow(), "");
}

#[test]
fn closing_the_keyboard_ends_the_dictation() {
    let (mut rig, voice, cell) = dictation_rig();
    rig.start_dictation();
    voice.say("Hello", " there");
    rig.paint();
    rig.tap(key_where(|a| matches!(a, KeyAction::Dismiss)).rect);
    assert!(!osk::is_dictating());
    assert_eq!(voice.count("cancel"), 1);
    assert_eq!(*cell.borrow(), "Hello there");
}

#[test]
fn removing_the_provider_mid_dictation_cancels_it() {
    let (mut rig, voice, cell) = dictation_rig();
    rig.start_dictation();
    voice.say("Hello", " there");
    rig.paint();
    osk::set_voice_input(None);
    rig.paint();
    assert!(!osk::is_dictating());
    assert_eq!(voice.count("cancel"), 1);
    assert_eq!(*cell.borrow(), "Hello", "committed text stays");
    assert_eq!(rig.preedit(), None, "the tail goes with the provider");
    assert!(mic_key().is_none());
}

#[test]
fn a_second_tap_before_listening_leaves_no_provider_listening() {
    // A racy provider whose start completes even after the stop: on the
    // second tap's own poll (lag 1, while the dictation waits on it), or
    // frames after the dictation ended (lag 3, no dictation at all).
    for lag in [1, 3] {
        let (mut rig, voice, _) = dictation_rig();
        voice.0.borrow_mut().ignores_abort = true;
        voice.0.borrow_mut().lag = lag;
        rig.tap_mic();
        rig.tap_mic();
        for _ in 0..4 {
            rig.paint();
        }
        assert_eq!(
            voice.calls(),
            vec!["prepare", "start", "stop", "cancel"],
            "lag {lag}: the late start is cancelled"
        );
        assert!(!osk::is_dictating(), "lag {lag}");
        assert_eq!(voice.state(), VoiceState::Ready, "lag {lag}");
        assert!(!osk::needs_draw(), "lag {lag}");
    }
}

#[test]
fn a_retry_after_an_error_survives_the_stale_error() {
    let voice = FakeVoice::new(error("No microphone", true));
    let (field, cell) = bound_field("");
    let mut rig = Rig::new(field, InputProfile::MobileAndroid, Some(&voice));
    rig.tap_mic();
    assert_eq!(voice.count("start"), 1);
    // The provider still reports the old error on the tap's own poll.
    assert!(matches!(voice.state(), VoiceState::Error { .. }));
    assert!(osk::is_dictating(), "the stale error doesn't end it");
    rig.paint();
    assert!(matches!(
        osk::voice_state(),
        Some(VoiceState::Listening { .. })
    ));
    voice.say("Hi", "");
    rig.paint();
    assert_eq!(*cell.borrow(), "Hi");
}

#[test]
fn an_error_shows_its_message_for_a_while_and_a_tap_retries() {
    let _clock = crate::clock::scoped_virtual(Some(web_time::Instant::now()));
    let (mut rig, voice, cell) = dictation_rig();
    rig.start_dictation();
    voice.say("Hello", " th");
    rig.paint();
    voice.later(error("Microphone lost", true));
    rig.paint();
    assert!(!osk::is_dictating(), "an error ends the dictation");
    assert_eq!(*cell.borrow(), "Hello th", "keeping what was shown");
    assert_eq!(
        test_hook::voice_status().as_deref(),
        Some("Microphone lost")
    );

    crate::clock::advance(std::time::Duration::from_secs(5));
    rig.paint();
    assert_eq!(test_hook::voice_status(), None, "the message goes away");

    // A tap tries again: the provider reports preparing, then fails again
    // with the same message, which shows again.
    voice.0.borrow_mut().next_start_leads_to = Some(error("Microphone lost", true));
    voice
        .0
        .borrow_mut()
        .queued
        .push_back((0, VoiceState::Preparing { progress: None }));
    rig.tap_mic();
    rig.paint();
    rig.paint();
    assert_eq!(voice.count("start"), 2);
    assert!(!osk::is_dictating());
    assert_eq!(
        test_hook::voice_status().as_deref(),
        Some("Microphone lost")
    );
    // And succeeding, it listens.
    rig.start_dictation();
    assert_eq!(voice.count("start"), 3);
}

#[test]
fn an_error_that_cant_be_retried_only_shows_its_message_again() {
    let _clock = crate::clock::scoped_virtual(Some(web_time::Instant::now()));
    let voice = FakeVoice::new(error("Microphone permission denied", false));
    let (field, _) = bound_field("");
    let mut rig = Rig::new(field, InputProfile::MobileAndroid, Some(&voice));
    crate::clock::advance(std::time::Duration::from_secs(5));
    rig.paint();
    assert_eq!(test_hook::voice_status(), None);
    rig.tap_mic();
    assert_eq!(voice.count("start"), 0);
    assert!(!osk::is_dictating());
    assert_eq!(
        test_hook::voice_status().as_deref(),
        Some("Microphone permission denied")
    );
}

#[test]
fn while_preparing_the_key_shows_progress_and_a_tap_does_nothing() {
    let voice = FakeVoice::new(VoiceState::Preparing {
        progress: Some(0.4),
    });
    let (field, _) = bound_field("");
    let mut rig = Rig::new(field, InputProfile::MobileAndroid, Some(&voice));
    let preparing = rig.paint();
    assert_eq!(
        test_hook::voice_status().as_deref(),
        Some("Preparing voice input\u{2026} 40%")
    );
    rig.tap_mic();
    assert_eq!(voice.count("start"), 0);
    assert!(!osk::is_dictating());

    // The progress ring runs round the rim from twelve o'clock clockwise:
    // drawn at three o'clock (40% is past it), not at nine.
    voice.later(VoiceState::Ready);
    let ready = rig.paint();
    let mic = mic_key().unwrap();
    let mid_y = mic.rect.y + mic.rect.height / 2.0;
    let three = Point::new(mic.rect.x + mic.rect.width - 1.5, mid_y);
    let nine = Point::new(mic.rect.x + 1.5, mid_y);
    assert_ne!(pixel(&preparing, three), pixel(&ready, three));
    assert_eq!(pixel(&preparing, nine), pixel(&ready, nine));
    assert_eq!(test_hook::voice_status(), None);
}

#[test]
fn an_unavailable_provider_shows_a_key_that_does_nothing() {
    let voice = FakeVoice::new(VoiceState::Unavailable);
    let (field, _) = bound_field("");
    let mut rig = Rig::new(field, InputProfile::MobileAndroid, Some(&voice));
    assert!(mic_key().is_some());
    rig.tap_mic();
    rig.paint();
    assert_eq!(voice.count("start"), 0);
    assert!(!osk::is_dictating());
}

#[test]
fn the_keyboard_has_no_toolbar_after_the_provider_is_removed() {
    let (mut rig, _voice, _) = dictation_rig();
    let with = super::on_screen_keyboard_voice::panel_height();
    osk::set_voice_input(None);
    rig.paint();
    assert!(mic_key().is_none());
    let without = super::on_screen_keyboard_voice::panel_height();
    assert!((with - without - 43.0).abs() < 1e-6);
    assert!(!osk::has_voice_input());
}
