//! The macOS capture, over AVFoundation/AVFAudio through agg-os (`agg_os::macos::audio_input`,
//! which makes every Objective-C call): an AVAudioEngine's input node feeds an
//! AVAudioSinkNode, whose receiver block gets every I/O cycle's input (at the device's own rate,
//! one or more channels of 32-bit float in an AudioBufferList, which agg-os hands over as
//! slices); here each cycle is mixed to mono (`mixdown`) and delivered at that rate.
//! [`MacBackend`] is the [`Backend`] `Microphone::default_input` picks on macOS; this module keeps
//! the permission logic and its wording, the engine's lifetime and the fast stop.
//!
//! A sink node rather than a tap on the input node: a tap collects a buffer of the size it asks
//! for (4096 frames, about 85 ms at 48 kHz) before handing it over, and removing the tap discards
//! the one half filled, so a stop lost up to that much of the end (dictation clipped the last
//! word). The sink node hands over each I/O cycle as the hardware delivers it, so what a stop can
//! still lose is the cycle in flight: one I/O buffer, typically 512 frames (about 10 ms at
//! 48 kHz), as under cpal. `tests/live_mic.rs` measures it on a real microphone. The block runs
//! on the real-time audio thread, so it does only the mixing and the hand-on.
//!
//! A microphone that goes away while recording (unplugged, or the default input switched) stops
//! the engine, which agg-os's observer hears and `mac_engine_change` reports through
//! [`Delivery::fail`]. Dropping the [`InputEngine`] removes that observer first, so a stop is never
//! reported as a lost device, then stops the engine and detaches the sink node.
//!
//! **A fast stop.** `AVAudioEngine.stop()` and the detach take tens of milliseconds (a stop took
//! 46 ms on a Mac, then 18 ms, against agg-gui-whistle's UI-thread budget of well under a
//! frame), so [`MacBackend::stop`] removes the observer, hands the engine to a teardown thread
//! (`teardown`) and returns; the microphone then closes the delivery, so the receiver's later
//! calls deliver nothing. The next start waits for that teardown (off the UI thread) before it
//! builds a new engine. `AGG_MIC_TRACE=1` prints each step's time.
//!
//! **Permission.** macOS asks the user once (TCC) and remembers the answer; the crate docs say
//! what a bundled and an unbundled app need. A user who said no has to turn it back on in System
//! Settings, which [`MAC_PERMISSION_DENIED_MESSAGE`] says; a policy block (Screen Time, device
//! management) is [`MAC_RESTRICTED_MESSAGE`], which the user can't lift there.
//!
//! From mattercad-ai's `audio/mac_recorder.rs` (its permission handling, verified there on a real
//! microphone; it tapped the input node); its 16 kHz WAV and its level meter stay in MatterCAD.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::mpsc::{self, Sender};
use std::sync::{Mutex, PoisonError};
use std::time::Instant;

use agg_os::macos::audio_input::{
    self, AudioAuthorization, InputCycle, InputEngine, InputEngineError,
};

use crate::delivery::Delivery;
use crate::mac_engine_change::input_change_reporter;
use crate::microphone::Backend;
use crate::mixdown::mono_from_buffers;
use crate::teardown::Teardown;
use crate::{
    MicError, HANDLER_PANICKED_MESSAGE, MAC_PERMISSION_DENIED_MESSAGE, MAC_RESTRICTED_MESSAGE,
    NO_INPUT_DEVICE_MESSAGE,
};

/// AVAuthorizationStatus: the user has not been asked yet.
pub const AUTHORIZATION_NOT_DETERMINED: i64 = 0;
/// AVAuthorizationStatus: a policy (parental controls, MDM) blocks the microphone.
pub const AUTHORIZATION_RESTRICTED: i64 = 1;
/// AVAuthorizationStatus: the user said no.
pub const AUTHORIZATION_DENIED: i64 = 2;
/// AVAuthorizationStatus: the user allowed the microphone.
pub const AUTHORIZATION_AUTHORIZED: i64 = 3;

/// The current AVAuthorizationStatus for audio capture. Never prompts.
pub fn authorization_status() -> i64 {
    match audio_input::audio_authorization() {
        AudioAuthorization::NotDetermined => AUTHORIZATION_NOT_DETERMINED,
        AudioAuthorization::Restricted => AUTHORIZATION_RESTRICTED,
        AudioAuthorization::Denied => AUTHORIZATION_DENIED,
        AudioAuthorization::Authorized => AUTHORIZATION_AUTHORIZED,
        AudioAuthorization::Unknown(status) => status as i64,
    }
}

/// The system's default input device, as macOS names it (what a recording records from).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputDevice {
    /// Its name, such as "MacBook Pro Microphone".
    pub name: String,
    /// Whether it is built into the Mac (so it can't be unplugged).
    pub built_in: bool,
}

/// The default input device, or `None` when there is none. Never prompts for permission.
pub fn default_input_device() -> Option<InputDevice> {
    audio_input::default_audio_input().map(|device| InputDevice {
        name: device.name,
        built_in: device.built_in,
    })
}

/// Why a settled permission `status` keeps the microphone off, if it does.
fn refusal(status: i64) -> Option<MicError> {
    match status {
        AUTHORIZATION_AUTHORIZED => None,
        AUTHORIZATION_RESTRICTED => Some(MicError::PermissionDenied(
            MAC_RESTRICTED_MESSAGE.to_owned(),
        )),
        _ => Some(MicError::PermissionDenied(
            MAC_PERMISSION_DENIED_MESSAGE.to_owned(),
        )),
    }
}

// Everyone waiting on the one permission request in flight.
static PENDING_PERMISSION: Mutex<Option<Vec<Sender<bool>>>> = Mutex::new(None);

/// +[AVCaptureDevice requestAccessForMediaType:completionHandler:], waited for. Requests are
/// serialized: a second ask while one is pending waits on the same answer. The answer may never
/// come (the user can leave the prompt up and macOS has no timeout), which is why
/// `Microphone::stop` during a start closes its delivery and lets it finish on its own.
fn request_permission() -> bool {
    let (sender, answer) = mpsc::channel();
    let mut pending = PENDING_PERMISSION
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if let Some(waiting) = pending.as_mut() {
        waiting.push(sender);
        drop(pending);
    } else {
        *pending = Some(vec![sender]);
        drop(pending);
        // Called by AVFoundation on one of its own queues, not the thread that asked (or at once
        // with `false` when AVFoundation has no audio media type).
        audio_input::request_audio_access(|granted| {
            let answered = PENDING_PERMISSION
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take();
            for waiting in answered.into_iter().flatten() {
                let _ = waiting.send(granted);
            }
        });
    }
    answer.recv().unwrap_or(false)
}

/// The macOS capture (see the module docs).
#[derive(Default)]
pub(crate) struct MacBackend {
    engine: Mutex<Option<InputEngine>>,
    /// The last stopped engine's teardown, which the next start waits for.
    teardown: Teardown,
}

impl MacBackend {
    fn engine(&self) -> std::sync::MutexGuard<'_, Option<InputEngine>> {
        self.engine.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Backend for MacBackend {
    fn is_available(&self) -> bool {
        true
    }

    fn start(&self, delivery: Delivery) -> Result<(), MicError> {
        let mut status = authorization_status();
        if status == AUTHORIZATION_NOT_DETERMINED {
            status = if request_permission() {
                AUTHORIZATION_AUTHORIZED
            } else {
                AUTHORIZATION_DENIED
            };
        }
        if let Some(refused) = refusal(status) {
            return Err(refused);
        }
        // The last engine may still be stopping: a new one starts once it has (this runs off the
        // UI thread, so the wait holds up no frame).
        self.teardown.wait();
        let mut engine = self.engine();
        // An engine left by an earlier start that never stopped.
        drop(engine.take());
        *engine = Some(start_engine(delivery)?);
        Ok(())
    }

    /// Returns once the configuration observer is gone and the engine is handed off; the
    /// microphone then closes the delivery, so no chunk arrives after it. The engine stops on the
    /// teardown thread. Every chunk the receiver has delivered by then is the consumer's; the
    /// I/O cycle in flight (about 10 ms) is what a stop can lose.
    fn stop(&self) {
        let began = Instant::now();
        let Some(mut engine) = self.engine().take() else {
            return;
        };
        engine.unobserve();
        let unobserved = began.elapsed();
        let tracing = crate::tracing();
        self.teardown.hand_off(move || {
            let (stopped, detached) = engine.shut_down();
            if tracing {
                eprintln!(
                    "agg-mic trace: macOS teardown thread: engine stop {:.2} ms, detach {:.2} ms",
                    crate::ms(stopped),
                    crate::ms(detached)
                );
            }
        });
        if tracing {
            eprintln!(
                "agg-mic trace: macOS stop: observer removed {:.2} ms, engine handed off {:.2} ms",
                crate::ms(unobserved),
                crate::ms(began.elapsed() - unobserved)
            );
        }
    }
}

/// The error for an input that can't be recorded: no device, no usable format, or an engine that
/// won't start, all of which a user meets as "no microphone" (the detail goes to the console).
fn no_input(detail: &str) -> MicError {
    eprintln!("agg-mic: the microphone could not start: {detail}");
    MicError::NoDevice(NO_INPUT_DEVICE_MESSAGE.to_owned())
}

// Starts an engine whose input feeds a sink node delivering to `delivery`.
fn start_engine(delivery: Delivery) -> Result<InputEngine, MicError> {
    let audio = delivery.clone();
    InputEngine::start(
        move |cycle| deliver_cycle(&audio, cycle),
        input_change_reporter(delivery),
    )
    .map_err(|error: InputEngineError| no_input(&error.to_string()))
}

/// The receiver block's work, on the real-time audio thread: the cycle mixed to mono and handed
/// on at the input's rate. A panic must not unwind into the audio thread (an Objective-C block,
/// where it would abort the process): it ends the recording instead.
fn deliver_cycle(delivery: &Delivery, cycle: InputCycle<'_>) {
    let read = catch_unwind(AssertUnwindSafe(|| {
        let mono = mono_from_buffers(cycle.buffers, cycle.frames);
        delivery.deliver(&mono, cycle.sample_rate);
    }));
    if read.is_err() {
        delivery.fail(MicError::Failed(HANDLER_PANICKED_MESSAGE.to_owned()));
    }
}

#[cfg(test)]
#[path = "mac_capture_tests.rs"]
mod tests;
