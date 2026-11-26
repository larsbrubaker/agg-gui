//! The Windows capture, over WASAPI. [`WindowsBackend`] is the [`Backend`]
//! `Microphone::default_input` picks on Windows.
//!
//! A capture thread of its own opens the default capture endpoint in shared mode and asks the
//! audio engine to convert to 16 kHz mono 16-bit (`AUTOCONVERTPCM` with default-quality
//! resampling), then polls the capture buffer every 10 ms, delivering each packet as it arrives.
//! An engine that refuses the conversion (an older Windows, a driver that won't) gets the
//! device's own mix format instead (`GetMixFormat`: float or integer, any channel count), which
//! `mixdown` brings to mono and which is delivered at the device's rate.
//! COM objects stay on that thread; the backend shares only the delivery and two channels with it.
//! A stop wakes the thread at once (its poll waits on the stop channel, not a sleep), the thread
//! drains the buffer once more and says so, and the stop returns then: every packet captured is
//! delivered by that time, and the thread's slower teardown (`IAudioClient::Stop`,
//! `CoUninitialize`) goes on after the stop has returned. The wait is bounded ([`STOP_WAIT`]); a
//! drain is a few copies, so in practice it is well under a millisecond plus the wake-up. A start
//! right after needs nothing from the old thread: it opens a client of its own. A device that goes away
//! (`AUDCLNT_E_DEVICE_INVALIDATED`) or any other capture error ends the recording through
//! [`Delivery::fail`].
//!
//! **Permission.** Windows has no prompt for desktop apps: Settings > Privacy & security >
//! Microphone either lets them use the microphone or it doesn't, and when it doesn't, opening the
//! stream fails with "access denied", which becomes [`MicError::PermissionDenied`] with
//! [`WINDOWS_PERMISSION_DENIED_MESSAGE`].
//!
//! The COM and WASAPI calls are agg-os's (`agg_os::windows::audio_capture`, feature
//! `microphone`), where the workspace's unsafe code lives: COM on this thread is a
//! [`ComApartment`], and the stream a [`CaptureStream`] that opens the endpoint, reads each
//! packet's bytes and stops on drop. What is logic stays here: falling back to the mix format and
//! whether `mixdown` can read it, the bytes of a silent packet, the samples, the delivery, the
//! error words and the polling.
//!
//! From mattercad-ai's `audio/windows_recorder.rs`; its WAV and its level meter stay in
//! MatterCAD.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use agg_os::windows::audio_capture::{
    CaptureError, CaptureFormat, CaptureStream, ComApartment, MixFormat, OpenError,
};

use crate::delivery::Delivery;
use crate::microphone::Backend;
use crate::mixdown::{mono_from_bytes, sample_kind, SampleKind};
use crate::{
    MicError, DISCONNECTED_MESSAGE, NO_INPUT_DEVICE_MESSAGE, WINDOWS_PERMISSION_DENIED_MESSAGE,
};

/// The rate the audio engine converts to.
const SAMPLE_RATE: u32 = 16_000;

// E_ACCESSDENIED: the privacy setting keeps desktop apps off the microphone.
const E_ACCESSDENIED: i32 = 0x8007_0005_u32 as i32;
// E_NOTFOUND (HRESULT_FROM_WIN32(ERROR_NOT_FOUND)): no capture endpoint at all.
const E_NOTFOUND: i32 = 0x8007_0490_u32 as i32;
// AUDCLNT_E_DEVICE_INVALIDATED: the microphone was unplugged.
const AUDCLNT_E_DEVICE_INVALIDATED: i32 = 0x8889_0004_u32 as i32;

// The shared-mode buffer the engine fills between polls: 200 ms, in 100 ns units.
const BUFFER_DURATION_HNS: i64 = 2_000_000;
const POLL_INTERVAL: Duration = Duration::from_millis(10);
/// The longest a stop waits for the capture thread's last drain.
const STOP_WAIT: Duration = Duration::from_millis(200);

/// What the backend and its capture thread share.
struct Shared {
    delivery: Delivery,
}

/// The capture thread, as the backend holds it. Dropping it (unfinished) still stops the thread:
/// the stop channel disconnects.
struct Running {
    /// Dropped to stop the thread.
    stop: Option<Sender<()>>,
    /// Told once the thread's last drain is done (or disconnected when it ends).
    drained: Receiver<()>,
}

impl Running {
    // Wakes the capture thread and waits (at most STOP_WAIT) for its last drain; its teardown
    // goes on after.
    fn finish(mut self) {
        drop(self.stop.take());
        let _ = self.drained.recv_timeout(STOP_WAIT);
    }
}

/// The Windows capture (see the module docs).
#[derive(Default)]
pub(crate) struct WindowsBackend {
    running: Mutex<Option<Running>>,
}

impl WindowsBackend {
    fn take_running(&self) -> Option<Running> {
        self.running
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
    }
}

impl Backend for WindowsBackend {
    fn is_available(&self) -> bool {
        true
    }

    fn start(&self, delivery: Delivery) -> Result<(), MicError> {
        // A capture left by an earlier start that never stopped.
        if let Some(stale) = self.take_running() {
            stale.finish();
        }
        let shared = Shared { delivery };
        let (started, answer) = mpsc::channel();
        let (stop, stop_rx) = mpsc::channel();
        let (drained_tx, drained) = mpsc::channel();
        std::thread::Builder::new()
            .name("agg-mic-wasapi".to_owned())
            .spawn(move || capture_thread(&shared, started, &stop_rx, drained_tx))
            .map_err(|error| MicError::Failed(error.to_string()))?;
        let running = Running {
            stop: Some(stop),
            drained,
        };
        let result = answer
            .recv()
            .unwrap_or_else(|_| Err(MicError::Failed(NO_INPUT_DEVICE_MESSAGE.to_owned())));
        match result {
            Ok(()) => {
                *self.running.lock().unwrap_or_else(PoisonError::into_inner) = Some(running);
                Ok(())
            }
            Err(error) => {
                running.finish();
                Err(error)
            }
        }
    }

    fn stop(&self) {
        if let Some(running) = self.take_running() {
            running.finish();
        }
    }
}

// The capture thread: COM lives here from initialize to uninitialize.
fn capture_thread(
    shared: &Shared,
    started: Sender<Result<(), MicError>>,
    stop: &Receiver<()>,
    drained: Sender<()>,
) {
    // COM on this thread until the end of this function (uninitialized when it drops).
    let com = ComApartment::multithreaded();
    match open_stream(&com) {
        Ok(stream) => {
            let _ = started.send(Ok(()));
            if let Err(error) = pump(shared, &stream, stop) {
                shared
                    .delivery
                    .fail(if error.code() == AUDCLNT_E_DEVICE_INVALIDATED {
                        MicError::NoDevice(DISCONNECTED_MESSAGE.to_owned())
                    } else {
                        MicError::Failed(error.message().to_owned())
                    });
            }
            // The stop can return now; the rest is this thread's own teardown.
            let _ = drained.send(());
            // `IAudioClient::Stop`, then the stream's interfaces are released.
            drop(stream);
        }
        Err(error) => {
            let _ = started.send(Err(error));
        }
    }
    // CoUninitialize, if COM initialized above.
    drop(com);
}

/// What the capture buffer holds.
#[derive(Clone, Copy)]
enum Wire {
    /// 16 kHz mono 16-bit, converted by the audio engine.
    Converted,
    /// The device's own mix format, mixed down here (`mixdown`).
    Mix {
        kind: SampleKind,
        channels: usize,
        rate: u32,
    },
}

impl Wire {
    fn frame_bytes(self) -> usize {
        match self {
            Wire::Converted => 2,
            Wire::Mix { kind, channels, .. } => kind.size() * channels,
        }
    }
}

/// An open, started stream on this thread.
struct Stream<'com> {
    capture: CaptureStream<'com>,
    wire: Wire,
}

// Opens the default microphone in shared mode, at 16 kHz mono 16-bit if the audio engine converts
// to it, else in the device's mix format, and starts it.
fn open_stream(com: &ComApartment) -> Result<Stream<'_>, MicError> {
    let mut mix_wire = None;
    let capture = CaptureStream::open_default(
        com,
        SAMPLE_RATE,
        BUFFER_DURATION_HNS,
        |error| {
            // A refusal is a refusal whatever the format.
            if error.code() == E_ACCESSDENIED {
                return false;
            }
            eprintln!(
                "agg-mic: WASAPI won't convert to 16 kHz ({}); recording the device's format",
                error.message()
            );
            true
        },
        |format| {
            mix_wire = mix_wire_of(format);
            mix_wire.is_some()
        },
    )
    .map_err(|error| match error {
        OpenError::MixRejected(format) => MicError::Failed(format!(
            "The microphone's format (tag {:#x}, {} bits) can't be read.",
            format.format_tag, format.bits_per_sample
        )),
        OpenError::Call(error) => start_error(&error),
    })?;
    let wire = match capture.format() {
        CaptureFormat::Converted => Wire::Converted,
        CaptureFormat::Mix(_) => {
            mix_wire.ok_or_else(|| MicError::Failed(NO_INPUT_DEVICE_MESSAGE.to_owned()))?
        }
    };
    Ok(Stream { capture, wire })
}

/// How this crate reads the device's own mix format, if it can: a sample kind `mixdown` reads,
/// at least one channel, and frames of exactly one sample per channel.
fn mix_wire_of(format: &MixFormat) -> Option<Wire> {
    sample_kind(format.format_tag, format.bits_per_sample, format.sub_format)
        .filter(|kind| {
            format.channels > 0
                && usize::from(format.block_align) == kind.size() * usize::from(format.channels)
        })
        .map(|kind| Wire::Mix {
            kind,
            channels: usize::from(format.channels),
            rate: format.samples_per_sec,
        })
}

// Why the stream would not open, in words a user can act on.
fn start_error(error: &CaptureError) -> MicError {
    let code = error.code();
    if code == E_ACCESSDENIED {
        MicError::PermissionDenied(WINDOWS_PERMISSION_DENIED_MESSAGE.to_owned())
    } else if code == E_NOTFOUND {
        MicError::NoDevice(NO_INPUT_DEVICE_MESSAGE.to_owned())
    } else {
        MicError::Failed(error.message().to_owned())
    }
}

// Delivers every captured packet until asked to stop (woken at once, not at the next poll), then
// drains once more.
fn pump(shared: &Shared, stream: &Stream<'_>, stop: &Receiver<()>) -> Result<(), CaptureError> {
    loop {
        drain(shared, stream)?;
        match stop.recv_timeout(POLL_INTERVAL) {
            Err(RecvTimeoutError::Timeout) => {}
            // Asked to stop (the sender dropped), or the backend gone.
            Ok(()) | Err(RecvTimeoutError::Disconnected) => return drain(shared, stream),
        }
    }
}

// Delivers every packet waiting in the capture buffer.
fn drain(shared: &Shared, stream: &Stream<'_>) -> Result<(), CaptureError> {
    let frame_bytes = stream.wire.frame_bytes();
    loop {
        let mut bytes: Vec<u8> = Vec::new();
        let Some(packet) = stream.capture.next_packet(&mut bytes)? else {
            break;
        };
        if packet.silent {
            bytes = vec![0; packet.frames as usize * frame_bytes];
        }
        match stream.wire {
            // The engine already converted to 16 kHz mono, so the packet goes on as it is.
            Wire::Converted => {
                let packet: Vec<i16> = bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|pair| i16::from_le_bytes(*pair))
                    .collect();
                shared.delivery.deliver_pcm16(&packet, SAMPLE_RATE);
            }
            // The device's rate goes with the chunk; the consumer resamples.
            Wire::Mix {
                kind,
                channels,
                rate,
            } => {
                let mono = mono_from_bytes(&bytes, kind, channels);
                shared.delivery.deliver(&mono, rate);
            }
        }
    }
    Ok(())
}
