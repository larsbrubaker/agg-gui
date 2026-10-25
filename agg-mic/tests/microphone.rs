//! The chunk plumbing and the one-recording state of `agg_mic::Microphone`, over a fake backend
//! (no device needed): chunks reach the handler while recording and the last one before a stop
//! returns, nothing gets through after the stop, 16-bit audio arrives as -1..1 floats, a failure
//! is reported once and ends the recording, a second start is refused, a stop during a start that
//! waits on a permission prompt is honoured once it is answered (and nothing at all of that start
//! reaches the handlers), a handler that panics ends the recording instead of the process, the
//! handlers are dropped inside the stop (so a consumer's last flush lands before it returns), and
//! a refused or unavailable microphone is left idle. Adapted from mattercad-ai's `tests/ai/audio_chunks_tests.rs` and
//! `audio_recorder_tests.rs`.

use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use agg_mic::{
    Backend, ChunkHandler, Delivery, FailureHandler, MicError, Microphone, ALREADY_RUNNING_MESSAGE,
    DISPLAY_TEXTS, HANDLER_PANICKED_MESSAGE,
};

/// The fake backend's knobs and counters, shared between the test and the microphone.
#[derive(Default)]
struct FakeState {
    unavailable: AtomicBool,
    deny: AtomicBool,
    /// A start waits for a message here, as a start waits on a permission prompt.
    gate: Mutex<Option<Receiver<()>>>,
    /// The running recording's delivery, as a capture thread holds it.
    delivery: Mutex<Option<Delivery>>,
    /// The last delivery, kept past the stop to try delivering late.
    last: Mutex<Option<Delivery>>,
    starts: AtomicI32,
    stops: AtomicI32,
}

/// Hands on one chunk as it starts and one as it stops, as a platform capture delivers its last
/// audio before its stop returns.
struct FakeBackend(Arc<FakeState>);

impl Backend for FakeBackend {
    fn is_available(&self) -> bool {
        !self.0.unavailable.load(Ordering::SeqCst)
    }

    fn start(&self, delivery: Delivery) -> Result<(), MicError> {
        let gate = self.0.gate.lock().unwrap().take();
        if let Some(gate) = gate {
            let _ = gate.recv();
        }
        if self.0.deny.load(Ordering::SeqCst) {
            return Err(MicError::PermissionDenied("denied".to_owned()));
        }
        self.0.starts.fetch_add(1, Ordering::SeqCst);
        delivery.deliver(&[0.5; 4], 48_000);
        *self.0.last.lock().unwrap() = Some(delivery.clone());
        *self.0.delivery.lock().unwrap() = Some(delivery);
        Ok(())
    }

    fn stop(&self) {
        self.0.stops.fetch_add(1, Ordering::SeqCst);
        if let Some(delivery) = self.0.delivery.lock().unwrap().take() {
            delivery.deliver(&[0.25; 2], 48_000);
        }
    }
}

/// The chunks a handler kept, each with its rate.
type Kept = Arc<Mutex<Vec<(Vec<f32>, u32)>>>;

fn keeping() -> (ChunkHandler, Kept) {
    let kept: Kept = Arc::default();
    let into = Arc::clone(&kept);
    let handler: ChunkHandler = Box::new(move |samples: &[f32], rate: u32| {
        into.lock().unwrap().push((samples.to_vec(), rate));
    });
    (handler, kept)
}

fn failures() -> (FailureHandler, Arc<Mutex<Vec<MicError>>>) {
    let kept: Arc<Mutex<Vec<MicError>>> = Arc::default();
    let into = Arc::clone(&kept);
    (
        Box::new(move |error| into.lock().unwrap().push(error)),
        kept,
    )
}

fn ignore_failures() -> FailureHandler {
    Box::new(|_| {})
}

fn fake() -> (Arc<Microphone>, Arc<FakeState>) {
    let state = Arc::new(FakeState::default());
    let microphone = Microphone::with_backend(FakeBackend(Arc::clone(&state)));
    (Arc::new(microphone), state)
}

impl FakeState {
    fn running(&self) -> Delivery {
        self.delivery.lock().unwrap().clone().expect("recording")
    }

    fn last(&self) -> Delivery {
        self.last.lock().unwrap().clone().expect("started once")
    }

    fn gate(&self) -> Sender<()> {
        let (open, gate) = mpsc::channel();
        *self.gate.lock().unwrap() = Some(gate);
        open
    }
}

#[test]
fn chunks_reach_the_handler_and_the_last_arrives_before_the_stop_returns() {
    let (microphone, state) = fake();
    let (handler, kept) = keeping();
    microphone.start(handler, ignore_failures()).unwrap();
    assert!(microphone.is_recording());
    state.running().deliver(&[0.75; 3], 48_000);
    // An empty chunk is not news.
    state.running().deliver(&[], 48_000);
    microphone.stop();
    assert!(!microphone.is_recording());
    assert_eq!(
        *kept.lock().unwrap(),
        vec![
            (vec![0.5; 4], 48_000),
            (vec![0.75; 3], 48_000),
            (vec![0.25; 2], 48_000)
        ]
    );
}

#[test]
fn nothing_gets_through_after_the_stop() {
    let (microphone, state) = fake();
    let (handler, kept) = keeping();
    microphone.start(handler, ignore_failures()).unwrap();
    microphone.stop();
    let late = state.last();
    assert!(!late.is_open());
    late.deliver(&[0.1; 8], 48_000);
    late.fail(MicError::Failed("late".to_owned()));
    assert_eq!(
        kept.lock().unwrap().len(),
        2,
        "the start's and the stop's chunk only"
    );
}

#[test]
fn sixteen_bit_chunks_arrive_as_floats_from_minus_one_to_one() {
    let (microphone, state) = fake();
    let (handler, kept) = keeping();
    microphone.start(handler, ignore_failures()).unwrap();
    state
        .running()
        .deliver_pcm16(&[i16::MIN, 0, 16_384], 16_000);
    microphone.stop();
    assert_eq!(kept.lock().unwrap()[1], (vec![-1.0, 0.0, 0.5], 16_000));
}

#[test]
fn a_failure_is_reported_once_and_ends_the_recording() {
    let (microphone, state) = fake();
    let (handler, kept) = keeping();
    let (on_failure, failed) = failures();
    microphone.start(handler, on_failure).unwrap();
    let running = state.running();
    running.fail(MicError::NoDevice("unplugged".to_owned()));
    running.fail(MicError::Failed("again".to_owned()));
    running.deliver(&[0.1; 8], 48_000);
    assert_eq!(
        *failed.lock().unwrap(),
        vec![MicError::NoDevice("unplugged".to_owned())]
    );
    assert_eq!(
        kept.lock().unwrap().len(),
        1,
        "no chunk follows the failure"
    );
    assert!(!microphone.is_recording());

    // A failed recording is cleaned up by the next start; no stop is needed first.
    let (handler, _kept) = keeping();
    microphone.start(handler, ignore_failures()).unwrap();
    assert_eq!(state.stops.load(Ordering::SeqCst), 1);
    assert!(microphone.is_recording());
}

#[test]
fn a_stop_is_never_reported_as_a_failure() {
    let (microphone, _state) = fake();
    let (handler, _kept) = keeping();
    let (on_failure, failed) = failures();
    microphone.start(handler, on_failure).unwrap();
    microphone.stop();
    assert!(failed.lock().unwrap().is_empty());
}

#[test]
fn starting_twice_is_refused() {
    let (microphone, state) = fake();
    let (handler, _kept) = keeping();
    microphone.start(handler, ignore_failures()).unwrap();
    let (second, _kept) = keeping();
    assert_eq!(
        microphone.start(second, ignore_failures()),
        Err(MicError::Failed(ALREADY_RUNNING_MESSAGE.to_owned()))
    );
    assert_eq!(state.starts.load(Ordering::SeqCst), 1);
    assert!(microphone.is_recording(), "the first recording goes on");
}

#[test]
fn a_stop_during_the_permission_prompt_stops_once_it_is_answered() {
    let (microphone, state) = fake();
    let answer = state.gate();
    let (handler, kept) = keeping();
    let (on_failure, failed) = failures();
    let starting = {
        let microphone = Arc::clone(&microphone);
        std::thread::spawn(move || microphone.start(handler, on_failure))
    };
    // Wait until the start is inside the backend (it took the gate), then stop.
    while state.gate.lock().unwrap().is_some() {
        std::thread::sleep(Duration::from_millis(1));
    }
    microphone.stop();
    assert_eq!(state.stops.load(Ordering::SeqCst), 0, "nothing to stop yet");
    answer.send(()).unwrap();

    assert_eq!(starting.join().unwrap(), Err(MicError::Stopped));
    assert!(!microphone.is_recording());
    assert_eq!(state.stops.load(Ordering::SeqCst), 1);
    // Nothing of the stopped start reached the handlers: not the chunk its backend delivered as
    // it started (after the stop had returned), not a later one, not a failure.
    state.last().deliver(&[0.1; 8], 48_000);
    state.last().fail(MicError::Failed("late".to_owned()));
    assert!(
        kept.lock().unwrap().is_empty(),
        "{:?}",
        kept.lock().unwrap()
    );
    assert!(failed.lock().unwrap().is_empty());
    let (handler, _kept) = keeping();
    microphone.start(handler, ignore_failures()).unwrap();
}

#[test]
fn a_refused_microphone_is_left_idle() {
    let (microphone, state) = fake();
    state.deny.store(true, Ordering::SeqCst);
    let (handler, _kept) = keeping();
    assert!(matches!(
        microphone.start(handler, ignore_failures()),
        Err(MicError::PermissionDenied(_))
    ));
    assert!(!microphone.is_recording());
    state.deny.store(false, Ordering::SeqCst);
    let (handler, _kept) = keeping();
    microphone.start(handler, ignore_failures()).unwrap();
    assert!(microphone.is_recording());
}

#[test]
fn an_unavailable_microphone_refuses_before_capturing() {
    let (microphone, state) = fake();
    state.unavailable.store(true, Ordering::SeqCst);
    assert!(!microphone.is_available());
    let (handler, _kept) = keeping();
    assert!(matches!(
        microphone.start(handler, ignore_failures()),
        Err(MicError::Unsupported(_))
    ));
    assert_eq!(state.starts.load(Ordering::SeqCst), 0);
}

#[test]
fn dropping_the_microphone_stops_its_recording() {
    let state = Arc::new(FakeState::default());
    let microphone = Microphone::with_backend(FakeBackend(Arc::clone(&state)));
    let (handler, _kept) = keeping();
    microphone.start(handler, ignore_failures()).unwrap();
    drop(microphone);
    assert_eq!(state.stops.load(Ordering::SeqCst), 1);
}

#[test]
fn every_error_has_a_message_to_show() {
    assert!(DISPLAY_TEXTS.iter().all(|text| !text.is_empty()));
    assert!(DISPLAY_TEXTS.contains(&MicError::Stopped.message()));
    assert_eq!(
        MicError::NoDevice("gone".to_owned()).to_string(),
        "gone".to_owned()
    );
}

#[test]
fn a_stop_during_a_start_that_then_fails_is_still_just_a_stop() {
    // A start that gives up (a sound server that never answers, a refused prompt) after the user
    // already stopped: the stop is what happened, not an error to show.
    let (microphone, state) = fake();
    let answer = state.gate();
    state.deny.store(true, Ordering::SeqCst);
    let (handler, _kept) = keeping();
    let starting = {
        let microphone = Arc::clone(&microphone);
        std::thread::spawn(move || microphone.start(handler, ignore_failures()))
    };
    while state.gate.lock().unwrap().is_some() {
        std::thread::sleep(Duration::from_millis(1));
    }
    microphone.stop();
    answer.send(()).unwrap();
    assert_eq!(starting.join().unwrap(), Err(MicError::Stopped));
    assert_eq!(state.stops.load(Ordering::SeqCst), 0, "nothing started");
}

#[test]
fn a_handler_that_panics_ends_the_recording_instead_of_the_process() {
    let (microphone, state) = fake();
    let (on_failure, failed) = failures();
    let mut seen = 0;
    microphone
        .start(
            Box::new(move |_: &[f32], _: u32| {
                // FnMut: the handler keeps its own state, no lock needed.
                seen += 1;
                if seen == 2 {
                    panic!("a handler bug");
                }
            }),
            on_failure,
        )
        .unwrap();
    let running = state.running();
    running.deliver(&[0.1; 8], 48_000);
    assert_eq!(
        *failed.lock().unwrap(),
        vec![MicError::Failed(HANDLER_PANICKED_MESSAGE.to_owned())]
    );
    assert!(!running.is_open(), "no chunk follows the panic");
    assert!(!microphone.is_recording());
    microphone.stop();
}

#[test]
fn a_failure_handler_that_panics_is_contained() {
    let (microphone, state) = fake();
    let (handler, _kept) = keeping();
    microphone
        .start(handler, Box::new(|_| panic!("a failure handler bug")))
        .unwrap();
    state
        .running()
        .fail(MicError::NoDevice("unplugged".to_owned()));
    assert!(!microphone.is_recording());
    microphone.stop();
}

/// Records when it is dropped, as a consumer's handler state that flushes on drop would.
struct DropFlag(&'static str, Arc<Mutex<Vec<&'static str>>>);

impl Drop for DropFlag {
    fn drop(&mut self) {
        self.1.lock().unwrap().push(self.0);
    }
}

#[test]
fn the_stop_drops_the_handlers_before_it_returns() {
    let (microphone, _state) = fake();
    let events: Arc<Mutex<Vec<&'static str>>> = Arc::default();
    let chunk_flag = DropFlag("chunks dropped", Arc::clone(&events));
    let failure_flag = DropFlag("failure dropped", Arc::clone(&events));
    microphone
        .start(
            Box::new(move |_: &[f32], _: u32| {
                let _ = &chunk_flag;
            }),
            Box::new(move |_| {
                let _ = &failure_flag;
            }),
        )
        .unwrap();
    microphone.stop();
    let mut dropped = events.lock().unwrap().clone();
    dropped.sort_unstable();
    assert_eq!(dropped, vec!["chunks dropped", "failure dropped"]);
}

#[test]
fn a_failure_drops_the_chunk_handler_before_it_reports() {
    // A consumer whose chunk handler flushes on drop has its last audio out before it hears of
    // the failure, so it can finish with everything that was heard.
    let (microphone, state) = fake();
    let events: Arc<Mutex<Vec<&'static str>>> = Arc::default();
    let chunk_flag = DropFlag("chunks dropped", Arc::clone(&events));
    let reported = Arc::clone(&events);
    microphone
        .start(
            Box::new(move |_: &[f32], _: u32| {
                let _ = &chunk_flag;
            }),
            Box::new(move |_| reported.lock().unwrap().push("failure reported")),
        )
        .unwrap();
    state
        .running()
        .fail(MicError::NoDevice("unplugged".to_owned()));
    assert_eq!(
        *events.lock().unwrap(),
        vec!["chunks dropped", "failure reported"]
    );
}

#[test]
fn a_stop_costs_next_to_nothing_beyond_the_backends_own() {
    // Microphone::stop adds its state change and the delivery's close to the backend's stop;
    // agg-gui-whistle calls it on the UI thread.
    let (microphone, _state) = fake();
    let (handler, _kept) = keeping();
    microphone.start(handler, ignore_failures()).unwrap();
    let stopping = std::time::Instant::now();
    microphone.stop();
    let took = stopping.elapsed();
    eprintln!("Microphone::stop over a fake backend: {took:?}");
    assert!(took < Duration::from_millis(2), "a stop took {took:?}");
}
