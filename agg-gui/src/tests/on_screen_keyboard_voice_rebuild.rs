//! Dictation into a field the app rebuilds in place
//! (`widgets/on_screen_keyboard/voice_session.rs`, `widget/app/voice.rs`).
//!
//! agg-gui apps rebuild a field from its text whenever the text changes (a
//! `Rebuilder` keyed on a text cell, or on a shared `TextEditState`'s
//! epoch), and focus follows the rebuilt field (`widget/app/focus_change.rs`).
//! The dictation must follow it too: every word once, in order, the caret
//! where the dictation left it, the unconfirmed tail shown again in the new
//! field and never left behind as text. A different widget the app puts in
//! the field's place, even an empty one of the same kind, must get none of
//! it.

use std::cell::{Cell, RefCell};
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use crate::input_profile::InputProfile;
use crate::widgets::on_screen_keyboard::{self as osk, VoiceState};
use crate::widgets::text_field_core::TextEditState;
use crate::widgets::{Rebuilder, TextArea, TextField};
use crate::{FlexColumn, Widget};

use super::on_screen_keyboard_touch::font;
use super::on_screen_keyboard_voice::{FakeVoice, Rig};

type Text = Rc<RefCell<String>>;

fn hash_of(text: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}

/// A rebuilder that builds `build()` afresh whenever `version()` moves,
/// counting the builds.
fn rebuilder(
    version: impl Fn() -> u64 + 'static,
    build: impl Fn() -> Box<dyn Widget> + 'static,
) -> (Rebuilder, Rc<Cell<usize>>) {
    let builds = Rc::new(Cell::new(0));
    let counter = Rc::clone(&builds);
    let r = Rebuilder::new(version, move || {
        counter.set(counter.get() + 1);
        build()
    });
    (r, builds)
}

/// A `TextField` bound to `cell`, rebuilt whenever the cell changes. The
/// builder does nothing about the caret: a new field puts it at the start.
fn field_rebuilt_from(cell: &Text) -> (Rebuilder, Rc<Cell<usize>>) {
    let (version, build) = (Rc::clone(cell), Rc::clone(cell));
    rebuilder(
        move || hash_of(&version.borrow()),
        move || Box::new(TextField::new(font()).with_text_cell(Rc::clone(&build))),
    )
}

/// A `TextArea` holding `cell`'s text and writing its edits back, rebuilt
/// whenever the cell changes.
fn area_rebuilt_from_cell(cell: &Text) -> (Rebuilder, Rc<Cell<usize>>) {
    let (version, build) = (Rc::clone(cell), Rc::clone(cell));
    rebuilder(
        move || hash_of(&version.borrow()),
        move || {
            let write = Rc::clone(&build);
            let text = build.borrow().clone();
            Box::new(
                TextArea::new(font())
                    .with_text(text)
                    .on_change(move |t| *write.borrow_mut() = t.to_string()),
            )
        },
    )
}

/// A `TextArea` sharing `state`, rebuilt whenever its epoch moves (which a
/// composition shown in it moves too).
fn area_rebuilt_from_state(state: &Rc<RefCell<TextEditState>>) -> (Rebuilder, Rc<Cell<usize>>) {
    let (version, build) = (Rc::clone(state), Rc::clone(state));
    rebuilder(
        move || version.borrow().epoch,
        move || Box::new(TextArea::new(font()).with_edit_state(Rc::clone(&build))),
    )
}

/// The widget the rebuilder holds now.
fn built(rig: &Rig) -> &dyn Widget {
    rig.app.root().children()[0].as_ref()
}

/// The composition the rebuilt field shows.
fn shown_tail(rig: &Rig) -> Option<String> {
    let any = built(rig).as_any()?;
    if let Some(field) = any.downcast_ref::<TextField>() {
        return field.preedit().map(|p| p.text.clone());
    }
    any.downcast_ref::<TextArea>()
        .and_then(|area| area.preedit().map(|p| p.text.clone()))
}

/// Dictate "Hello world this is" in three updates with tails, then stop,
/// checking after the first that the rebuilt field took over. `text` reads
/// the field's committed text.
fn dictate_through_rebuilds(rig: &mut Rig, voice: &FakeVoice, text: &dyn Fn() -> String) {
    rig.start_dictation();
    voice.say("Hello", " wor");
    rig.paint();
    rig.paint();
    assert!(osk::is_dictating(), "the rebuild doesn't end it");
    assert_eq!(text(), "Hello");
    assert_eq!(
        shown_tail(rig).as_deref(),
        Some(" wor"),
        "the rebuilt field shows the tail"
    );
    voice.say(" world", " th");
    rig.paint();
    rig.paint();
    voice.say(" this", " is");
    rig.paint();
    rig.paint();
    rig.tap_mic();
    rig.paint();
    voice.later(VoiceState::Ready);
    rig.paint();
    rig.paint();
    assert!(!osk::is_dictating());
    assert_eq!(text(), "Hello world this is", "all of it, once, in order");
    assert_eq!(shown_tail(rig), None);
    assert_eq!(voice.count("cancel"), 0, "never cut short");
}

#[test]
fn a_text_field_rebuilt_on_every_change_keeps_taking_the_dictation() {
    let voice = FakeVoice::new(VoiceState::Ready);
    let cell = Text::default();
    let (root, builds) = field_rebuilt_from(&cell);
    let mut rig = Rig::new(Box::new(root), InputProfile::MobileAndroid, Some(&voice));
    let read = Rc::clone(&cell);
    dictate_through_rebuilds(&mut rig, &voice, &move || read.borrow().clone());
    assert!(
        builds.get() >= 4,
        "rebuilt on each change: {}",
        builds.get()
    );
}

#[test]
fn a_text_area_rebuilt_from_a_text_cell_keeps_taking_the_dictation() {
    let voice = FakeVoice::new(VoiceState::Ready);
    let cell = Text::default();
    let (root, builds) = area_rebuilt_from_cell(&cell);
    let mut rig = Rig::new(Box::new(root), InputProfile::MobileAndroid, Some(&voice));
    let read = Rc::clone(&cell);
    dictate_through_rebuilds(&mut rig, &voice, &move || read.borrow().clone());
    assert!(
        builds.get() >= 4,
        "rebuilt on each change: {}",
        builds.get()
    );
}

#[test]
fn a_text_area_rebuilt_from_its_shared_edit_state_keeps_taking_the_dictation() {
    let voice = FakeVoice::new(VoiceState::Ready);
    let state = Rc::new(RefCell::new(TextEditState::default()));
    let (root, builds) = area_rebuilt_from_state(&state);
    let mut rig = Rig::new(Box::new(root), InputProfile::MobileAndroid, Some(&voice));
    // The shared buffer holds the composition while it shows; the field's
    // committed text is the rebuilt area's, which leaves it out.
    let committed = |rig: &Rig| built(rig).text_input_value().unwrap_or_default();
    rig.start_dictation();
    voice.say("Hello", " wor");
    for _ in 0..3 {
        rig.paint();
    }
    assert!(osk::is_dictating());
    assert_eq!(committed(&rig), "Hello");
    assert_eq!(
        state.borrow().text,
        "Hello wor",
        "one tail, not a stale copy"
    );
    assert_eq!(shown_tail(&rig).as_deref(), Some(" wor"));
    voice.say(" world", "");
    for _ in 0..3 {
        rig.paint();
    }
    assert_eq!(state.borrow().text, "Hello world");
    rig.tap_mic();
    rig.paint();
    voice.later(VoiceState::Ready);
    rig.paint();
    rig.paint();
    assert!(!osk::is_dictating());
    assert_eq!(state.borrow().text, "Hello world");
    assert_eq!(voice.count("cancel"), 0);
    assert!(
        builds.get() >= 3,
        "rebuilt on each change: {}",
        builds.get()
    );
}

/// Two fields in a column, the first dictated into; returns the rig and the
/// first field's cell.
fn column_of_fields(voice: &FakeVoice, first: &Text, second: &Text) -> Rig {
    let root = FlexColumn::new()
        .add(Box::new(
            TextField::new(font()).with_text_cell(Rc::clone(first)),
        ))
        .add(Box::new(
            TextField::new(font()).with_text_cell(Rc::clone(second)),
        ));
    Rig::new(Box::new(root), InputProfile::MobileAndroid, Some(voice))
}

#[test]
fn a_field_the_app_replaces_ends_the_dictation_without_its_words() {
    let voice = FakeVoice::new(VoiceState::Ready);
    let (old, fresh, other) = (Text::default(), Text::default(), Text::default());
    let mut rig = column_of_fields(&voice, &old, &other);
    rig.start_dictation();
    voice.say("Hello", " there");
    rig.paint();

    rig.app.root_mut().children_mut()[0] =
        Box::new(TextField::new(font()).with_text_cell(Rc::clone(&fresh)));
    voice.say(" more", " words");
    rig.paint();
    assert!(!osk::is_dictating());
    assert_eq!(voice.count("cancel"), 1);
    assert_eq!(*fresh.borrow(), "", "the replacement gets none of it");
    assert_eq!(*old.borrow(), "Hello");
}

#[test]
fn a_different_empty_field_put_in_an_empty_fields_place_gets_nothing() {
    // Every field of a form starts empty: an empty field of the same kind
    // in the dictated empty field's place is no proof it is the same field.
    let voice = FakeVoice::new(VoiceState::Ready);
    let (old, fresh, other) = (Text::default(), Text::default(), Text::default());
    let mut rig = column_of_fields(&voice, &old, &other);
    rig.start_dictation();
    voice.say("", "Hel");
    rig.paint();

    rig.app.root_mut().children_mut()[0] =
        Box::new(TextField::new(font()).with_text_cell(Rc::clone(&fresh)));
    voice.say("Hello", "");
    rig.paint();
    rig.paint();
    assert!(!osk::is_dictating());
    assert_eq!(*fresh.borrow(), "");
    assert_eq!(shown_tail(&rig), None);
}

#[test]
fn a_widget_of_another_kind_holding_the_same_text_gets_nothing() {
    let voice = FakeVoice::new(VoiceState::Ready);
    let (old, other) = (Text::default(), Text::default());
    let mut rig = column_of_fields(&voice, &old, &other);
    rig.start_dictation();
    voice.say("Hello", "");
    rig.paint();

    let area = TextArea::new(font()).with_text("Hello");
    let state = area.edit_state();
    rig.app.root_mut().children_mut()[0] = Box::new(area);
    voice.say(" world", "");
    rig.paint();
    rig.paint();
    assert!(!osk::is_dictating());
    assert_eq!(state.borrow().text, "Hello");
}
