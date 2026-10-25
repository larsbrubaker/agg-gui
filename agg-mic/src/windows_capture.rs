//! The Windows capture, over WASAPI through the `windows` crate. [`WindowsBackend`] is the
//! [`Backend`] `Microphone::default_input` picks on Windows.
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
//! From mattercad-ai's `audio/windows_recorder.rs`; its WAV and its level meter stay in
//! MatterCAD.

use std::ffi::c_void;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use windows::core::HRESULT;
use windows::Win32::Media::Audio::{
    eCapture, eConsole, IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator, MMDeviceEnumerator,
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
    AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY, WAVEFORMATEX, WAVE_FORMAT_PCM,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
    COINIT_MULTITHREADED,
};

use crate::delivery::Delivery;
use crate::microphone::Backend;
use crate::mixdown::{mono_from_bytes, sample_kind, SampleKind, WAVE_FORMAT_EXTENSIBLE};
use crate::{
    MicError, DISCONNECTED_MESSAGE, NO_INPUT_DEVICE_MESSAGE, WINDOWS_PERMISSION_DENIED_MESSAGE,
};

/// The rate the audio engine converts to.
const SAMPLE_RATE: u32 = 16_000;

// E_ACCESSDENIED: the privacy setting keeps desktop apps off the microphone.
const E_ACCESSDENIED: HRESULT = HRESULT(0x8007_0005_u32 as i32);
// E_NOTFOUND (HRESULT_FROM_WIN32(ERROR_NOT_FOUND)): no capture endpoint at all.
const E_NOTFOUND: HRESULT = HRESULT(0x8007_0490_u32 as i32);
// AUDCLNT_E_DEVICE_INVALIDATED: the microphone was unplugged.
const AUDCLNT_E_DEVICE_INVALIDATED: HRESULT = HRESULT(0x8889_0004_u32 as i32);

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
    // SAFETY: COM is initialized and uninitialized on this thread only, in pairs.
    let initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
    match open_stream() {
        Ok(stream) => {
            let _ = started.send(Ok(()));
            if let Err(error) = pump(shared, &stream, stop) {
                shared
                    .delivery
                    .fail(if error.code() == AUDCLNT_E_DEVICE_INVALIDATED {
                        MicError::NoDevice(DISCONNECTED_MESSAGE.to_owned())
                    } else {
                        MicError::Failed(error.message())
                    });
            }
            // The stop can return now; the rest is this thread's own teardown.
            let _ = drained.send(());
            // SAFETY: the client was started by `open_stream` on this thread.
            unsafe {
                let _ = stream.client.Stop();
            }
        }
        Err(error) => {
            let _ = started.send(Err(error));
        }
    }
    if initialized {
        // SAFETY: pairs the successful CoInitializeEx above.
        unsafe { CoUninitialize() };
    }
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
struct Stream {
    client: IAudioClient,
    capture: IAudioCaptureClient,
    wire: Wire,
}

// Opens the default microphone in shared mode, at 16 kHz mono 16-bit if the audio engine converts
// to it, else in the device's mix format, and starts it.
fn open_stream() -> Result<Stream, MicError> {
    // SAFETY: plain WASAPI calls on a COM-initialized thread; each format outlives the
    // Initialize it is passed to, and the mix format is freed with the allocator that made it.
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(start_error)?;
        let device = enumerator
            .GetDefaultAudioEndpoint(eCapture, eConsole)
            .map_err(start_error)?;
        let client: IAudioClient = device.Activate(CLSCTX_ALL, None).map_err(start_error)?;
        let converted = WAVEFORMATEX {
            wFormatTag: WAVE_FORMAT_PCM as u16,
            nChannels: 1,
            nSamplesPerSec: SAMPLE_RATE,
            nAvgBytesPerSec: SAMPLE_RATE * 2,
            nBlockAlign: 2,
            wBitsPerSample: 16,
            cbSize: 0,
        };
        let initialized = client.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
            BUFFER_DURATION_HNS,
            0,
            &converted,
            None,
        );
        let (client, wire) = match initialized {
            Ok(()) => (client, Wire::Converted),
            // A refusal is a refusal whatever the format.
            Err(error) if error.code() == E_ACCESSDENIED => return Err(start_error(error)),
            Err(error) => {
                eprintln!(
                    "agg-mic: WASAPI won't convert to 16 kHz ({}); recording the device's format",
                    error.message()
                );
                // A fresh client: one whose Initialize failed is not reused.
                let client: IAudioClient =
                    device.Activate(CLSCTX_ALL, None).map_err(start_error)?;
                let wire = initialize_mix_format(&client)?;
                (client, wire)
            }
        };
        let capture: IAudioCaptureClient = client.GetService().map_err(start_error)?;
        client.Start().map_err(start_error)?;
        Ok(Stream {
            client,
            capture,
            wire,
        })
    }
}

/// Initializes `client` in shared mode with the device's own mix format, if this crate can read
/// its samples.
///
/// # Safety
///
/// Called on a COM-initialized thread with a client that is not initialized yet.
unsafe fn initialize_mix_format(client: &IAudioClient) -> Result<Wire, MicError> {
    // SAFETY: GetMixFormat returns a WAVEFORMATEX (or a WAVEFORMATEXTENSIBLE, whose `cbSize` says
    // so) allocated with CoTaskMemAlloc, read unaligned (it is packed) and freed below.
    unsafe {
        let mix = client.GetMixFormat().map_err(start_error)?;
        let format = std::ptr::read_unaligned(mix);
        // WAVEFORMATEXTENSIBLE: the 18-byte WAVEFORMATEX, then Samples (2 bytes) and
        // dwChannelMask (4 bytes), then the SubFormat GUID at byte 24.
        let sub_format = (format.wFormatTag == WAVE_FORMAT_EXTENSIBLE && format.cbSize >= 22)
            .then(|| std::ptr::read_unaligned((mix as *const u8).add(24) as *const [u8; 16]));
        let wire = sample_kind(format.wFormatTag, format.wBitsPerSample, sub_format)
            .filter(|kind| {
                format.nChannels > 0
                    && usize::from(format.nBlockAlign)
                        == kind.size() * usize::from(format.nChannels)
            })
            .map(|kind| Wire::Mix {
                kind,
                channels: usize::from(format.nChannels),
                rate: format.nSamplesPerSec,
            });
        let initialized = match wire {
            Some(_) => client
                .Initialize(
                    AUDCLNT_SHAREMODE_SHARED,
                    0,
                    BUFFER_DURATION_HNS,
                    0,
                    mix,
                    None,
                )
                .map_err(start_error),
            None => Err(MicError::Failed(format!(
                "The microphone's format (tag {:#x}, {} bits) can't be read.",
                { format.wFormatTag },
                { format.wBitsPerSample }
            ))),
        };
        CoTaskMemFree(Some(mix as *const c_void));
        initialized?;
        wire.ok_or_else(|| MicError::Failed(NO_INPUT_DEVICE_MESSAGE.to_owned()))
    }
}

// Why the stream would not open, in words a user can act on.
fn start_error(error: windows::core::Error) -> MicError {
    let code = error.code();
    if code == E_ACCESSDENIED {
        MicError::PermissionDenied(WINDOWS_PERMISSION_DENIED_MESSAGE.to_owned())
    } else if code == E_NOTFOUND {
        MicError::NoDevice(NO_INPUT_DEVICE_MESSAGE.to_owned())
    } else {
        MicError::Failed(error.message())
    }
}

// Delivers every captured packet until asked to stop (woken at once, not at the next poll), then
// drains once more.
fn pump(shared: &Shared, stream: &Stream, stop: &Receiver<()>) -> windows::core::Result<()> {
    loop {
        drain(shared, stream)?;
        match stop.recv_timeout(POLL_INTERVAL) {
            Err(RecvTimeoutError::Timeout) => {}
            // Asked to stop (the sender dropped), or the backend gone.
            Ok(()) | Err(RecvTimeoutError::Disconnected) => return drain(shared, stream),
        }
    }
}

fn drain(shared: &Shared, stream: &Stream) -> windows::core::Result<()> {
    let frame_bytes = stream.wire.frame_bytes();
    // SAFETY: each GetBuffer is paired with ReleaseBuffer of the same frame count, and the
    // buffer is read (`frames` frames of the stream's format) before it is released.
    unsafe {
        while stream.capture.GetNextPacketSize()? > 0 {
            let mut data = std::ptr::null_mut();
            let mut frames = 0u32;
            let mut flags = 0u32;
            stream
                .capture
                .GetBuffer(&mut data, &mut frames, &mut flags, None, None)?;
            let silent = flags & (AUDCLNT_BUFFERFLAGS_SILENT.0 as u32) != 0 || data.is_null();
            let bytes: Vec<u8> = if silent {
                vec![0; frames as usize * frame_bytes]
            } else {
                std::slice::from_raw_parts(data, frames as usize * frame_bytes).to_vec()
            };
            stream.capture.ReleaseBuffer(frames)?;
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
    }
    Ok(())
}
