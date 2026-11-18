//! Dictated text counts as the user's input for
//! [`crate::animation::last_input_at`] when it reaches the focused field, and
//! a frame that polls a dictation with nothing new does not: the text
//! arrives on the keyboard's poll from `App::paint`, not through an `App`
//! input entry point. Driven with the scripted provider and rig from
//! `on_screen_keyboard_voice.rs`, on the virtual clock.

use std::time::Duration;

use crate::animation::last_input_at;
use crate::clock;

use super::on_screen_keyboard_voice::dictation_rig;

#[test]
fn delivered_dictation_is_input_and_an_empty_poll_is_not() {
    let _clock = clock::scoped_virtual(None);
    let (mut rig, voice, cell) = dictation_rig();
    rig.start_dictation();
    let tapped_at = last_input_at();
    assert!(tapped_at.is_some(), "the tap on the microphone is input");

    clock::advance(Duration::from_secs(1));
    rig.paint();
    assert_eq!(
        last_input_at(),
        tapped_at,
        "a frame polling a dictation with nothing new is not input"
    );

    clock::advance(Duration::from_secs(1));
    voice.say("Hello", " wor");
    rig.paint();
    assert_eq!(*cell.borrow(), "Hello", "the text reached the field");
    assert_eq!(
        last_input_at(),
        Some(clock::now()),
        "dictated text reaching the field is input"
    );
}
