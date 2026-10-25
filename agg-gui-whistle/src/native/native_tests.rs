//! Tests for the native engine thread (`native/mod.rs`): a load that fails
//! is reported and ends the thread, so the next load starts a new one; and
//! a dictation runs through it. The model tests skip without
//! `../whistle-rust/models/whistle.cact`.

use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use super::*;
use crate::engine::tests::clip;

fn notifier() -> (Notifier, Receiver<Event>) {
    let (tx, rx) = mpsc::channel();
    (
        Notifier {
            events: tx,
            queue: UiQueue::default(),
        },
        rx,
    )
}

fn config(model_path: Option<PathBuf>) -> Config {
    let cache = std::env::temp_dir().join(format!("agg-gui-whistle-native-{}", std::process::id()));
    Config {
        model_urls: Vec::new(),
        model_path,
        cache_dir: Some(cache),
        ..Config::default()
    }
}

/// The next engine reply, skipping progress and microphone events.
fn reply(events: &Receiver<Event>) -> FromWorker {
    loop {
        match events.recv_timeout(Duration::from_secs(60)) {
            Ok(Event::Worker(FromWorker::Progress { .. })) => continue,
            Ok(Event::Worker(msg)) => return msg,
            Ok(_) => continue,
            Err(err) => panic!("no reply: {err}"),
        }
    }
}

fn load() -> ToWorker {
    ToWorker::Load { urls: Vec::new() }
}

#[test]
fn a_failed_load_reports_and_ends_the_thread() {
    let missing = std::env::temp_dir().join("agg-gui-whistle-no-such-model.cact");
    let source = model::Source::from_config(&config(Some(missing)));
    let (notifier, events) = notifier();
    let engine = spawn_engine(source, notifier);
    engine.send(load()).expect("send");
    assert!(matches!(
        reply(&events),
        FromWorker::LoadFailed {
            reason: LoadFailure::Download,
            ..
        }
    ));
    // The thread is gone: its inbox no longer takes messages.
    std::thread::sleep(Duration::from_millis(100));
    assert!(engine.send(load()).is_err());
}

#[test]
fn a_dictation_runs_through_the_engine_thread() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../whistle-rust/models/whistle.cact");
    if !path.exists() {
        eprintln!("skipped: no ../whistle-rust/models/whistle.cact");
        return;
    }
    let source = model::Source::from_config(&config(Some(path)));
    let (notifier, events) = notifier();
    let engine = spawn_engine(source, notifier);
    engine.send(load()).expect("send");
    assert!(matches!(reply(&events), FromWorker::Loaded { .. }));
    let mut machine = Machine::new(Vec::new(), None);
    machine.on_worker(FromWorker::Loaded {
        source: "test".into(),
    });
    let mut ctx = agg_gui::widgets::on_screen_keyboard::VoiceContext::default();
    ctx.sentence_start = true;
    machine.start(&ctx);
    let session = machine.current_session().expect("session");
    for command in machine.take_commands() {
        if let Command::Worker(msg) = command {
            engine.send(msg).expect("send");
        }
    }
    machine.on_mic_opened(session);
    // All the audio at once: the thread works through the queued pushes.
    for chunk in clip("en_samantha.wav").chunks(4_000) {
        engine
            .send(ToWorker::Push {
                session,
                rate: 16_000,
                samples: chunk.to_vec(),
            })
            .expect("send");
    }
    machine.stop();
    for command in machine.take_commands() {
        if let Command::Worker(msg) = command {
            engine.send(msg).expect("send");
        }
    }
    let mut text = String::new();
    loop {
        let msg = reply(&events);
        let last = matches!(msg, FromWorker::Final { .. });
        machine.on_worker(msg);
        if let Some(t) = machine.poll().text {
            text.push_str(&t.committed);
        }
        if last {
            break;
        }
    }
    assert_eq!(text, "Make the cube 20 mm wide and round the top edges.");
}

/// A panic on the engine thread is reported, so the key doesn't stay on
/// Listening (or Preparing) with nobody answering.
#[test]
fn an_engine_thread_that_panics_reports_the_engine_lost() {
    let (notifier, events) = notifier();
    let thread = std::thread::spawn(move || {
        let _guard = LostGuard(notifier);
        panic!("a kernel bug");
    });
    assert!(thread.join().is_err());
    assert!(matches!(events.try_recv(), Ok(Event::EngineLost(_))));
}

#[test]
fn an_engine_thread_that_returns_reports_nothing_more() {
    let (notifier, events) = notifier();
    std::thread::spawn(move || {
        let _guard = LostGuard(notifier);
    })
    .join()
    .expect("no panic");
    assert!(events.try_recv().is_err());
}

/// The host takes a lost engine as the machine's `worker_lost`: a
/// dictation in progress ends with an error and the next load starts a new
/// thread.
#[cfg(feature = "native-mic")]
#[test]
fn the_host_ends_the_dictation_when_the_engine_is_lost() {
    use agg_gui::widgets::on_screen_keyboard::VoiceState;
    let host = NativeHost::new(config(None));
    // A listening session, its commands dropped (no device is opened).
    let mut session = None;
    host.dispatch(&mut |m| {
        m.on_worker(FromWorker::Loaded {
            source: "test".into(),
        });
        m.start(&agg_gui::widgets::on_screen_keyboard::VoiceContext::default());
        m.take_commands();
        let s = m.current_session().expect("a session");
        m.on_mic_opened(s);
        session = Some(s);
    });
    let mut state = None;
    host.dispatch(&mut |m| state = Some(m.poll().state));
    assert!(
        matches!(state, Some(VoiceState::Listening { .. })),
        "{state:?}"
    );

    let notifier = host.inner.borrow().notifier.clone();
    notifier.send(Event::EngineLost("panicked".into()));
    let mut current = session;
    host.dispatch(&mut |m| {
        state = Some(m.poll().state);
        current = m.current_session();
    });
    assert_eq!(current, None, "the dictation is over");
    assert!(
        matches!(state, Some(VoiceState::Error { retry: true, .. })),
        "{state:?}"
    );
    assert!(host.inner.borrow().engine.is_none());
}
