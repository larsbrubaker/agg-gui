//! Tests for `engine.rs` on the real model and a real clip, both from the
//! sibling whistle-rust checkout (`scripts/fetch-weights.sh` fetches the
//! model). Without them (CI has no model) each test says so and passes.

use std::path::PathBuf;
use std::sync::OnceLock;

use super::*;
use crate::protocol::{FromWorker, ToWorker};
use crate::text_map::TextMapper;

fn whistle_rust() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../whistle-rust")
}

/// The model, built once for the test binary; `None` without the file.
pub(crate) fn model() -> Option<&'static Whistle> {
    static MODEL: OnceLock<Option<&'static Whistle>> = OnceLock::new();
    *MODEL.get_or_init(|| {
        let bytes = std::fs::read(whistle_rust().join("models/whistle.cact")).ok()?;
        let whistle = Whistle::from_cact(&bytes).expect("whistle.cact builds");
        Some(Box::leak(Box::new(whistle)))
    })
}

/// A test clip as 16 kHz mono samples.
pub(crate) fn clip(name: &str) -> Vec<f32> {
    let wav = std::fs::read(whistle_rust().join("test-data/clips").join(name)).expect("clip");
    let audio = whistle::audio::read_wav(&wav).expect("wav decodes");
    assert_eq!(audio.sample_rate, 16_000);
    audio.samples
}

macro_rules! require_model {
    () => {
        match model() {
            Some(m) => m,
            None => {
                eprintln!("skipped: no ../whistle-rust/models/whistle.cact");
                return;
            }
        }
    };
}

fn start(session: u32) -> ToWorker {
    ToWorker::Start {
        session,
        language: String::new(),
    }
}

#[test]
fn a_streamed_clip_maps_to_the_text_whistle_commits() {
    let whistle = require_model!();
    let samples = clip("en_samantha.wav");
    let mut engine = Engine::new(whistle);
    let mut mapper = TextMapper::new(true);
    let mut text = String::new();
    assert_eq!(engine.handle(start(1)), None);
    for chunk in samples.chunks(4_000) {
        match engine.handle(ToWorker::Push {
            session: 1,
            rate: 16_000,
            samples: chunk.to_vec(),
        }) {
            Some(FromWorker::Update { session: 1, update }) => mapper.update(&update),
            other => panic!("unexpected reply {other:?}"),
        }
        if let Some(d) = mapper.take() {
            text.push_str(&d.committed);
        }
    }

    // Before the finish, whistle's own stream over the same chunks has
    // committed exactly the text the mapper delivered (its own join rules).
    let mut reference = whistle.stream(&Options::streaming()).expect("stream");
    for chunk in samples.chunks(4_000) {
        reference.push(chunk).expect("push");
    }
    assert_eq!(text, reference.transcript().text);

    match engine.handle(ToWorker::Finish { session: 1 }) {
        Some(FromWorker::Final { session: 1, update }) => mapper.update(&update),
        other => panic!("unexpected reply {other:?}"),
    }
    mapper.finish();
    if let Some(d) = mapper.take() {
        text.push_str(&d.committed);
        assert_eq!(d.pending, "", "nothing pending after the end");
    }
    assert_eq!(engine.session(), None, "finish closes the stream");
    assert_eq!(text, "Make the cube 20 mm wide and round the top edges.");
}

#[test]
fn messages_for_other_sessions_are_ignored() {
    let whistle = require_model!();
    let mut engine = Engine::new(whistle);
    engine.handle(start(2));
    let push = |session| ToWorker::Push {
        session,
        rate: 16_000,
        samples: vec![0.0; 1_600],
    };
    assert_eq!(engine.handle(push(1)), None, "stale session");
    assert_eq!(engine.handle(ToWorker::Cancel { session: 1 }), None);
    assert_eq!(
        engine.session(),
        Some(2),
        "a stale cancel leaves the stream"
    );
    assert!(matches!(
        engine.handle(push(2)),
        Some(FromWorker::Update { session: 2, .. })
    ));
}

#[test]
fn a_cancelled_stream_takes_no_more_audio_and_still_answers_finish() {
    let whistle = require_model!();
    let mut engine = Engine::new(whistle);
    engine.handle(start(3));
    assert_eq!(engine.handle(ToWorker::Cancel { session: 3 }), None);
    assert_eq!(engine.session(), None);
    let pushed = engine.handle(ToWorker::Push {
        session: 3,
        rate: 16_000,
        samples: vec![0.0; 1_600],
    });
    assert_eq!(pushed, None);
    assert_eq!(
        engine.handle(ToWorker::Finish { session: 3 }),
        Some(FromWorker::Final {
            session: 3,
            update: Recognised::default()
        })
    );
}

/// The text a session gets for `samples` at `rate`, pushed in `chunk`s.
fn dictate(whistle: &'static Whistle, rate: u32, samples: &[f32], chunk: usize) -> String {
    let mut engine = Engine::new(whistle);
    let mut mapper = TextMapper::new(true);
    engine.handle(start(4));
    let mut text = String::new();
    for piece in samples.chunks(chunk) {
        match engine.handle(ToWorker::Push {
            session: 4,
            rate,
            samples: piece.to_vec(),
        }) {
            Some(FromWorker::Update { update, .. }) => mapper.update(&update),
            other => panic!("unexpected reply {other:?}"),
        }
        if let Some(d) = mapper.take() {
            text.push_str(&d.committed);
        }
    }
    match engine.handle(ToWorker::Finish { session: 4 }) {
        Some(FromWorker::Final { update, .. }) => mapper.update(&update),
        other => panic!("unexpected reply {other:?}"),
    }
    mapper.finish();
    if let Some(d) = mapper.take() {
        text.push_str(&d.committed);
    }
    text
}

#[test]
fn audio_at_another_rate_is_resampled() {
    let whistle = require_model!();
    let samples = clip("en_samantha.wav");
    let upsampled = whistle::audio::resample(&samples, 16_000, 48_000);
    let text = dictate(whistle, 48_000, &upsampled, 12_000);
    assert!(text.to_lowercase().contains("cube"), "recognised: {text}");
}

/// The audio a stream gets at 16 kHz is the same however the device's
/// audio was cut into pushes, so the words are too: the 250 ms pushes of
/// a 44.1 kHz microphone read as the whole clip resampled at once does.
#[test]
fn pushes_at_another_rate_read_as_the_whole_clip_resampled() {
    let whistle = require_model!();
    let samples = clip("en_samantha.wav");
    let device = whistle::audio::resample(&samples, 16_000, 44_100);
    let whole = crate::resample::tests::whole(&device, 44_100);
    let reference = dictate(whistle, 16_000, &whole, 4_000);
    assert_eq!(dictate(whistle, 44_100, &device, 11_025), reference);
    assert_eq!(dictate(whistle, 44_100, &device, 1_000), reference);
}

fn rec(text: &str, continues: bool, pending: &str) -> Recognised {
    Recognised {
        text: text.into(),
        continues,
        pending: pending.into(),
    }
}

/// Two updates merged map to the same text as the two in a row.
#[test]
fn merged_updates_read_as_the_two_in_a_row() {
    let cases = [
        (rec("", false, "a"), rec("make the", false, "cube")),
        (rec("make the", false, "cu"), rec("", false, "cube")),
        (rec("make the", false, "cu"), rec("be wide", true, "")),
        (rec(" 2", true, ""), rec("0 mm ", true, "wide")),
        (rec("make", false, ""), rec("the cube", false, "x")),
        (rec("  ", false, "p"), rec("", false, "")),
    ];
    for (first, second) in cases {
        for sentence_start in [true, false] {
            let mut apart = TextMapper::new(sentence_start);
            apart.update(&rec("Say", false, ""));
            apart.update(&first);
            apart.update(&second);
            let mut merged = TextMapper::new(sentence_start);
            merged.update(&rec("Say", false, ""));
            merged.update(&merge(first.clone(), second.clone()));
            assert_eq!(merged.take(), apart.take(), "{first:?} then {second:?}");
            let mut fresh_apart = TextMapper::new(sentence_start);
            fresh_apart.update(&first);
            fresh_apart.update(&second);
            let mut fresh_merged = TextMapper::new(sentence_start);
            fresh_merged.update(&merge(first.clone(), second.clone()));
            assert_eq!(
                fresh_merged.take(),
                fresh_apart.take(),
                "{first:?} then {second:?}"
            );
        }
    }
}
