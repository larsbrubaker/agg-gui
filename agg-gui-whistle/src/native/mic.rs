//! The native microphone (feature `native-mic`): an `agg_mic::Microphone`
//! per session on the default input device (AVAudioEngine on macOS, WASAPI
//! on Windows, the PulseAudio protocol on Linux; pure Rust, no `-sys`
//! crates), whose mono chunks go to a [`Sink`] the chunk handler owns,
//! which sends ~250 ms `Push`es straight to the engine thread at the
//! capture's rate (the engine resamples) and the level to the UI thread.
//!
//! A start asks for permission and opens the device, which can take as long
//! as the user leaves a prompt up, so a thread per session waits in it and
//! reports `MicOpened` or `MicFailed`. Closing stops the microphone on the
//! UI thread. agg-mic drops the chunk handler inside that stop, once the
//! last audio is in, and the sink's drop sends its last partial chunk (unless
//! the close discards it), so a stop's last audio reaches the engine before
//! the `Finish` the machine sends next. While the start still waits, the
//! stop returns at once and that start stops itself. The close is timed:
//! one slower than [`SLOW_CLOSE`] is logged, since it holds up a frame.
//!
//! A microphone that ends on its own (unplugged, the sound server gone)
//! ends the dictation with an error (`MicFailed`; agg-mic drops the chunk
//! handler first, so what was captured, its last chunk included, is already
//! on its way to the engine, and the machine finishes with it). A push the
//! engine thread can no longer take reports the engine lost.
//!
//! The OS asks for microphone permission at the first start (macOS wants
//! `NSMicrophoneUsageDescription` in the app bundle, agg-mic's docs say
//! what else); a refusal ends the dictation with "access denied", and the
//! console says where to turn the microphone back on.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::{Duration, Instant};

use agg_mic::Microphone;

use super::{Event, Notifier};
use crate::machine::MicError;
use crate::protocol::ToWorker;

/// Audio per push, in seconds.
const CHUNK_SECONDS: f32 = 0.25;
/// Samples between level reports (at most ~20 a second at 48 kHz).
const LEVEL_EVERY: usize = 2_400;
/// A close slower than this is logged: it runs on the UI thread.
const SLOW_CLOSE: Duration = Duration::from_millis(4);

/// Collects the microphone's chunks into pushes.
struct Sink {
    session: u32,
    /// The rate of the audio in `buffer`.
    rate: u32,
    buffer: Vec<f32>,
    /// Samples per push at `rate`.
    chunk: usize,
    engine: Sender<ToWorker>,
    notifier: Notifier,
    /// Cleared once the engine stopped taking audio: later chunks go
    /// nowhere.
    open: bool,
    /// Set by a close that drops the last audio (a cancel): the drop then
    /// sends nothing.
    discard: Arc<AtomicBool>,
    level: f32,
    since_level: usize,
}

impl Sink {
    fn new(
        session: u32,
        engine: Sender<ToWorker>,
        notifier: Notifier,
        discard: Arc<AtomicBool>,
    ) -> Sink {
        Sink {
            session,
            rate: 0,
            buffer: Vec::new(),
            chunk: 0,
            engine,
            notifier,
            open: true,
            discard,
            level: 0.0,
            since_level: 0,
        }
    }

    fn feed(&mut self, mono: &[f32], rate: u32) {
        if !self.open || mono.is_empty() {
            return;
        }
        if rate != self.rate {
            // A push carries one rate: the old rate's audio goes first.
            self.flush();
            self.rate = rate;
            self.chunk = (rate as f32 * CHUNK_SECONDS) as usize;
        }
        let power = mono.iter().map(|s| s * s).sum::<f32>() / mono.len() as f32;
        let db = 10.0 * (power + 1e-12).log10();
        let level = ((db + 55.0) / 40.0).clamp(0.0, 1.0);
        self.level = level.max(self.level * 0.9);
        self.since_level += mono.len();
        if self.since_level >= LEVEL_EVERY {
            self.since_level = 0;
            self.notifier.send(Event::Level(self.session, self.level));
        }
        self.buffer.extend_from_slice(mono);
        if self.buffer.len() >= self.chunk {
            self.flush();
        }
    }

    fn flush(&mut self) {
        if self.buffer.is_empty() {
            return;
        }
        let sent = self.engine.send(ToWorker::Push {
            session: self.session,
            rate: self.rate,
            samples: std::mem::take(&mut self.buffer),
        });
        if sent.is_err() {
            // The engine thread is gone: nothing will hear this session.
            self.open = false;
            self.notifier.send(Event::EngineLost(
                "the engine thread stopped taking audio".into(),
            ));
        }
    }
}

/// The chunk handler is dropped when the recording ends (inside the stop,
/// or as it fails): the last partial chunk goes out then.
impl Drop for Sink {
    fn drop(&mut self) {
        if !self.discard.load(Ordering::SeqCst) {
            self.flush();
        }
    }
}

/// The machine's error for agg-mic's, with its message on the console.
fn mic_error(err: agg_mic::MicError) -> MicError {
    eprintln!("agg-gui-whistle: microphone: {err}");
    match err {
        agg_mic::MicError::PermissionDenied(_) => MicError::Denied,
        agg_mic::MicError::NoDevice(_) => MicError::NoDevice,
        other => MicError::Other(other.to_string()),
    }
}

struct Capture {
    session: u32,
    /// The sink's: set by a close that drops the last audio.
    discard: Arc<AtomicBool>,
    microphone: Arc<Microphone>,
}

/// Makes a session's microphone: the default input device, or a test's.
pub(super) type MakeMicrophone = Arc<dyn Fn() -> Microphone + Send + Sync>;

/// Opens and closes the session's microphone (see the module docs).
pub(super) struct Recorder {
    notifier: Notifier,
    make: MakeMicrophone,
    current: Option<Capture>,
}

impl Recorder {
    pub(super) fn new(notifier: Notifier) -> Recorder {
        Recorder::with_microphones(notifier, Arc::new(Microphone::default_input))
    }

    pub(super) fn with_microphones(notifier: Notifier, make: MakeMicrophone) -> Recorder {
        Recorder {
            notifier,
            make,
            current: None,
        }
    }

    pub(super) fn open(&mut self, session: u32, engine: Sender<ToWorker>) {
        if let Some(old) = self.current.take() {
            close_capture(old, false);
        }
        let discard = Arc::new(AtomicBool::new(false));
        let mut sink = Sink::new(session, engine, self.notifier.clone(), discard.clone());
        let microphone = Arc::new((self.make)());
        let notifier = self.notifier.clone();
        let failed = self.notifier.clone();
        let starting = microphone.clone();
        let spawned = std::thread::Builder::new()
            .name("agg-gui-whistle-mic".into())
            .spawn(move || {
                let started = starting.start(
                    Box::new(move |samples: &[f32], rate: u32| sink.feed(samples, rate)),
                    // agg-mic reports a recording's end at most once, never
                    // for a stop, and after the sink has sent what it holds.
                    Box::new(move |err| failed.send(Event::MicFailed(session, mic_error(err)))),
                );
                match started {
                    Ok(()) => notifier.send(Event::MicOpened(session)),
                    // Closed while it started: nobody is waiting for it.
                    Err(agg_mic::MicError::Stopped) => {}
                    Err(err) => notifier.send(Event::MicFailed(session, mic_error(err))),
                }
            });
        if let Err(err) = spawned {
            self.notifier
                .send(Event::MicFailed(session, MicError::Other(err.to_string())));
            return;
        }
        self.current = Some(Capture {
            session,
            discard,
            microphone,
        });
    }

    pub(super) fn close(&mut self, session: u32, flush: bool) {
        if self.current.as_ref().map(|c| c.session) == Some(session) {
            if let Some(capture) = self.current.take() {
                close_capture(capture, flush);
            }
        }
    }
}

/// Stops the session's microphone on the UI thread. agg-mic drops the
/// chunk handler inside the stop, so the sink's last partial chunk is sent
/// before this returns, unless `flush` is false (the session's audio is
/// thrown away).
fn close_capture(capture: Capture, flush: bool) {
    if !flush {
        capture.discard.store(true, Ordering::SeqCst);
    }
    let began = Instant::now();
    capture.microphone.stop();
    let took = began.elapsed();
    if took > SLOW_CLOSE {
        eprintln!(
            "agg-gui-whistle: stopping the microphone took {:.1} ms",
            took.as_secs_f64() * 1000.0
        );
    }
}

#[cfg(test)]
#[path = "mic_tests.rs"]
mod tests;
