//! Tests for `machine.rs`: what the provider reports and asks its host to do
//! through a dictation's life — preparing (progress, failure, retry),
//! starting (the microphone, aborted starts, denied permission), the text,
//! stopping (Finishing, then the final text, then Ready), cancelling, and
//! replies for sessions that are over.

use agg_gui::widgets::on_screen_keyboard::{VoiceContext, VoicePoll, VoiceState, VoiceText};

use super::*;
use crate::protocol::{FromWorker, LoadFailure, Recognised, ToWorker};

const URLS: [&str; 2] = ["https://here/whistle.cact", "https://there/whistle.cact"];

fn machine() -> Machine {
    Machine::new(URLS.iter().map(|u| u.to_string()).collect(), None)
}

fn ctx(sentence_start: bool) -> VoiceContext {
    let mut ctx = VoiceContext::default();
    ctx.sentence_start = sentence_start;
    ctx
}

fn rec(text: &str, continues: bool, pending: &str) -> Recognised {
    Recognised {
        text: text.into(),
        continues,
        pending: pending.into(),
    }
}

fn state(m: &mut Machine) -> VoiceState {
    m.poll().state
}

fn load() -> Command {
    Command::Worker(ToWorker::Load {
        urls: URLS.iter().map(|u| u.to_string()).collect(),
    })
}

/// A machine with the model loaded and its commands drained.
fn ready() -> Machine {
    let mut m = machine();
    m.prepare();
    m.on_worker(FromWorker::Loaded {
        source: "cache".into(),
    });
    m.take_commands();
    assert_eq!(state(&mut m), VoiceState::Ready);
    m
}

/// A ready machine listening in session `s` (returned).
fn listening(sentence_start: bool) -> (Machine, u32) {
    let mut m = ready();
    m.start(&ctx(sentence_start));
    let s = match m.take_commands().as_slice() {
        [Command::Worker(ToWorker::Start { session, .. }), Command::OpenMic { session: open }] => {
            assert_eq!(session, open);
            *session
        }
        other => panic!("unexpected commands {other:?}"),
    };
    m.on_mic_opened(s);
    assert!(m.take_commands().is_empty());
    assert!(matches!(state(&mut m), VoiceState::Listening { .. }));
    (m, s)
}

#[test]
fn before_prepare_it_reports_preparing_and_asks_for_nothing() {
    let mut m = machine();
    assert_eq!(state(&mut m), VoiceState::Preparing { progress: None });
    assert!(m.take_commands().is_empty());
}

#[test]
fn prepare_loads_once_with_progress_then_is_ready() {
    let mut m = machine();
    m.prepare();
    assert_eq!(m.take_commands(), vec![load()]);
    m.prepare();
    assert!(m.take_commands().is_empty(), "idempotent while loading");
    m.on_worker(FromWorker::Progress {
        loaded: 25,
        total: 100,
    });
    assert_eq!(
        state(&mut m),
        VoiceState::Preparing {
            progress: Some(0.25)
        }
    );
    m.on_worker(FromWorker::Loaded {
        source: "https://here/whistle.cact".into(),
    });
    assert_eq!(state(&mut m), VoiceState::Ready);
    m.prepare();
    assert!(m.take_commands().is_empty(), "idempotent once loaded");
}

#[test]
fn a_failed_download_is_an_error_to_retry_and_the_next_prepare_retries() {
    let mut m = machine();
    m.prepare();
    m.take_commands();
    m.on_worker(FromWorker::LoadFailed {
        reason: LoadFailure::Download,
        detail: "HTTP 503".into(),
    });
    let VoiceState::Error { message, retry } = state(&mut m) else {
        panic!("not an error");
    };
    assert!(retry);
    assert!(message.contains("download"), "{message}");
    m.prepare();
    assert_eq!(m.take_commands(), vec![load()]);
    assert_eq!(state(&mut m), VoiceState::Preparing { progress: None });
}

#[test]
fn a_damaged_download_says_so() {
    let mut m = machine();
    m.prepare();
    m.on_worker(FromWorker::LoadFailed {
        reason: LoadFailure::Checksum,
        detail: "sha differs".into(),
    });
    let VoiceState::Error { message, retry } = state(&mut m) else {
        panic!("not an error");
    };
    assert!(retry);
    assert!(message.contains("damaged"), "{message}");
}

#[test]
fn a_worker_that_dies_is_an_error_and_the_next_prepare_starts_over() {
    let mut m = machine();
    m.prepare();
    m.take_commands();
    m.worker_lost("script error");
    assert!(matches!(
        state(&mut m),
        VoiceState::Error { retry: true, .. }
    ));
    m.prepare();
    assert_eq!(m.take_commands(), vec![load()]);
}

#[test]
fn start_after_a_failed_load_retries_and_listens_once_loaded() {
    let mut m = machine();
    m.prepare();
    m.take_commands();
    m.on_worker(FromWorker::LoadFailed {
        reason: LoadFailure::Download,
        detail: String::new(),
    });
    state(&mut m);
    m.start(&ctx(true));
    assert_eq!(m.take_commands(), vec![load()]);
    assert_eq!(state(&mut m), VoiceState::Preparing { progress: None });
    m.on_worker(FromWorker::Loaded {
        source: "cache".into(),
    });
    assert!(matches!(
        m.take_commands().as_slice(),
        [
            Command::Worker(ToWorker::Start { .. }),
            Command::OpenMic { .. }
        ]
    ));
}

#[test]
fn start_opens_the_stream_before_the_microphone_and_listens_once_it_opens() {
    let mut m = ready();
    m.start(&ctx(true));
    let cmds = m.take_commands();
    let [Command::Worker(ToWorker::Start { session, language }), Command::OpenMic { session: open }] =
        cmds.as_slice()
    else {
        panic!("unexpected commands {cmds:?}");
    };
    assert_eq!(session, open);
    assert_eq!(language, "");
    // Waiting for the microphone (a permission prompt, perhaps).
    assert_eq!(state(&mut m), VoiceState::Preparing { progress: None });
    m.on_mic_opened(*session);
    m.on_level(*session, 0.4);
    assert_eq!(state(&mut m), VoiceState::Listening { level: Some(0.4) });
}

#[test]
fn the_language_comes_from_the_keyboard_else_the_config() {
    let mut m = Machine::new(vec![], Some("de".into()));
    m.on_worker(FromWorker::Loaded {
        source: "cache".into(),
    });
    m.start(&VoiceContext::default());
    assert!(matches!(
        m.take_commands().first(),
        Some(Command::Worker(ToWorker::Start { language, .. })) if language == "de"
    ));
    m.cancel();
    m.take_commands();
    let mut c = VoiceContext::default();
    c.language = Some("fr".into());
    m.start(&c);
    assert!(matches!(
        m.take_commands().first(),
        Some(Command::Worker(ToWorker::Start { language, .. })) if language == "fr"
    ));
}

#[test]
fn stop_while_the_microphone_opens_aborts_and_a_late_open_is_released() {
    let mut m = ready();
    m.start(&ctx(true));
    let s = m.current_session().expect("a session");
    m.take_commands();
    m.stop();
    assert_eq!(
        m.take_commands(),
        vec![
            Command::CloseMic {
                session: s,
                flush: false
            },
            Command::Worker(ToWorker::Cancel { session: s }),
        ]
    );
    assert_eq!(state(&mut m), VoiceState::Ready);
    // The permission prompt resolves after the stop: release, don't listen.
    m.on_mic_opened(s);
    assert_eq!(
        m.take_commands(),
        vec![Command::CloseMic {
            session: s,
            flush: false
        }]
    );
    assert_eq!(state(&mut m), VoiceState::Ready);
}

#[test]
fn cancel_while_the_microphone_opens_aborts_too() {
    let mut m = ready();
    m.start(&ctx(true));
    let s = m.current_session().expect("a session");
    m.take_commands();
    m.cancel();
    assert_eq!(
        m.take_commands(),
        vec![
            Command::CloseMic {
                session: s,
                flush: false
            },
            Command::Worker(ToWorker::Cancel { session: s }),
        ]
    );
    m.on_mic_opened(s);
    assert!(matches!(
        m.take_commands().as_slice(),
        [Command::CloseMic { flush: false, .. }]
    ));
    assert_eq!(state(&mut m), VoiceState::Ready);
}

#[test]
fn a_denied_microphone_is_an_error_without_retry_and_cancels_the_stream() {
    let mut m = ready();
    m.start(&ctx(true));
    let s = m.current_session().expect("a session");
    m.take_commands();
    assert_eq!(state(&mut m), VoiceState::Preparing { progress: None });
    m.on_mic_failed(s, MicError::Denied);
    assert_eq!(
        m.take_commands(),
        vec![Command::Worker(ToWorker::Cancel { session: s })]
    );
    let VoiceState::Error { message, retry } = state(&mut m) else {
        panic!("not an error");
    };
    assert!(!retry);
    assert!(message.contains("denied"), "{message}");
    // Reopening the keyboard clears it, so the user can try again.
    m.prepare();
    assert_eq!(state(&mut m), VoiceState::Ready);
}

#[test]
fn a_retry_that_fails_at_once_reports_another_state_first() {
    let mut m = ready();
    m.start(&ctx(true));
    let s = m.current_session().expect("a session");
    assert_eq!(state(&mut m), VoiceState::Preparing { progress: None });
    m.on_mic_failed(s, MicError::Other("busy".into()));
    assert!(matches!(state(&mut m), VoiceState::Error { .. }));
    m.take_commands();
    // Retry; the failure lands before the keyboard polls.
    m.start(&ctx(true));
    let s = m.current_session().expect("a session");
    m.on_mic_failed(s, MicError::Other("busy".into()));
    assert_eq!(state(&mut m), VoiceState::Preparing { progress: None });
    assert!(matches!(
        state(&mut m),
        VoiceState::Error { retry: true, .. }
    ));
}

#[test]
fn updates_reach_the_keyboard_spaced_and_accumulated_per_poll() {
    let (mut m, s) = listening(true);
    m.on_worker(FromWorker::Update {
        session: s,
        update: rec("Hello", false, "wor"),
    });
    m.on_worker(FromWorker::Update {
        session: s,
        update: rec("world", false, "how are"),
    });
    assert_eq!(
        m.poll().text,
        Some(VoiceText {
            committed: "Hello world".into(),
            pending: " how are".into(),
        })
    );
    assert_eq!(m.poll().text, None, "nothing new");
}

#[test]
fn mid_sentence_the_first_word_is_lowercased() {
    let (mut m, s) = listening(false);
    m.on_worker(FromWorker::Update {
        session: s,
        update: rec("Then we left", false, ""),
    });
    assert_eq!(
        m.poll().text.map(|t| t.committed),
        Some("then we left".into())
    );
}

#[test]
fn stop_reports_finishing_then_the_final_text_then_ready() {
    let (mut m, s) = listening(true);
    m.on_worker(FromWorker::Update {
        session: s,
        update: rec("Make the cube", false, "twenty"),
    });
    m.stop();
    // The microphone flushes its last chunk before the finish is sent.
    assert_eq!(
        m.take_commands(),
        vec![
            Command::CloseMic {
                session: s,
                flush: true
            },
            Command::Worker(ToWorker::Finish { session: s }),
        ]
    );
    assert_eq!(
        m.poll(),
        VoicePoll {
            state: VoiceState::Finishing,
            text: Some(VoiceText {
                committed: "Make the cube".into(),
                pending: " twenty".into(),
            }),
        }
    );
    // A pass that was in flight when the stop came still counts.
    m.on_worker(FromWorker::Update {
        session: s,
        update: rec("", false, "twenty mm"),
    });
    m.on_worker(FromWorker::Final {
        session: s,
        update: rec("20 mm wide.", false, ""),
    });
    assert_eq!(
        m.poll(),
        VoicePoll {
            state: VoiceState::Finishing,
            text: Some(VoiceText {
                committed: " 20 mm wide.".into(),
                pending: String::new(),
            }),
        }
    );
    assert_eq!(
        m.poll(),
        VoicePoll {
            state: VoiceState::Ready,
            text: None
        }
    );
}

#[test]
fn a_final_with_a_tail_commits_it() {
    let (mut m, s) = listening(true);
    m.stop();
    m.on_worker(FromWorker::Final {
        session: s,
        update: rec("one", false, "two"),
    });
    assert_eq!(
        m.poll().text,
        Some(VoiceText {
            committed: "one two".into(),
            pending: String::new(),
        })
    );
    assert_eq!(state(&mut m), VoiceState::Ready);
}

#[test]
fn cancel_drops_undelivered_text_and_late_replies() {
    let (mut m, s) = listening(true);
    m.on_worker(FromWorker::Update {
        session: s,
        update: rec("never shown", false, "nor this"),
    });
    m.cancel();
    assert_eq!(
        m.take_commands(),
        vec![
            Command::CloseMic {
                session: s,
                flush: false
            },
            Command::Worker(ToWorker::Cancel { session: s }),
        ]
    );
    assert_eq!(
        m.poll(),
        VoicePoll {
            state: VoiceState::Ready,
            text: None
        }
    );
    m.on_worker(FromWorker::Update {
        session: s,
        update: rec("late", false, ""),
    });
    m.on_worker(FromWorker::Final {
        session: s,
        update: rec("later", false, ""),
    });
    assert_eq!(m.poll().text, None);
}

#[test]
fn cancel_while_finishing_drops_the_final_text() {
    let (mut m, s) = listening(true);
    m.stop();
    m.take_commands();
    m.cancel();
    assert_eq!(
        m.take_commands(),
        vec![Command::Worker(ToWorker::Cancel { session: s })]
    );
    m.on_worker(FromWorker::Final {
        session: s,
        update: rec("too late", false, ""),
    });
    assert_eq!(
        m.poll(),
        VoicePoll {
            state: VoiceState::Ready,
            text: None
        }
    );
}

#[test]
fn a_new_session_ignores_the_old_ones_replies() {
    let (mut m, old) = listening(true);
    m.cancel();
    m.start(&ctx(true));
    let new = m.current_session().expect("a session");
    assert_ne!(old, new);
    m.on_mic_opened(new);
    m.on_worker(FromWorker::Update {
        session: old,
        update: rec("stale", false, ""),
    });
    m.on_worker(FromWorker::Update {
        session: new,
        update: rec("fresh", false, ""),
    });
    assert_eq!(m.poll().text.map(|t| t.committed), Some("fresh".into()));
}

#[test]
fn an_engine_failure_ends_the_dictation_with_an_error() {
    let (mut m, s) = listening(true);
    m.on_worker(FromWorker::Failed {
        session: s,
        detail: "non-finite sample".into(),
    });
    assert_eq!(
        m.take_commands(),
        vec![Command::CloseMic {
            session: s,
            flush: false
        }]
    );
    assert!(matches!(
        state(&mut m),
        VoiceState::Error { retry: true, .. }
    ));
}

#[test]
fn an_unsupported_browser_is_an_error_without_retry_for_good() {
    let mut m = Machine::unsupported("This browser can't record audio");
    m.prepare();
    m.start(&ctx(true));
    assert!(m.take_commands().is_empty());
    assert_eq!(
        state(&mut m),
        VoiceState::Error {
            message: "This browser can't record audio".into(),
            retry: false
        }
    );
}
