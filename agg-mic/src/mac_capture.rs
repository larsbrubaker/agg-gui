//! The macOS capture, over the objc2 bindings to AVFoundation/AVFAudio: an AVAudioEngine's input
//! node feeds an AVAudioSinkNode, whose receiver block gets every I/O cycle's input (at the
//! device's own rate, one or more channels of 32-bit float in an AudioBufferList), mixes it to
//! mono (`mixdown`) and delivers it at that rate. [`MacBackend`] is the [`Backend`]
//! `Microphone::default_input` picks on macOS.
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
//! the engine, which `mac_engine_change` hears and reports through [`Delivery::fail`]. Dropping
//! the [`Engine`] removes that observer first, so a stop is never reported as a lost device, then
//! stops the engine and detaches the sink node.
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
use std::ptr::NonNull;
use std::sync::mpsc::{self, Sender};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use block2::RcBlock;
use objc2::rc::{autoreleasepool, Retained};
use objc2::runtime::Bool;
use objc2::AnyThread;
use objc2_av_foundation::{AVAuthorizationStatus, AVCaptureDevice, AVMediaTypeAudio};
use objc2_avf_audio::{AVAudioCommonFormat, AVAudioEngine, AVAudioSinkNode};
use objc2_core_audio_types::{AudioBufferList, AudioTimeStamp};

use crate::delivery::Delivery;
use crate::mac_engine_change::ChangeObserver;
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
    // SAFETY: reading an extern static AVFoundation defines.
    let Some(audio) = (unsafe { AVMediaTypeAudio }) else {
        return AUTHORIZATION_DENIED;
    };
    // SAFETY: AVMediaTypeAudio is one of the two media types the method documents.
    let status: AVAuthorizationStatus =
        unsafe { AVCaptureDevice::authorizationStatusForMediaType(audio) };
    status.0 as i64
}

/// kIOAudioDeviceTransportTypeBuiltIn (`'bltn'`): a device built into the Mac.
const TRANSPORT_BUILT_IN: i32 = 0x626C_746E;

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
    autoreleasepool(|_| {
        // SAFETY: reading an extern static AVFoundation defines.
        let audio = unsafe { AVMediaTypeAudio }?;
        // SAFETY: AVMediaTypeAudio is a media type the method documents; the device's name and
        // transport type are plain property reads.
        unsafe {
            let device = AVCaptureDevice::defaultDeviceWithMediaType(audio)?;
            Some(InputDevice {
                name: device.localizedName().to_string(),
                built_in: device.transportType() == TRANSPORT_BUILT_IN,
            })
        }
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
        // Called by AVFoundation on one of its own queues, not the thread that asked.
        let handler = RcBlock::new(|granted: Bool| {
            let answered = PENDING_PERMISSION
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take();
            for waiting in answered.into_iter().flatten() {
                let _ = waiting.send(granted.as_bool());
            }
        });
        // SAFETY: reading an extern static AVFoundation defines.
        match unsafe { AVMediaTypeAudio } {
            // SAFETY: a valid media type and a block of the documented `void (^)(BOOL)` shape;
            // AVFoundation copies the block.
            Some(audio) => unsafe {
                AVCaptureDevice::requestAccessForMediaType_completionHandler(audio, &handler);
            },
            None => handler.call((Bool::NO,)),
        }
    }
    answer.recv().unwrap_or(false)
}

/// The sink node's receiver block.
type Receiver = RcBlock<dyn Fn(NonNull<AudioTimeStamp>, u32, NonNull<AudioBufferList>) -> i32>;

/// A running engine: its sink node, the block that node calls, and the observer of its
/// configuration changes. AVAudioEngine is not marked thread-safe, so it is only ever touched
/// with the backend's lock held (start and stop, and this drop), never from two threads at once.
struct Engine {
    engine: Retained<AVAudioEngine>,
    sink: Retained<AVAudioSinkNode>,
    /// Kept alive as long as the node that calls it.
    _receiver: Receiver,
    /// Taken (removed) first when the engine stops.
    observer: Option<ChangeObserver>,
    /// Whether `shut_down` has run.
    shut: bool,
}

// SAFETY: an engine is used by one thread at a time: under `MacBackend::engine`'s mutex while it
// runs, then only by the teardown it is handed to. Objective-C objects may be retained and
// released from any thread, and the receiver block only holds a `Delivery` (Send + Sync).
unsafe impl Send for Engine {}

impl Engine {
    /// Stops listening for configuration changes, so a stop is never reported as a lost device.
    fn unobserve(&mut self) {
        drop(self.observer.take());
    }

    /// Stops the engine and detaches the sink node; returns how long each took. The slow part of
    /// a stop (tens of ms), so `MacBackend::stop` runs it on a teardown thread.
    fn shut_down(&mut self) -> (Duration, Duration) {
        self.unobserve();
        self.shut = true;
        // Reached from a teardown thread, which has no autorelease pool of its own.
        // SAFETY: the engine and node are ours; the node is attached to this engine.
        autoreleasepool(|_| unsafe {
            let began = Instant::now();
            self.engine.stop();
            let stopped = began.elapsed();
            self.engine.detachNode(&self.sink);
            (stopped, began.elapsed() - stopped)
        })
    }
}

impl Drop for Engine {
    /// An engine dropped unhanded (a start that failed after building it, a teardown thread that
    /// could not start) still stops.
    fn drop(&mut self) {
        if !self.shut {
            self.shut_down();
        }
    }
}

/// The macOS capture (see the module docs).
#[derive(Default)]
pub(crate) struct MacBackend {
    engine: Mutex<Option<Engine>>,
    /// The last stopped engine's teardown, which the next start waits for.
    teardown: Teardown,
}

impl MacBackend {
    fn engine(&self) -> std::sync::MutexGuard<'_, Option<Engine>> {
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
fn start_engine(delivery: Delivery) -> Result<Engine, MicError> {
    // Reached from a caller's thread, which may drain no autorelease pool of its own.
    // SAFETY: plain AVAudioEngine calls on an engine this function owns; the sink node's
    // preconditions are checked and stated below.
    autoreleasepool(|_| unsafe {
        let engine = AVAudioEngine::new();
        let input = engine.inputNode();
        let format = input.outputFormatForBus(0);
        // No input device leaves the bus without a format; connecting it would raise an
        // exception.
        if format.sampleRate() <= 0.0 || format.channelCount() == 0 {
            return Err(no_input("the input has no format (no input device)"));
        }
        // The receiver reads 32-bit floats: the input node's output is the engine's standard
        // format, which is.
        if format.commonFormat() != AVAudioCommonFormat::PCMFormatFloat32 {
            return Err(no_input("the input's format is not 32-bit float"));
        }
        // Device rates are whole numbers of Hz well inside u32.
        let rate = format.sampleRate().round() as u32;
        // Watching from before the start, so a change as it starts is heard too.
        let observer = ChangeObserver::watch(engine.as_ref(), delivery.clone());
        let receiver: Receiver = RcBlock::new(
            move |_when: NonNull<AudioTimeStamp>, frames: u32, input: NonNull<AudioBufferList>| {
                // A panic must not unwind into the audio thread (an Objective-C block, where it
                // would abort the process): it ends the recording instead.
                let read = catch_unwind(AssertUnwindSafe(|| {
                    // SAFETY: the engine hands the block a valid buffer list, consistent with
                    // `frames`, for the call's duration.
                    let mono = mono_from_buffer_list(input.as_ref(), frames);
                    delivery.deliver(&mono, rate);
                }));
                if read.is_err() {
                    delivery.fail(MicError::Failed(HANDLER_PANICKED_MESSAGE.to_owned()));
                }
                0
            },
        );
        // SAFETY: a block of the documented receiver shape; it is kept alive in the `Engine` as
        // long as the node.
        let sink = AVAudioSinkNode::initWithReceiverBlock(
            AVAudioSinkNode::alloc(),
            RcBlock::as_ptr(&receiver),
        );
        engine.attachNode(&sink);
        // SAFETY: both nodes are attached to this engine, and the format is the input node's own
        // output format, the one a sink node must be connected with (it converts nothing).
        engine.connect_to_format(&input, &sink, Some(&format));
        let running = Engine {
            engine,
            sink,
            _receiver: receiver,
            observer: Some(observer),
            shut: false,
        };
        running.engine.prepare();
        if let Err(error) = running.engine.startAndReturnError() {
            let detail = error.localizedDescription().to_string();
            drop(running);
            return Err(no_input(&detail));
        }
        Ok(running)
    })
}

/// The receiver block's input as mono: every buffer of `list` (one per channel when planar, one
/// with every channel when interleaved), read no further than its byte size says.
///
/// # Safety
///
/// `list` holds `mNumberBuffers` buffers, each with `mData` null or pointing at `mDataByteSize`
/// bytes of 32-bit floats, all valid for the call.
unsafe fn mono_from_buffer_list(list: &AudioBufferList, frames: u32) -> Vec<f32> {
    let first = list.mBuffers.as_ptr();
    let buffers: Vec<(&[f32], usize)> = (0..list.mNumberBuffers as usize)
        .map(|i| {
            // SAFETY: the list holds `mNumberBuffers` buffers, laid out after the first.
            let buffer = unsafe { &*first.add(i) };
            let samples: &[f32] = if buffer.mData.is_null() {
                &[]
            } else {
                let len = buffer.mDataByteSize as usize / std::mem::size_of::<f32>();
                // SAFETY: `mData` points at `mDataByteSize` bytes of floats (the caller's
                // promise).
                unsafe { std::slice::from_raw_parts(buffer.mData as *const f32, len) }
            };
            (samples, buffer.mNumberChannels as usize)
        })
        .collect();
    mono_from_buffers(&buffers, frames as usize)
}

#[cfg(test)]
#[path = "mac_capture_tests.rs"]
mod tests;
