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

#[test]
fn audio_at_another_rate_is_resampled() {
    let whistle = require_model!();
    let samples = clip("en_samantha.wav");
    let upsampled = whistle::audio::resample(&samples, 16_000, 48_000);
    let mut engine = Engine::new(whistle);
    let mut mapper = TextMapper::new(true);
    engine.handle(start(4));
    for chunk in upsampled.chunks(12_000) {
        if let Some(FromWorker::Update { update, .. }) = engine.handle(ToWorker::Push {
            session: 4,
            rate: 48_000,
            samples: chunk.to_vec(),
        }) {
            mapper.update(&update);
        }
    }
    if let Some(FromWorker::Final { update, .. }) = engine.handle(ToWorker::Finish { session: 4 }) {
        mapper.update(&update);
    }
    mapper.finish();
    let text = mapper.take().map(|d| d.committed).unwrap_or_default();
    assert!(text.to_lowercase().contains("cube"), "recognised: {text}");
}
