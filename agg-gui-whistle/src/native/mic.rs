//! The native microphone (feature `native-mic`): a recorder thread per
//! session that owns a cpal input stream on the default device, mixes it to
//! mono and hands it to a [`Sink`], which sends ~250 ms `Push`es straight to
//! the engine thread at the device's rate (the engine resamples) and the
//! level to the UI thread.
//!
//! The thread owns the stream because a cpal stream may not move between
//! threads; it opens the device (which can take a while), reports
//! `MicOpened` or `MicFailed`, then waits to be told to stop. Closing takes
//! the sink's last partial chunk on the UI thread itself, so a stop's last
//! audio reaches the engine before the `Finish` the machine sends next.
//!
//! The OS asks for microphone permission when the stream opens (macOS
//! wants `NSMicrophoneUsageDescription` in the app bundle); cpal does not
//! say when it is refused, so a refused microphone listens to silence.

use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};

use super::{Event, Notifier};
use crate::machine::MicError;
use crate::protocol::ToWorker;

/// Audio per push, in seconds.
const CHUNK_SECONDS: f32 = 0.25;
/// Samples between level reports (at most ~20 a second at 48 kHz).
const LEVEL_EVERY: usize = 2_400;

/// Collects the recorder's samples into pushes.
struct Sink {
    session: u32,
    rate: u32,
    buffer: Vec<f32>,
    chunk: usize,
    engine: Sender<ToWorker>,
    notifier: Notifier,
    /// Cleared on close: late callbacks are dropped.
    open: bool,
    level: f32,
    since_level: usize,
}

impl Sink {
    fn feed(&mut self, mono: &[f32]) {
        if !self.open || mono.is_empty() {
            return;
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
        let _ = self.engine.send(ToWorker::Push {
            session: self.session,
            rate: self.rate,
            samples: std::mem::take(&mut self.buffer),
        });
    }
}

struct Capture {
    session: u32,
    sink: Arc<Mutex<Sink>>,
    /// Dropping it tells the recorder thread to close the stream.
    _stop: Sender<()>,
}

/// Opens and closes the session's microphone (see the module docs).
pub(super) struct Recorder {
    notifier: Notifier,
    current: Option<Capture>,
}

impl Recorder {
    pub(super) fn new(notifier: Notifier) -> Recorder {
        Recorder {
            notifier,
            current: None,
        }
    }

    pub(super) fn open(&mut self, session: u32, engine: Sender<ToWorker>) {
        if let Some(old) = self.current.take() {
            close_capture(old, false);
        }
        let sink = Arc::new(Mutex::new(Sink {
            session,
            rate: 0,
            buffer: Vec::new(),
            chunk: 0,
            engine,
            notifier: self.notifier.clone(),
            open: true,
            level: 0.0,
            since_level: 0,
        }));
        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        let notifier = self.notifier.clone();
        let thread_sink = sink.clone();
        let spawned = std::thread::Builder::new()
            .name("agg-gui-whistle-mic".into())
            .spawn(move || match open_stream(thread_sink) {
                Ok(stream) => {
                    notifier.send(Event::MicOpened(session));
                    // Until closed (the sender dropped).
                    let _ = stop_rx.recv();
                    drop(stream);
                }
                Err(err) => notifier.send(Event::MicFailed(session, err)),
            });
        if let Err(err) = spawned {
            self.notifier
                .send(Event::MicFailed(session, MicError::Other(err.to_string())));
            return;
        }
        self.current = Some(Capture {
            session,
            sink,
            _stop: stop_tx,
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

fn close_capture(capture: Capture, flush: bool) {
    if let Ok(mut sink) = capture.sink.lock() {
        if flush {
            sink.flush();
        }
        sink.open = false;
    }
    // Dropping `capture` drops the stop sender: the thread closes the stream.
}

/// Open the default input device and start feeding `sink`.
fn open_stream(sink: Arc<Mutex<Sink>>) -> Result<cpal::Stream, MicError> {
    let host = cpal::default_host();
    let device = host.default_input_device().ok_or(MicError::NoDevice)?;
    let supported = device
        .default_input_config()
        .map_err(|e| MicError::Other(e.to_string()))?;
    let config: cpal::StreamConfig = supported.config();
    let channels = config.channels.max(1) as usize;
    if let Ok(mut s) = sink.lock() {
        s.rate = config.sample_rate;
        s.chunk = (config.sample_rate as f32 * CHUNK_SECONDS) as usize;
    }
    let stream = match supported.sample_format() {
        SampleFormat::F32 => build::<f32>(&device, &config, channels, sink),
        SampleFormat::I16 => build::<i16>(&device, &config, channels, sink),
        SampleFormat::U16 => build::<u16>(&device, &config, channels, sink),
        SampleFormat::I32 => build::<i32>(&device, &config, channels, sink),
        other => Err(MicError::Other(format!(
            "unsupported sample format {other}"
        ))),
    }?;
    stream.play().map_err(|e| MicError::Other(e.to_string()))?;
    Ok(stream)
}

fn build<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: usize,
    sink: Arc<Mutex<Sink>>,
) -> Result<cpal::Stream, MicError>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let mut mono = Vec::new();
    device
        .build_input_stream(
            config,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                mono.clear();
                mono.extend(data.chunks(channels).map(|frame| {
                    frame.iter().map(|&s| s.to_sample::<f32>()).sum::<f32>() / channels as f32
                }));
                if let Ok(mut sink) = sink.lock() {
                    sink.feed(&mono);
                }
            },
            |err| eprintln!("agg-gui-whistle: microphone: {err}"),
            None,
        )
        .map_err(|e| MicError::Other(e.to_string()))
}
