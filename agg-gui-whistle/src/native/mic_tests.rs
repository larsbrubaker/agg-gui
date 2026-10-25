//! Tests for `native/mic.rs` without a device: what the [`Sink`] does
//! with chunks, when the engine is gone and when it is dropped (the last
//! chunk out, unless discarded), what agg-mic's errors become, and the
//! [`Recorder`] over a fake agg-mic backend (opened, closed with or without
//! its last audio, failed while listening, refused, closed while its start
//! still waits on a prompt), with how long a close takes.

use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use agg_gui::ui_thread::UiQueue;
use agg_mic::{Backend, Delivery};

use super::*;

fn notifier() -> (Notifier, Receiver<Event>) {
    let (events_tx, events) = mpsc::channel();
    let notifier = Notifier {
        events: events_tx,
        queue: UiQueue::default(),
    };
    (notifier, events)
}

/// A sink for session 7 at 16 kHz with 100-sample pushes, its discard
/// flag, the engine's inbox and the UI thread's events.
fn new_sink() -> (Sink, Arc<AtomicBool>, Receiver<ToWorker>, Receiver<Event>) {
    let (engine, inbox) = mpsc::channel();
    let (notifier, events) = notifier();
    let discard = Arc::new(AtomicBool::new(false));
    let mut sink = Sink::new(7, engine, notifier, discard.clone());
    sink.rate = 16_000;
    sink.chunk = 100;
    (sink, discard, inbox, events)
}

/// The pushes waiting in the engine's inbox, as (rate, sample count).
fn pushes(inbox: &Receiver<ToWorker>) -> Vec<(u32, usize)> {
    inbox
        .try_iter()
        .map(|msg| match msg {
            ToWorker::Push { rate, samples, .. } => (rate, samples.len()),
            _ => panic!("only pushes go to the engine from here"),
        })
        .collect()
}

#[test]
fn a_full_chunk_goes_to_the_engine() {
    let (mut sink, _discard, inbox, _events) = new_sink();
    sink.feed(&[0.1; 150], 16_000);
    assert!(matches!(
        inbox.try_recv(),
        Ok(ToWorker::Push { session: 7, rate: 16_000, ref samples }) if samples.len() == 150
    ));
}

#[test]
fn a_change_of_rate_pushes_the_old_rates_audio_first() {
    let (mut sink, _discard, inbox, _events) = new_sink();
    sink.feed(&[0.1; 50], 16_000);
    sink.feed(&[0.1; 30], 48_000);
    sink.flush();
    assert_eq!(pushes(&inbox), vec![(16_000, 50), (48_000, 30)]);
}

#[test]
fn a_push_the_engine_cannot_take_reports_it_lost_once() {
    let (mut sink, _discard, inbox, events) = new_sink();
    drop(inbox);
    sink.feed(&[0.1; 150], 16_000);
    sink.feed(&[0.1; 150], 16_000);
    drop(sink);
    let lost: Vec<_> = events
        .try_iter()
        .filter(|e| matches!(e, Event::EngineLost(_)))
        .collect();
    assert_eq!(lost.len(), 1);
}

#[test]
fn a_dropped_sink_sends_its_last_chunk_unless_discarded() {
    let (mut sink, _discard, inbox, _events) = new_sink();
    sink.feed(&[0.1; 40], 16_000);
    drop(sink);
    assert_eq!(pushes(&inbox), vec![(16_000, 40)]);

    let (mut sink, discard, inbox, _events) = new_sink();
    sink.feed(&[0.1; 40], 16_000);
    discard.store(true, Ordering::SeqCst);
    drop(sink);
    assert!(pushes(&inbox).is_empty());
}

#[test]
fn agg_mics_errors_become_the_machines() {
    let cases = [
        (
            agg_mic::MicError::PermissionDenied("off".into()),
            MicError::Denied,
        ),
        (
            agg_mic::MicError::NoDevice("gone".into()),
            MicError::NoDevice,
        ),
        (
            agg_mic::MicError::Failed("broken".into()),
            MicError::Other("broken".into()),
        ),
        (
            agg_mic::MicError::Unsupported("none".into()),
            MicError::Other("none".into()),
        ),
    ];
    for (theirs, ours) in cases {
        assert_eq!(mic_error(theirs), ours);
    }
}

/// The fake backend's knobs, shared between the test and its microphones.
#[derive(Default)]
struct FakeState {
    deny: AtomicBool,
    /// A start waits for a message here, as on a permission prompt.
    gate: Mutex<Option<Receiver<()>>>,
    delivery: Mutex<Option<Delivery>>,
    stops: AtomicI32,
}

/// Delivers 40 samples at 16 kHz as it starts and 20 more as it stops.
struct FakeBackend(Arc<FakeState>);

impl Backend for FakeBackend {
    fn is_available(&self) -> bool {
        true
    }
    fn start(&self, delivery: Delivery) -> Result<(), agg_mic::MicError> {
        let gate = self.0.gate.lock().unwrap().take();
        if let Some(gate) = gate {
            let _ = gate.recv();
        }
        if self.0.deny.load(Ordering::SeqCst) {
            return Err(agg_mic::MicError::PermissionDenied("denied".into()));
        }
        delivery.deliver(&[0.5; 40], 16_000);
        *self.0.delivery.lock().unwrap() = Some(delivery);
        Ok(())
    }
    fn stop(&self) {
        if let Some(delivery) = self.0.delivery.lock().unwrap().take() {
            delivery.deliver(&[0.25; 20], 16_000);
        }
        self.0.stops.fetch_add(1, Ordering::SeqCst);
    }
}

/// A recorder whose microphones are fakes over one state, the engine's
/// inbox and the UI thread's events.
fn recorder() -> (
    Recorder,
    Arc<FakeState>,
    Sender<ToWorker>,
    Receiver<ToWorker>,
    Receiver<Event>,
) {
    let state = Arc::new(FakeState::default());
    let made = state.clone();
    let (notifier, events) = notifier();
    let recorder = Recorder::with_microphones(
        notifier,
        Arc::new(move || Microphone::with_backend(FakeBackend(made.clone()))),
    );
    let (engine, inbox) = mpsc::channel();
    (recorder, state, engine, inbox, events)
}

/// The next microphone event (levels skipped), within a few seconds.
fn next_mic_event(events: &Receiver<Event>) -> Event {
    loop {
        match events.recv_timeout(Duration::from_secs(5)) {
            Ok(Event::Level(..)) => continue,
            Ok(event) => return event,
            Err(err) => panic!("no microphone event: {err}"),
        }
    }
}

#[test]
fn a_close_hands_the_engine_every_chunk_before_it_returns() {
    let (mut recorder, state, engine, inbox, events) = recorder();
    recorder.open(7, engine);
    assert!(matches!(next_mic_event(&events), Event::MicOpened(7)));
    let closing = Instant::now();
    recorder.close(7, true);
    let took = closing.elapsed();
    // The start's 40 samples and the stop's 20, in one partial push.
    assert_eq!(pushes(&inbox), vec![(16_000, 60)]);
    assert_eq!(state.stops.load(Ordering::SeqCst), 1);
    // The plumbing's own cost on the UI thread (a real device's stop adds
    // its own: agg-mic's live test measures that).
    eprintln!("close_capture over a fake device: {took:?}");
    assert!(took < Duration::from_millis(16), "a close took {took:?}");
}

#[test]
fn a_close_without_its_last_audio_sends_nothing() {
    let (mut recorder, _state, engine, inbox, events) = recorder();
    recorder.open(7, engine);
    assert!(matches!(next_mic_event(&events), Event::MicOpened(7)));
    recorder.close(7, false);
    assert!(pushes(&inbox).is_empty());
}

#[test]
fn a_microphone_that_fails_while_listening_keeps_what_it_heard() {
    let (mut recorder, state, engine, inbox, events) = recorder();
    recorder.open(7, engine);
    assert!(matches!(next_mic_event(&events), Event::MicOpened(7)));
    let running = state.delivery.lock().unwrap().clone().expect("recording");
    running.fail(agg_mic::MicError::NoDevice("unplugged".into()));
    running.fail(agg_mic::MicError::Failed("again".into()));
    assert!(matches!(
        next_mic_event(&events),
        Event::MicFailed(7, MicError::NoDevice)
    ));
    // What was heard already went to the engine, ahead of the Finish the
    // machine sends on hearing of the failure.
    assert_eq!(pushes(&inbox), vec![(16_000, 40)]);
    // Once: the second failure said nothing.
    assert!(events
        .try_iter()
        .all(|e| !matches!(e, Event::MicFailed(..))));
    // The machine then stops as for a tap: nothing more to send.
    recorder.close(7, true);
    assert!(pushes(&inbox).is_empty());
}

#[test]
fn a_refused_microphone_fails_the_session() {
    let (mut recorder, state, engine, inbox, events) = recorder();
    state.deny.store(true, Ordering::SeqCst);
    recorder.open(7, engine);
    assert!(matches!(
        next_mic_event(&events),
        Event::MicFailed(7, MicError::Denied)
    ));
    assert!(pushes(&inbox).is_empty());
}

#[test]
fn a_close_while_the_start_waits_returns_at_once_and_the_start_stops_itself() {
    let (mut recorder, state, engine, inbox, events) = recorder();
    let (answer, gate) = mpsc::channel();
    *state.gate.lock().unwrap() = Some(gate);
    recorder.open(7, engine);
    // Wait for the start to reach the prompt, then close.
    let deadline = Instant::now() + Duration::from_secs(5);
    while state.gate.lock().unwrap().is_some() {
        assert!(Instant::now() < deadline, "the start never began");
        std::thread::sleep(Duration::from_millis(1));
    }
    recorder.close(7, true);
    assert_eq!(state.stops.load(Ordering::SeqCst), 0, "nothing to stop yet");
    answer.send(()).unwrap();
    while state.stops.load(Ordering::SeqCst) == 0 {
        assert!(Instant::now() < deadline, "the start never stopped itself");
        std::thread::sleep(Duration::from_millis(1));
    }
    // Nobody waits for that session: no event, no audio.
    std::thread::sleep(Duration::from_millis(50));
    assert!(events
        .try_iter()
        .all(|e| !matches!(e, Event::MicOpened(_) | Event::MicFailed(..))));
    assert!(pushes(&inbox).is_empty());
}
