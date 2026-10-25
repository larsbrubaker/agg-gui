//! Tests for `machine.rs` as a caller other than the keyboard drives it
//! ([`crate::Dictation`]): the full pass a dictation can ask for (sent after
//! the finish, waited for, and handed over with the final text), what each
//! poll says went wrong ([`DictationError`]), and a microphone lost
//! mid-dictation, which is known at once while its words are finished. A
//! child module of `machine_tests.rs`, sharing its helpers.

use super::*;
use crate::dictation::{DictationError, DictationOptions, FullPass};
use crate::protocol::{FromWorker, ToWorker};

fn with_full_pass() -> DictationOptions {
    DictationOptions {
        sentence_start: true,
        full_pass: true,
        ..DictationOptions::default()
    }
}

/// A ready machine listening in a full-pass session `s` (returned).
fn listening_with_full_pass() -> (Machine, u32) {
    let mut m = ready();
    m.start(with_full_pass());
    let s = match m.take_commands().as_slice() {
        [Command::Worker(ToWorker::Start {
            session,
            full_pass: true,
            ..
        }), Command::OpenMic { session: open }] => {
            assert_eq!(session, open);
            *session
        }
        other => panic!("unexpected commands {other:?}"),
    };
    m.on_mic_opened(s);
    m.on_level(s, 0.2);
    assert!(matches!(state(&mut m), VoiceState::Listening { .. }));
    (m, s)
}

/// The keyboard's dictation asks for no full pass: its start says so and
/// its stop sends none.
#[test]
fn the_keyboards_dictation_asks_for_no_full_pass() {
    let (mut m, s) = listening(true);
    m.stop();
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
    m.on_worker(FromWorker::Final {
        session: s,
        update: rec("done", false, ""),
    });
    let poll = m.poll_dictation();
    assert!(poll.finished, "the final text ends it");
    assert_eq!(poll.full_pass, None);
}

/// A full-pass dictation's stop asks for the pass after the finish, and
/// the dictation is finishing until both are in: the stream's final text
/// first (it shows at once), then the pass, handed over with the poll that
/// ends the dictation.
#[test]
fn a_full_pass_follows_the_finish_and_ends_the_dictation() {
    let (mut m, s) = listening_with_full_pass();
    m.on_worker(FromWorker::Update {
        session: s,
        update: rec("make the cube", false, "twenty"),
    });
    m.stop();
    assert_eq!(
        m.take_commands(),
        vec![
            Command::CloseMic {
                session: s,
                flush: true
            },
            Command::Worker(ToWorker::Finish { session: s }),
            Command::Worker(ToWorker::FullPass { session: s }),
        ]
    );
    m.on_worker(FromWorker::Final {
        session: s,
        update: rec("20 mm taller", false, ""),
    });
    let poll = m.poll_dictation();
    assert_eq!(poll.state, VoiceState::Finishing, "the pass still runs");
    assert!(!poll.finished);
    assert_eq!(
        poll.text,
        Some(VoiceText {
            committed: "make the cube 20 mm taller".into(),
            pending: String::new(),
        }),
        "the stream's final text shows while the pass runs"
    );
    assert_eq!(poll.full_pass, None);
    assert_eq!(m.poll_dictation().text, None);

    m.on_worker(FromWorker::FullPass {
        session: s,
        text: "Make the cube 20 mm taller.".into(),
        repeated: false,
    });
    let poll = m.poll_dictation();
    assert_eq!(poll.state, VoiceState::Finishing);
    assert!(poll.finished);
    assert_eq!(
        poll.full_pass,
        Some(FullPass::Read {
            text: "Make the cube 20 mm taller.".into(),
            repeated: false,
        })
    );
    assert_eq!(poll.error, None);
    let after = m.poll_dictation();
    assert_eq!(after.state, VoiceState::Ready);
    assert!(!after.finished);
    assert_eq!(after.full_pass, None, "handed over once");
}

/// A pass that fails says so, and still ends the dictation; one for a
/// session that is over (cancelled, or an earlier one) is dropped.
#[test]
fn a_failed_full_pass_ends_the_dictation_and_stale_passes_are_dropped() {
    let (mut m, s) = listening_with_full_pass();
    m.stop();
    m.on_worker(FromWorker::FullPass {
        session: s + 7,
        text: "someone else's".into(),
        repeated: false,
    });
    m.on_worker(FromWorker::Final {
        session: s,
        update: rec("hello", false, ""),
    });
    assert!(!m.poll_dictation().finished, "a stale pass doesn't count");
    m.on_worker(FromWorker::FullPassFailed {
        session: s,
        detail: "decode failed".into(),
    });
    let poll = m.poll_dictation();
    assert!(poll.finished);
    assert_eq!(
        poll.full_pass,
        Some(FullPass::Failed {
            detail: "decode failed".into()
        })
    );

    let (mut m, s) = listening_with_full_pass();
    m.stop();
    m.take_commands();
    m.cancel();
    assert_eq!(
        m.take_commands(),
        vec![Command::Worker(ToWorker::Cancel { session: s })]
    );
    m.on_worker(FromWorker::Final {
        session: s,
        update: rec("late", false, ""),
    });
    m.on_worker(FromWorker::FullPass {
        session: s,
        text: "late".into(),
        repeated: false,
    });
    let poll = m.poll_dictation();
    assert_eq!(poll.state, VoiceState::Ready);
    assert_eq!(
        (poll.text, poll.full_pass, poll.finished),
        (None, None, false)
    );
}

/// The pass may land before the stream's final reply would have been
/// polled: one poll then hands over both.
#[test]
fn the_final_text_and_the_pass_can_arrive_between_two_polls() {
    let (mut m, s) = listening_with_full_pass();
    m.stop();
    m.on_worker(FromWorker::Final {
        session: s,
        update: rec("one", false, "two"),
    });
    m.on_worker(FromWorker::FullPass {
        session: s,
        text: "One, two.".into(),
        repeated: true,
    });
    let poll = m.poll_dictation();
    assert!(poll.finished);
    assert_eq!(
        poll.text,
        Some(VoiceText {
            committed: "one two".into(),
            pending: String::new()
        })
    );
    assert_eq!(
        poll.full_pass,
        Some(FullPass::Read {
            text: "One, two.".into(),
            repeated: true
        })
    );
}

/// A microphone lost mid-dictation (unplugged, switched, its track ended)
/// is known at once: the poll says `MicLost` while the words heard are
/// finished (and read by the full pass), and the dictation ends with them;
/// the keyboard's message says the microphone was disconnected.
#[test]
fn a_microphone_lost_mid_dictation_says_so_at_once_and_keeps_the_words() {
    let (mut m, s) = listening_with_full_pass();
    m.on_worker(FromWorker::Update {
        session: s,
        update: rec("make the", false, "cube"),
    });
    m.on_mic_failed(s, MicError::Other("the track ended".into()));
    assert_eq!(
        m.take_commands(),
        vec![
            Command::CloseMic {
                session: s,
                flush: true
            },
            Command::Worker(ToWorker::Finish { session: s }),
            Command::Worker(ToWorker::FullPass { session: s }),
        ]
    );
    let poll = m.poll_dictation();
    assert_eq!(poll.state, VoiceState::Finishing);
    assert_eq!(poll.error, Some(DictationError::MicLost));
    m.on_worker(FromWorker::Final {
        session: s,
        update: rec("cube", false, ""),
    });
    m.on_worker(FromWorker::FullPass {
        session: s,
        text: "Make the cube.".into(),
        repeated: false,
    });
    let poll = m.poll_dictation();
    assert!(poll.finished);
    assert_eq!(poll.error, Some(DictationError::MicLost));
    assert!(poll.full_pass.is_some());
    assert_eq!(
        m.poll_dictation().state,
        VoiceState::Error {
            message: DictationError::MicLost.message().into(),
            retry: true
        }
    );
}

/// Each way a start fails is told apart: a refused microphone, none at
/// all, a browser that can't record, a model that won't download.
#[test]
fn a_failed_start_says_what_went_wrong() {
    for (err, expected) in [
        (MicError::Denied, DictationError::MicDenied),
        (MicError::NoDevice, DictationError::NoMic),
        (MicError::Unsupported, DictationError::MicUnsupported),
        (MicError::Suspended, DictationError::MicSuspended),
        (MicError::Other("boom".into()), DictationError::MicFailed),
    ] {
        let mut m = ready();
        m.start(with_full_pass());
        let s = m.current_session().expect("a session");
        m.poll_dictation();
        m.on_mic_failed(s, err);
        let poll = m.poll_dictation();
        assert_eq!(poll.error, Some(expected.clone()));
        assert_eq!(
            poll.state,
            VoiceState::Error {
                message: expected.message().into(),
                retry: expected.retry()
            }
        );
        assert!(!poll.finished, "a failed start is no finished dictation");
    }

    let mut m = machine();
    m.start(with_full_pass());
    m.poll_dictation();
    m.on_worker(FromWorker::LoadFailed {
        reason: crate::protocol::LoadFailure::Download,
        detail: "offline".into(),
    });
    assert_eq!(
        m.poll_dictation().error,
        Some(DictationError::ModelDownload)
    );
}

/// A platform that can't dictate says why from the first start on, as an
/// error a tap can't fix.
#[test]
fn an_unsupported_platform_says_why_at_the_start() {
    let mut m = Machine::unsupported("This browser can't run speech recognition");
    assert_eq!(m.poll_dictation().error, None, "nothing before a tap");
    m.start(with_full_pass());
    let poll = m.poll_dictation();
    let expected = DictationError::Unsupported("This browser can't run speech recognition".into());
    assert!(!expected.retry());
    assert_eq!(poll.error, Some(expected));
}
