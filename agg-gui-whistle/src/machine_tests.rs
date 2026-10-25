//! Tests for `machine.rs`: what the provider reports and asks its host to do
//! through a dictation's life — starting (the microphone, aborted starts,
//! denied permission), the text, stopping (Finishing, then the final text,
//! then Ready), cancelling, and replies for sessions that are over. The
//! model's load (on the first start, an app's prepare, failures and
//! retries) is in `machine_load_tests.rs`, a child module sharing the
//! helpers here; what a caller other than the keyboard adds (the full
//! pass, the structured errors) is in `machine_dictation_tests.rs`.

use agg_gui::widgets::on_screen_keyboard::{VoiceContext, VoicePoll, VoiceState, VoiceText};

use super::*;
use crate::protocol::{FromWorker, Recognised, ToWorker};

const URLS: [&str; 2] = ["https://here/whistle.cact", "https://there/whistle.cact"];

#[path = "machine_load_tests.rs"]
mod load;

#[path = "machine_dictation_tests.rs"]
mod dictation;

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
        pending_continues: false,
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
    // The first audio: only now is it listening.
    m.on_level(s, 0.0);
    assert!(m.take_commands().is_empty());
    assert!(matches!(state(&mut m), VoiceState::Listening { .. }));
    (m, s)
}

#[test]
fn start_opens_the_stream_before_the_microphone_and_listens_once_it_opens() {
    let mut m = ready();
    m.start(&ctx(true));
    let cmds = m.take_commands();
    let [Command::Worker(ToWorker::Start {
        session, language, ..
    }), Command::OpenMic { session: open }] = cmds.as_slice()
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

/// The key says Listening ("Speak now") only once audio flows: the
/// microphone open and its graph wired is not enough, its first audio must
/// have reached the pipeline (its first level). Until then it says
/// Preparing, so nothing said after the cue is lost, however late a starved
/// browser delivers the stream.
#[test]
fn listening_is_reported_only_once_the_first_audio_arrives() {
    let mut m = ready();
    m.start(&ctx(true));
    let s = m.current_session().expect("a session");
    m.take_commands();
    m.on_mic_opened(s);
    assert_eq!(
        state(&mut m),
        VoiceState::Preparing { progress: None },
        "open, but no audio yet"
    );
    assert!(m.on_level(s, 0.0), "the first audio");
    assert_eq!(state(&mut m), VoiceState::Listening { level: Some(0.0) });
    assert!(!m.on_level(s, 0.3), "not the first any more");
    assert!(!m.on_level(s + 1, 0.3), "another session's is none of its");
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

/// A refused microphone ends the dictation with an error a tap retries:
/// the browser is asked again, so a user who has since allowed the site
/// dictates without reloading (and one who hasn't sees the message again).
#[test]
fn a_denied_microphone_is_an_error_a_tap_retries_and_cancels_the_stream() {
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
    assert!(retry, "a tap asks the browser again");
    assert!(message.contains("denied"), "{message}");
    m.start(&ctx(true));
    let again = m.take_commands();
    assert!(
        matches!(
            again.as_slice(),
            [Command::Worker(ToWorker::Start { .. }), Command::OpenMic { session }] if *session != s
        ),
        "{again:?}"
    );
}

/// A microphone that fails mid-dictation (unplugged, its stream broken)
/// ends it instead of listening on to nothing: like a stop, the speech
/// already captured is recognised and committed, and the error shows once
/// the final text is in.
#[test]
fn a_microphone_that_fails_while_listening_finishes_then_reports_it() {
    let (mut m, s) = listening(true);
    m.on_worker(FromWorker::Update {
        session: s,
        update: rec("make the", false, "cube"),
    });
    m.on_mic_failed(s, MicError::NoDevice);
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
    let poll = m.poll();
    assert_eq!(poll.state, VoiceState::Finishing);
    assert_eq!(
        poll.text,
        Some(VoiceText {
            committed: "make the".into(),
            pending: " cube".into()
        })
    );
    m.on_worker(FromWorker::Final {
        session: s,
        update: rec("cube", false, ""),
    });
    let poll = m.poll();
    assert_eq!(
        poll.state,
        VoiceState::Finishing,
        "the final text goes first"
    );
    assert_eq!(
        poll.text,
        Some(VoiceText {
            committed: " cube".into(),
            pending: String::new()
        })
    );
    // Whatever the microphone reported, a microphone that worked and went
    // away was disconnected ("No microphone found" read as if it never had
    // one).
    assert_eq!(
        m.poll().state,
        VoiceState::Error {
            message: "The microphone was disconnected".into(),
            retry: true
        }
    );
    assert_eq!(m.current_session(), None);
    // A late update for the session is dropped.
    m.on_worker(FromWorker::Update {
        session: s,
        update: rec("wide", false, ""),
    });
    assert_eq!(m.poll().text, None);
}

/// The microphone failing after the stop is nothing new: the dictation is
/// finishing already.
#[test]
fn a_microphone_failure_while_finishing_changes_nothing() {
    let (mut m, s) = listening(true);
    m.stop();
    m.take_commands();
    m.on_mic_failed(s, MicError::NoDevice);
    assert!(m.take_commands().is_empty());
    assert_eq!(m.poll().state, VoiceState::Finishing);
    m.on_worker(FromWorker::Final {
        session: s,
        update: Recognised::default(),
    });
    m.poll();
    assert_eq!(m.poll().state, VoiceState::Ready);
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

/// A platform that can't dictate says why when the user taps the key, not
/// with an error banner on the keyboard's first show. The keyboard's
/// `Unavailable` can't carry a reason (a tap on it does nothing), so the
/// key looks ready until the first tap, and is an error without retry from
/// then on.
#[test]
fn an_unsupported_browser_says_why_from_the_first_tap_and_for_good() {
    let mut m = Machine::unsupported("This browser can't record audio");
    m.prepare();
    for _ in 0..3 {
        assert_eq!(state(&mut m), VoiceState::Ready, "no error before a tap");
    }
    m.start(&ctx(true));
    assert!(m.take_commands().is_empty());
    assert_eq!(m.current_session(), None);
    let error = VoiceState::Error {
        message: "This browser can't record audio".into(),
        retry: false,
    };
    assert_eq!(state(&mut m), error);
    // The keyboard coming up again doesn't hide it.
    m.prepare();
    assert_eq!(state(&mut m), error);
    assert!(m.take_commands().is_empty());
}
