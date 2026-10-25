//! Tests for `machine.rs`, the model's load: nothing loads before the first
//! start, which opens the whole session in the tap (the load, the engine's
//! stream, the one microphone request) and listens once the model is in;
//! a stop, cancel or refused microphone while it loads; failed loads and
//! their retries; and an app's opt-in prepare. A child of
//! `machine_tests.rs`, whose helpers it uses.

use agg_gui::widgets::on_screen_keyboard::VoiceState;

use super::*;
use crate::protocol::{FromWorker, LoadFailure, ToWorker};

/// The commands of a start that opens session `s`.
fn opens(s: u32) -> Vec<Command> {
    vec![
        Command::Worker(ToWorker::Start {
            session: s,
            language: String::new(),
        }),
        Command::OpenMic { session: s },
    ]
}

/// The commands of a session `s` ended while it waits for the model.
fn ends(s: u32) -> Vec<Command> {
    vec![
        Command::CloseMic {
            session: s,
            flush: false,
        },
        Command::Worker(ToWorker::Cancel { session: s }),
    ]
}

fn fail(m: &mut Machine, reason: LoadFailure) {
    m.on_worker(FromWorker::LoadFailed {
        reason,
        detail: "test".into(),
    });
}

/// A start before the model is in, its session's microphone open; the
/// machine and the session.
fn waiting() -> (Machine, u32) {
    let mut m = machine();
    m.start(&ctx(true));
    let s = m.current_session().expect("a session from the tap");
    m.take_commands();
    m.on_mic_opened(s);
    state(&mut m);
    (m, s)
}

/// Nothing loads before the first tap, and the key is ready for it.
#[test]
fn before_any_start_it_reports_ready_and_asks_for_nothing() {
    let mut m = machine();
    assert_eq!(state(&mut m), VoiceState::Ready);
    assert!(m.take_commands().is_empty());
}

/// The first tap opens everything at once: the load, then the engine's
/// stream (queued behind it in the engine), then the microphone, so the
/// permission prompt shows at the tap and speech is captured from then on.
/// The key shows the load's progress, then listens with no second tap.
#[test]
fn the_first_start_opens_the_session_in_the_tap_and_listens_once_loaded() {
    let mut m = machine();
    m.start(&ctx(false));
    let s = m.current_session().expect("a session from the tap");
    let mut want = vec![load()];
    want.extend(opens(s));
    assert_eq!(m.take_commands(), want);
    assert_eq!(state(&mut m), VoiceState::Preparing { progress: None });
    m.on_mic_opened(s);
    m.on_level(s, 0.5);
    assert_eq!(
        state(&mut m),
        VoiceState::Preparing { progress: None },
        "capturing, but not listening until the model is in"
    );
    m.on_worker(FromWorker::Progress {
        loaded: 40,
        total: 100,
    });
    assert_eq!(
        state(&mut m),
        VoiceState::Preparing {
            progress: Some(0.4)
        }
    );
    m.on_worker(FromWorker::Loaded {
        source: "https://here/whistle.cact".into(),
    });
    assert!(
        m.take_commands().is_empty(),
        "the stream was already started"
    );
    assert_eq!(state(&mut m), VoiceState::Listening { level: Some(0.5) });
}

/// A stop or cancel while the model loads ends the session: the microphone
/// closes without its last chunk and the engine drops the stream and audio
/// queued behind the load. The load goes on, nothing listens when it is in,
/// and the next start begins at once. A microphone that opens after the
/// cancel is released.
#[test]
fn a_stop_or_cancel_while_the_model_loads_ends_the_session_but_not_the_load() {
    let (mut m, s) = waiting();
    m.stop();
    assert_eq!(
        m.take_commands(),
        ends(s),
        "no finish: nothing was recognised"
    );
    assert_eq!(state(&mut m), VoiceState::Ready, "the cancel shows at once");
    m.on_worker(FromWorker::Loaded {
        source: "cache".into(),
    });
    assert!(m.take_commands().is_empty(), "nothing listens");
    assert_eq!(state(&mut m), VoiceState::Ready);
    m.start(&ctx(false));
    let next = m.current_session().expect("a session");
    assert_ne!(next, s);
    assert_eq!(m.take_commands(), opens(next), "loaded: no second download");

    let mut m = machine();
    m.start(&ctx(false));
    let s = m.current_session().expect("a session");
    m.take_commands();
    m.cancel();
    assert_eq!(m.take_commands(), ends(s));
    m.on_mic_opened(s);
    assert_eq!(
        m.take_commands(),
        vec![Command::CloseMic {
            session: s,
            flush: false
        }],
        "a late open is released"
    );
    assert_eq!(state(&mut m), VoiceState::Ready);
}

/// The one microphone request, refused while the model loads: the session
/// ends with the error, which a tap retries (the browser asked again); the
/// load goes on.
#[test]
fn a_microphone_refused_while_the_model_loads_ends_the_session() {
    let mut m = machine();
    m.start(&ctx(true));
    let s = m.current_session().expect("a session");
    m.take_commands();
    state(&mut m);
    m.on_mic_failed(s, MicError::Denied);
    assert_eq!(
        m.take_commands(),
        vec![Command::Worker(ToWorker::Cancel { session: s })]
    );
    let VoiceState::Error { message, retry } = state(&mut m) else {
        panic!("not an error");
    };
    assert!(message.contains("denied"), "{message}");
    assert!(retry);
    m.on_worker(FromWorker::Loaded {
        source: "cache".into(),
    });
    assert!(m.take_commands().is_empty(), "nothing opens");
    m.start(&ctx(true));
    let next = m.current_session().expect("a session");
    assert_eq!(m.take_commands(), opens(next), "asked again");
}

/// A load a session waits for that fails ends the session with the error;
/// the next start downloads again and listens once loaded.
#[test]
fn a_failed_first_load_ends_the_session_and_the_next_start_retries_it() {
    let (mut m, s) = waiting();
    fail(&mut m, LoadFailure::Download);
    assert_eq!(m.take_commands(), ends(s));
    let VoiceState::Error { message, retry } = state(&mut m) else {
        panic!("not an error");
    };
    assert!(retry);
    assert!(message.contains("download"), "{message}");

    m.start(&ctx(true));
    let next = m.current_session().expect("a session");
    let mut want = vec![load()];
    want.extend(opens(next));
    assert_eq!(m.take_commands(), want);
    assert_eq!(state(&mut m), VoiceState::Preparing { progress: None });
    m.on_mic_opened(next);
    m.on_level(next, 0.0);
    m.on_worker(FromWorker::Loaded {
        source: "cache".into(),
    });
    assert!(matches!(state(&mut m), VoiceState::Listening { .. }));
}

#[test]
fn a_damaged_download_says_so() {
    let (mut m, _) = waiting();
    fail(&mut m, LoadFailure::Checksum);
    let VoiceState::Error { message, retry } = state(&mut m) else {
        panic!("not an error");
    };
    assert!(retry);
    assert!(message.contains("damaged"), "{message}");
}

/// A load nobody waits for (an app's prepare, or one whose session was
/// cancelled) that fails shows no error: the key stays ready, and the next
/// tap loads afresh, whose failure does show.
#[test]
fn a_load_nobody_waits_for_that_fails_stays_ready_and_the_next_start_loads_afresh() {
    let mut m = machine();
    m.prepare();
    m.take_commands();
    fail(&mut m, LoadFailure::Download);
    assert_eq!(state(&mut m), VoiceState::Ready, "an app's prepare");
    m.start(&ctx(true));
    let s = m.current_session().expect("a session");
    let mut want = vec![load()];
    want.extend(opens(s));
    assert_eq!(m.take_commands(), want, "a fresh load");
    assert_eq!(state(&mut m), VoiceState::Preparing { progress: None });
    fail(&mut m, LoadFailure::Download);
    assert!(
        matches!(state(&mut m), VoiceState::Error { retry: true, .. }),
        "the tap's load failing shows"
    );

    let (mut m, _) = waiting();
    m.cancel();
    m.take_commands();
    fail(&mut m, LoadFailure::Download);
    assert_eq!(state(&mut m), VoiceState::Ready, "a cancelled tap's");
    m.start(&ctx(true));
    assert_eq!(m.take_commands().first(), Some(&load()));

    // The engine lost while loading for nobody: the same.
    let mut m = machine();
    m.prepare();
    m.take_commands();
    m.worker_lost("script error");
    assert_eq!(state(&mut m), VoiceState::Ready);
    m.start(&ctx(true));
    assert_eq!(m.take_commands().first(), Some(&load()));
}

#[test]
fn a_worker_that_dies_while_a_session_waits_is_an_error_and_the_next_start_starts_over() {
    let (mut m, s) = waiting();
    m.worker_lost("script error");
    assert_eq!(
        m.take_commands(),
        vec![Command::CloseMic {
            session: s,
            flush: false
        }],
        "the cancel has nowhere to go"
    );
    assert!(matches!(
        state(&mut m),
        VoiceState::Error { retry: true, .. }
    ));
    m.start(&ctx(true));
    assert_eq!(m.take_commands().first(), Some(&load()));
}

/// An app may warm the model with `prepare` (the keyboard never does). The
/// load shows no progress nobody asked for: the key stays ready, and a
/// start then opens its session behind that same load, showing how far it
/// has got.
#[test]
fn prepare_loads_once_in_the_background_and_a_start_joins_it() {
    let mut m = machine();
    m.prepare();
    assert_eq!(m.take_commands(), vec![load()]);
    m.prepare();
    assert!(m.take_commands().is_empty(), "idempotent while loading");
    m.on_worker(FromWorker::Progress {
        loaded: 25,
        total: 100,
    });
    assert_eq!(state(&mut m), VoiceState::Ready);
    m.start(&ctx(false));
    let s = m.current_session().expect("a session");
    assert_eq!(m.take_commands(), opens(s), "the same load");
    m.on_mic_opened(s);
    m.on_level(s, 0.0);
    assert_eq!(
        state(&mut m),
        VoiceState::Preparing {
            progress: Some(0.25)
        }
    );
    m.on_worker(FromWorker::Loaded {
        source: "https://here/whistle.cact".into(),
    });
    assert!(matches!(state(&mut m), VoiceState::Listening { .. }));
    m.cancel();
    m.take_commands();
    m.prepare();
    assert!(m.take_commands().is_empty(), "idempotent once loaded");
}

/// Natively, a load that fails ends the engine thread, and the microphone's
/// next push then reports the engine lost: the load's error must survive
/// that, for the user to see why and retry.
#[test]
fn an_engine_lost_after_a_failed_load_keeps_the_load_error() {
    let (mut m, _) = waiting();
    fail(&mut m, LoadFailure::Download);
    m.take_commands();
    m.worker_lost("a push found the engine gone");
    assert!(m.take_commands().is_empty());
    let VoiceState::Error { message, retry } = state(&mut m) else {
        panic!("the load's error was erased");
    };
    assert!(message.contains("download"), "{message}");
    assert!(retry);
    m.start(&ctx(true));
    assert_eq!(m.take_commands().first(), Some(&load()), "a tap retries it");
}

/// Loaded, the microphone open, but no audio yet (a starved browser
/// delivering the stream late): still Preparing, with no progress, until
/// the first audio arrives.
#[test]
fn a_loaded_model_still_waits_for_the_first_audio_before_listening() {
    let (mut m, s) = waiting();
    m.on_worker(FromWorker::Loaded {
        source: "cache".into(),
    });
    assert_eq!(state(&mut m), VoiceState::Preparing { progress: None });
    assert!(m.on_level(s, 0.2));
    assert_eq!(state(&mut m), VoiceState::Listening { level: Some(0.2) });
}
