//! Tests for `protocol.rs`: every message survives the trip through its
//! fields (the form it takes as a JavaScript object), malformed messages are
//! refused with the field at fault, and the engine's queue skips cancelled
//! work and keeps the rest in order.

use std::collections::{HashMap, VecDeque};

use super::*;

fn as_map(fields: Vec<(&'static str, Field)>) -> HashMap<String, Field> {
    fields
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect()
}

fn to_round_trip(msg: ToWorker) -> ToWorker {
    let map = as_map(msg.to_fields());
    ToWorker::from_fields(&|k| map.get(k).cloned()).expect("decodes")
}

fn from_round_trip(msg: FromWorker) -> FromWorker {
    let map = as_map(msg.to_fields());
    FromWorker::from_fields(&|k| map.get(k).cloned()).expect("decodes")
}

#[test]
fn every_message_to_the_worker_round_trips() {
    for msg in [
        ToWorker::Load {
            urls: vec!["https://a/whistle.cact".into(), "./b.cact".into()],
        },
        ToWorker::Load { urls: vec![] },
        ToWorker::Start {
            session: 7,
            language: "de".into(),
        },
        ToWorker::Start {
            session: 0,
            language: String::new(),
        },
        ToWorker::Push {
            session: 3,
            rate: 48_000,
            samples: vec![0.0, -0.5, 0.25],
        },
        ToWorker::Finish { session: 3 },
        ToWorker::Cancel { session: u32::MAX },
    ] {
        assert_eq!(to_round_trip(msg.clone()), msg);
    }
}

#[test]
fn every_message_from_the_worker_round_trips() {
    let update = Recognised {
        text: "hello \"world\"\n".into(),
        continues: true,
        pending: "and more".into(),
        pending_continues: true,
    };
    for msg in [
        FromWorker::Progress {
            loaded: 1_234_567,
            total: 16_919_407,
        },
        FromWorker::Loaded {
            source: "cache".into(),
        },
        FromWorker::LoadFailed {
            reason: LoadFailure::Checksum,
            detail: "size differs".into(),
        },
        FromWorker::LoadFailed {
            reason: LoadFailure::Download,
            detail: "HTTP 404".into(),
        },
        FromWorker::LoadFailed {
            reason: LoadFailure::Model,
            detail: "bad archive".into(),
        },
        FromWorker::LoadFailed {
            reason: LoadFailure::Startup,
            detail: "no wasm".into(),
        },
        FromWorker::Update {
            session: 2,
            update: update.clone(),
        },
        FromWorker::Final {
            session: 2,
            update: Recognised::default(),
        },
        FromWorker::Failed {
            session: 9,
            detail: "non-finite sample".into(),
        },
    ] {
        assert_eq!(from_round_trip(msg.clone()), msg);
    }
}

#[test]
fn malformed_messages_name_the_field_at_fault() {
    let unknown = as_map(vec![("type", Field::Str("explode".into()))]);
    let err = ToWorker::from_fields(&|k| unknown.get(k).cloned()).unwrap_err();
    assert!(err.0.contains("explode"), "{err}");

    let missing = as_map(vec![("type", Field::Str("finish".into()))]);
    let err = ToWorker::from_fields(&|k| missing.get(k).cloned()).unwrap_err();
    assert!(err.0.contains("session"), "{err}");

    let wrong = as_map(vec![
        ("type", Field::Str("push".into())),
        ("session", Field::Num(1.0)),
        ("rate", Field::Num(16_000.0)),
        ("samples", Field::Str("nope".into())),
    ]);
    let err = ToWorker::from_fields(&|k| wrong.get(k).cloned()).unwrap_err();
    assert!(err.0.contains("samples"), "{err}");

    let fractional = as_map(vec![
        ("type", Field::Str("cancel".into())),
        ("session", Field::Num(1.5)),
    ]);
    assert!(ToWorker::from_fields(&|k| fractional.get(k).cloned()).is_err());

    let reason = as_map(vec![
        ("type", Field::Str("loadFailed".into())),
        ("reason", Field::Str("gremlins".into())),
        ("detail", Field::Str(String::new())),
    ]);
    let err = FromWorker::from_fields(&|k| reason.get(k).cloned()).unwrap_err();
    assert!(err.0.contains("gremlins"), "{err}");
}

fn push(session: u32, rate: u32, samples: &[f32]) -> ToWorker {
    ToWorker::Push {
        session,
        rate,
        samples: samples.to_vec(),
    }
}

fn drain(queue: &mut VecDeque<ToWorker>) -> Vec<ToWorker> {
    std::iter::from_fn(|| next_job(queue)).collect()
}

#[test]
fn queued_pushes_stay_separate_and_in_order() {
    let mut q: VecDeque<ToWorker> = [
        ToWorker::Start {
            session: 1,
            language: String::new(),
        },
        push(1, 16_000, &[1.0]),
        push(1, 16_000, &[2.0, 3.0]),
        push(1, 16_000, &[4.0]),
        ToWorker::Finish { session: 1 },
    ]
    .into();
    assert_eq!(
        drain(&mut q),
        vec![
            ToWorker::Start {
                session: 1,
                language: String::new()
            },
            push(1, 16_000, &[1.0]),
            push(1, 16_000, &[2.0, 3.0]),
            push(1, 16_000, &[4.0]),
            ToWorker::Finish { session: 1 },
        ]
    );
}

#[test]
fn work_for_a_session_cancelled_later_is_skipped() {
    let mut q: VecDeque<ToWorker> = [
        ToWorker::Start {
            session: 1,
            language: String::new(),
        },
        push(1, 16_000, &[1.0]),
        ToWorker::Finish { session: 1 },
        ToWorker::Cancel { session: 1 },
        ToWorker::Start {
            session: 2,
            language: String::new(),
        },
        push(2, 16_000, &[5.0]),
    ]
    .into();
    assert_eq!(
        drain(&mut q),
        vec![
            ToWorker::Cancel { session: 1 },
            ToWorker::Start {
                session: 2,
                language: String::new()
            },
            push(2, 16_000, &[5.0]),
        ]
    );
}
