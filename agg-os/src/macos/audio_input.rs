//! The AVFoundation side of agg-mic's macOS capture (feature `microphone`):
//! the microphone permission ([`audio_authorization`],
//! [`request_audio_access`]), the default input device
//! ([`default_audio_input`]), and an [`InputEngine`]: an AVAudioEngine whose
//! input node feeds an AVAudioSinkNode, whose receiver block hands every I/O
//! cycle's input to a closure as safe slices ([`InputCycle`]), and whose
//! configuration changes (`AVAudioEngineConfigurationChangeNotification`,
//! observed in `engine_change`) reach another closure.
//!
//! agg-mic (`mac_capture.rs`, `mac_engine_change.rs`) is the consumer: it
//! serializes permission requests, words the refusals, mixes each cycle to
//! mono (its `mixdown`), delivers it, and runs the slow teardown on a thread
//! of its own. What needs `unsafe` is here: the extern statics, the
//! AVFoundation calls, the blocks, and the read of the sink node's
//! AudioBufferList.
//!
//! Soundness: the sample slices are built here from the AudioBufferList the
//! engine hands the receiver block, each no longer than its buffer's byte
//! size says, as 32-bit floats because the engine is only started when the
//! input's format is 32-bit float; they live for the closure's call only.
//! Neither closure can unwind into CoreAudio or Foundation: each call runs
//! under `catch_unwind`, and a panic drops that cycle or that event. The
//! blocks live as long as what calls them (the receiver block in the
//! [`InputEngine`] beside its node, the observer's block in the notification
//! center until the observer is removed), and the closures are `Send + Sync`
//! because they run on, and may be released on, AVFoundation's threads.
//!
//! A closure may own its engine (an `Arc<Mutex<Option<InputEngine>>>`) and
//! drop it while it runs. Tearing the engine down there would release the
//! node, the blocks and so the closure state the running call borrows, and
//! Apple documents that an engine must not be torn down from inside its
//! configuration-change handler. So each call runs inside a `CallbackScope`
//! naming its engine and which of its two callbacks it is (`CallbackKind`)
//! on that thread. An engine shut down or dropped while one of its own
//! callbacks is running on that thread is left running and never released;
//! its observer is removed then only from inside the audio callback, and is
//! forgotten (left in place) from inside its own change callback, whose
//! block is the one running (see [`InputEngine`]).

mod engine_change;

use std::cell::Cell;
use std::fmt;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr::NonNull;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use block2::RcBlock;
use objc2::rc::{autoreleasepool, Retained};
use objc2::runtime::Bool;
use objc2::AnyThread;
use objc2_av_foundation::{AVAuthorizationStatus, AVCaptureDevice, AVMediaTypeAudio};
use objc2_avf_audio::{AVAudioCommonFormat, AVAudioEngine, AVAudioSinkNode};
use objc2_core_audio_types::{AudioBuffer, AudioBufferList, AudioTimeStamp};

use engine_change::ChangeObserver;

/// The next [`InputEngine`]'s id. Ids start at 1; 0 names no engine.
static NEXT_ENGINE_ID: AtomicU64 = AtomicU64::new(1);

/// Which of an [`InputEngine`]'s two callbacks a [`CallbackScope`] records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CallbackKind {
    /// The sink node's receiver block, calling `on_audio`.
    Audio,
    /// The observer's block, calling `on_configuration_change`.
    Change,
}

thread_local! {
    /// The engine and the callback of it (`on_audio` or
    /// `on_configuration_change`) running on this thread, innermost first;
    /// `None` when none is. A const `Cell` of plain `Copy` data: no
    /// allocation and no destructor, so the audio thread may read and write
    /// it.
    static RUNNING_CALLBACK: Cell<Option<(u64, CallbackKind)>> = const { Cell::new(None) };
}

/// While alive, records that `engine`'s `kind` callback is running on this
/// thread; dropping it (also when the callback unwinds) restores what was
/// recorded before, so nested scopes unwind correctly.
struct CallbackScope {
    previous: Option<(u64, CallbackKind)>,
}

impl CallbackScope {
    fn enter(engine: u64, kind: CallbackKind) -> CallbackScope {
        CallbackScope {
            previous: RUNNING_CALLBACK.replace(Some((engine, kind))),
        }
    }
}

impl Drop for CallbackScope {
    fn drop(&mut self) {
        RUNNING_CALLBACK.set(self.previous);
    }
}

/// Whether removing `engine`'s observer here would release the block whose
/// call is running: its change callback is the innermost [`CallbackScope`]
/// on this thread. [`InputEngine::unobserve`] then forgets the observer.
fn must_forget_observer(engine: u64) -> bool {
    RUNNING_CALLBACK.get() == Some((engine, CallbackKind::Change))
}

/// Whether tearing `engine` down here would free state a running call uses
/// (or tear it down inside its change handler, which Apple forbids): either
/// of its own callbacks is the innermost [`CallbackScope`] on this thread.
/// [`InputEngine::shut_down`] and its drop then leak it.
fn must_leak(engine: u64) -> bool {
    matches!(RUNNING_CALLBACK.get(), Some((running, _)) if running == engine)
}

/// The microphone permission (AVAuthorizationStatus for audio capture).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioAuthorization {
    /// The user has not been asked yet.
    NotDetermined,
    /// A policy (parental controls, device management) blocks the microphone.
    Restricted,
    /// The user said no.
    Denied,
    /// The user allowed the microphone.
    Authorized,
    /// A status this crate does not know (a later macOS), as AVFoundation
    /// gave it.
    Unknown(isize),
}

impl AudioAuthorization {
    /// The status AVFoundation's raw `AVAuthorizationStatus` value means.
    fn from_status(status: isize) -> AudioAuthorization {
        match AVAuthorizationStatus(status) {
            AVAuthorizationStatus::NotDetermined => Self::NotDetermined,
            AVAuthorizationStatus::Restricted => Self::Restricted,
            AVAuthorizationStatus::Denied => Self::Denied,
            AVAuthorizationStatus::Authorized => Self::Authorized,
            _ => Self::Unknown(status),
        }
    }
}

/// The current microphone permission. Never prompts. [`AudioAuthorization::Denied`]
/// when AVFoundation does not define its audio media type.
pub fn audio_authorization() -> AudioAuthorization {
    // SAFETY: reading an extern static AVFoundation defines.
    let Some(audio) = (unsafe { AVMediaTypeAudio }) else {
        return AudioAuthorization::Denied;
    };
    // SAFETY: AVMediaTypeAudio is one of the two media types the method documents.
    let status: AVAuthorizationStatus =
        unsafe { AVCaptureDevice::authorizationStatusForMediaType(audio) };
    AudioAuthorization::from_status(status.0)
}

/// `+[AVCaptureDevice requestAccessForMediaType:completionHandler:]` for
/// audio: asks the user (the first time only; macOS remembers the answer) and
/// returns at once. `answer` gets whether access is granted, once, on one of
/// AVFoundation's own queues, not the calling thread; it may never be called
/// (the user can leave the prompt up). When AVFoundation does not define its
/// audio media type, `answer` gets `false` on the calling thread, before this
/// returns. A panic in `answer` is contained.
pub fn request_audio_access(answer: impl FnOnce(bool) + Send + 'static) {
    let answer = Mutex::new(Some(answer));
    let handler = RcBlock::new(move |granted: Bool| {
        let answer = answer.lock().unwrap_or_else(PoisonError::into_inner).take();
        if let Some(answer) = answer {
            let _ = catch_unwind(AssertUnwindSafe(|| answer(granted.as_bool())));
        }
    });
    // SAFETY: reading an extern static AVFoundation defines.
    match unsafe { AVMediaTypeAudio } {
        // SAFETY: a valid media type and a block of the documented `void (^)(BOOL)` shape,
        // which is sendable (its closure holds a `Mutex` of a `Send` closure); AVFoundation
        // copies the block.
        Some(audio) => unsafe {
            AVCaptureDevice::requestAccessForMediaType_completionHandler(audio, &handler);
        },
        None => handler.call((Bool::NO,)),
    }
}

/// kIOAudioDeviceTransportTypeBuiltIn (`'bltn'`): a device built into the Mac.
const TRANSPORT_BUILT_IN: i32 = 0x626C_746E;

/// The system's default input device, as macOS names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioInputInfo {
    /// Its localized name, such as "MacBook Pro Microphone".
    pub name: String,
    /// Whether its transport type is built-in (so it can't be unplugged).
    pub built_in: bool,
}

/// The default audio input device, or `None` when there is none. Never
/// prompts for permission.
pub fn default_audio_input() -> Option<AudioInputInfo> {
    autoreleasepool(|_| {
        // SAFETY: reading an extern static AVFoundation defines.
        let audio = unsafe { AVMediaTypeAudio }?;
        // SAFETY: AVMediaTypeAudio is a media type the method documents; the device's name and
        // transport type are plain property reads.
        unsafe {
            let device = AVCaptureDevice::defaultDeviceWithMediaType(audio)?;
            Some(AudioInputInfo {
                name: device.localizedName().to_string(),
                built_in: device.transportType() == TRANSPORT_BUILT_IN,
            })
        }
    })
}

/// One I/O cycle of the input, as the sink node's receiver block got it.
#[derive(Debug, Clone, Copy)]
pub struct InputCycle<'a> {
    /// The AudioBufferList's buffers in order, each as its 32-bit float
    /// samples (no more than its byte size holds; empty for a buffer without
    /// data) and its channel count. Planar input has one buffer per channel;
    /// interleaved input has one buffer holding every channel.
    pub buffers: &'a [(&'a [f32], usize)],
    /// The frames the engine says the cycle holds. A buffer may hold fewer.
    pub frames: usize,
    /// The input's sample rate in Hz (the device's own), rounded.
    pub sample_rate: u32,
}

/// Why [`InputEngine::start`] failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputEngineError {
    /// The input node's output has no sample rate or no channels: there is
    /// no input device. (Connecting it would raise an exception.)
    NoInputFormat,
    /// The input node's output is not 32-bit float.
    NotFloat32,
    /// `-[AVAudioEngine startAndReturnError:]` failed; the error's localized
    /// description.
    StartFailed(String),
}

impl fmt::Display for InputEngineError {
    /// The detail for a console line; a start failure is its description alone.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoInputFormat => f.write_str("the input has no format (no input device)"),
            Self::NotFloat32 => f.write_str("the input's format is not 32-bit float"),
            Self::StartFailed(description) => f.write_str(description),
        }
    }
}

impl std::error::Error for InputEngineError {}

/// The sink node's receiver block.
type Receiver = RcBlock<dyn Fn(NonNull<AudioTimeStamp>, u32, NonNull<AudioBufferList>) -> i32>;

/// A running engine: its sink node, the block that node calls, and the
/// observer of its configuration changes. Dropping it stops it (removing the
/// observer first, then stopping the engine and detaching the node), as
/// [`InputEngine::shut_down`] does.
///
/// An engine shut down or dropped from inside one of its own callbacks (on
/// the thread that callback runs on) is not stopped: it is left running,
/// its audio callback keeps being called, and its resources are never
/// released, because tearing it down there would free state the running
/// call is using, and Apple documents that an engine must not be torn down
/// from inside its configuration-change handler. Callers hand the engine to
/// another thread to stop it, as agg-mic's teardown does. Its observer is
/// still removed from inside the audio callback (its block is not the one
/// running), and is left in place only from inside the change callback; so
/// [`InputEngine::unobserve`] from inside the audio callback removes it as
/// anywhere else.
///
/// `Send`, not `Sync`: every method takes `&mut self` or `self`, so one
/// thread at a time uses it; it may be started on one thread and shut down
/// on another.
pub struct InputEngine {
    /// Unique to this engine; the [`CallbackScope`] of its callbacks names it.
    id: u64,
    engine: Retained<AVAudioEngine>,
    sink: Retained<AVAudioSinkNode>,
    /// Kept alive as long as the node that calls it.
    _receiver: Receiver,
    /// Taken (removed) first when the engine stops.
    observer: Option<ChangeObserver>,
    /// Whether the engine has been stopped and its node detached.
    shut: bool,
}

// SAFETY: an InputEngine is only reached through `&mut self` and `self` (`unobserve`,
// `shut_down`, drop) and is not `Sync`, so it is used by one thread at a time, which is all
// AVAudioEngine (not thread-safe, but not bound to a thread) needs. Objective-C objects may be
// retained and released from any thread. The receiver block's closure is `Send + Sync`
// (`InputEngine::start` requires it), so releasing the block elsewhere is sound, and the
// observer token is only handed back to NSNotificationCenter, which is thread-safe.
unsafe impl Send for InputEngine {}

impl InputEngine {
    /// Builds and starts an engine whose input node feeds a sink node.
    ///
    /// `on_audio` runs on the real-time audio thread, once per I/O cycle the
    /// sink node receives, so it should only hand the samples on.
    /// `on_configuration_change` runs on the engine's internal queue each
    /// time the engine posts a configuration change (its input unplugged, the
    /// default input switched, its format changed; the engine has stopped
    /// itself); it must not touch the engine. Observing starts before the
    /// engine does, so a change as it starts is heard too. A panic in either
    /// is caught here: the cycle or the event is dropped.
    ///
    /// Either closure may own the engine, but an engine dropped or shut down
    /// from inside one of its own callbacks is not stopped: it is left
    /// running and its resources are never released, because tearing it
    /// down there would free state the running call is using (see
    /// [`InputEngine`]). Hand it to another thread to stop it. Calling
    /// [`Self::unobserve`] from `on_audio` removes the observer as anywhere
    /// else.
    pub fn start(
        on_audio: impl Fn(InputCycle<'_>) + Send + Sync + 'static,
        on_configuration_change: impl Fn() + Send + Sync + 'static,
    ) -> Result<InputEngine, InputEngineError> {
        // Reached from a caller's thread, which may drain no autorelease pool of its own.
        // SAFETY: plain AVAudioEngine calls on an engine this function owns; the sink node's
        // preconditions are checked and stated below.
        autoreleasepool(|_| unsafe {
            let id = NEXT_ENGINE_ID.fetch_add(1, Ordering::Relaxed);
            let engine = AVAudioEngine::new();
            let input = engine.inputNode();
            let format = input.outputFormatForBus(0);
            // No input device leaves the bus without a format; connecting it would raise an
            // exception.
            if format.sampleRate() <= 0.0 || format.channelCount() == 0 {
                return Err(InputEngineError::NoInputFormat);
            }
            // The receiver reads 32-bit floats: the input node's output is the engine's standard
            // format, which is.
            if format.commonFormat() != AVAudioCommonFormat::PCMFormatFloat32 {
                return Err(InputEngineError::NotFloat32);
            }
            // Device rates are whole numbers of Hz well inside u32.
            let rate = format.sampleRate().round() as u32;
            // Watching from before the start, so a change as it starts is heard too.
            let observer = ChangeObserver::watch(engine.as_ref(), id, on_configuration_change);
            let receiver = receiver_block(on_audio, rate, id);
            // SAFETY: a block of the documented receiver shape; it is kept alive in the
            // `InputEngine` as long as the node, and forever when the engine is dropped from
            // inside either of its own callbacks (`InputEngine::leak`), so it outlives every call.
            let sink = AVAudioSinkNode::initWithReceiverBlock(
                AVAudioSinkNode::alloc(),
                RcBlock::as_ptr(&receiver),
            );
            engine.attachNode(&sink);
            // SAFETY: both nodes are attached to this engine, and the format is the input node's
            // own output format, the one a sink node must be connected with (it converts
            // nothing).
            engine.connect_to_format(&input, &sink, Some(&format));
            let running = InputEngine {
                id,
                engine,
                sink,
                _receiver: receiver,
                observer: Some(observer),
                shut: false,
            };
            running.engine.prepare();
            if let Err(error) = running.engine.startAndReturnError() {
                let description = error.localizedDescription().to_string();
                drop(running);
                return Err(InputEngineError::StartFailed(description));
            }
            Ok(running)
        })
    }

    /// Stops listening for configuration changes, so a stop is never
    /// reported as one. Fast; [`Self::shut_down`] does it too. Anywhere,
    /// including inside this engine's audio callback, it removes the
    /// observer. Only from inside this engine's own change callback does it
    /// forget the observer instead, leaving it in place for good, because
    /// removing it would release the block whose call is running.
    pub fn unobserve(&mut self) {
        if must_forget_observer(self.id) {
            std::mem::forget(self.observer.take());
            return;
        }
        drop(self.observer.take());
    }

    /// Removes the observer, stops the engine and detaches the sink node, in
    /// that order; returns how long the stop and the detach took. The slow
    /// part of a stop (tens of milliseconds), which agg-mic runs on a
    /// teardown thread. Dropping the engine does the same.
    ///
    /// From inside either of this engine's own callbacks it stops and
    /// detaches nothing: the engine is left running and its resources are
    /// never released, because tearing it down there would free state the
    /// running call is using (and Apple forbids it inside the change
    /// handler); it returns zero durations. The observer is still removed
    /// from inside the audio callback, as [`Self::unobserve`] does.
    pub fn shut_down(mut self) -> (Duration, Duration) {
        if must_leak(self.id) {
            // The drop at the end of this call leaks the engine.
            return (Duration::ZERO, Duration::ZERO);
        }
        self.shut_down_in_place()
    }

    /// The teardown from inside one of this engine's own callbacks: nothing
    /// is stopped or detached, and an extra retain of the engine, the sink
    /// node and the receiver block is forgotten, so none of them (nor the
    /// closure they hold) is ever released and the running call keeps the
    /// state it borrows. The observer goes through [`Self::unobserve`]:
    /// removed inside the audio callback, forgotten inside the change
    /// callback. The fields' own drops that follow release only the retains
    /// this engine held.
    fn leak(&mut self) {
        std::mem::forget(self.engine.clone());
        std::mem::forget(self.sink.clone());
        std::mem::forget(self._receiver.clone());
        self.unobserve();
    }

    fn shut_down_in_place(&mut self) -> (Duration, Duration) {
        self.unobserve();
        self.shut = true;
        // Reached from a teardown thread, which has no autorelease pool of its own.
        // SAFETY: the engine and node are ours; the node is attached to this engine, and is
        // detached once only (`shut` is set, so the drop does not come back here).
        autoreleasepool(|_| unsafe {
            let began = Instant::now();
            self.engine.stop();
            let stopped = began.elapsed();
            self.engine.detachNode(&self.sink);
            (stopped, began.elapsed() - stopped)
        })
    }
}

impl Drop for InputEngine {
    /// An engine dropped without [`InputEngine::shut_down`] (a start that
    /// failed after building it, a teardown thread that could not start)
    /// still stops.
    ///
    /// An engine dropped from inside either of its own callbacks is not
    /// stopped: it is left running and its resources are never released
    /// (`InputEngine::leak`), because tearing it down there would free
    /// state the running call is using; its observer is removed only from
    /// inside the audio callback.
    fn drop(&mut self) {
        if must_leak(self.id) {
            self.leak();
            return;
        }
        if !self.shut {
            self.shut_down_in_place();
        }
    }
}

/// The sink node's receiver block: hands each cycle to `on_audio` at `rate`,
/// inside a [`CallbackScope`] naming `engine`'s audio callback.
fn receiver_block(
    on_audio: impl Fn(InputCycle<'_>) + Send + Sync + 'static,
    rate: u32,
    engine: u64,
) -> Receiver {
    RcBlock::new(
        move |_when: NonNull<AudioTimeStamp>, frames: u32, list: NonNull<AudioBufferList>| {
            // SAFETY: the engine hands the block the AudioBufferList it fills, valid and
            // unchanged for the call's duration, through a pointer covering all of its
            // `mNumberBuffers` buffers; they hold the input's format, 32-bit floats
            // (`InputEngine::start` checks it before connecting the node).
            unsafe { receive(list, frames, rate, engine, &on_audio) };
            0
        },
    )
}

/// One receiver call: `list`'s buffers as slices, handed to `on_audio`
/// inside a [`CallbackScope`] naming `engine`'s audio callback, so the
/// engine is not torn down under the call. A panic must not unwind into the audio thread (an
/// Objective-C block, where it would abort the process): it drops the
/// cycle, without logging or allocating here; the scope is restored as it
/// unwinds.
///
/// # Safety
///
/// `list` points at an AudioBufferList as [`buffers_from_list`] requires it,
/// valid for the call.
unsafe fn receive(
    list: NonNull<AudioBufferList>,
    frames: u32,
    sample_rate: u32,
    engine: u64,
    on_audio: &impl Fn(InputCycle<'_>),
) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: `list` is as `buffers_from_list` requires for this call (the caller's
        // promise); the slices' lifetime is bounded by `buffers`, a local of this closure,
        // and `on_audio` takes them for any lifetime, so it cannot keep them past the call.
        let buffers = unsafe { buffers_from_list(list) };
        let _scope = CallbackScope::enter(engine, CallbackKind::Audio);
        on_audio(InputCycle {
            buffers: &buffers,
            frames: frames as usize,
            sample_rate,
        });
    }));
}

/// Every buffer of `list` (one per channel when planar, one with every
/// channel when interleaved) as its samples and channel count, each read no
/// further than its byte size says; a buffer without data is empty. The
/// walk goes through `list` itself, never a reference to the list (which
/// would cover its first buffer only).
///
/// # Safety
///
/// `list` points at an AudioBufferList whose `mBuffers` is followed by its
/// other buffers, `mNumberBuffers` in all, and its provenance covers all of
/// them; each buffer's `mData` is null or points at `mDataByteSize` bytes of
/// 32-bit floats. The list and the samples stay valid and unwritten for
/// `'a`, which the caller bounds to the time they do (`receive`: the call).
unsafe fn buffers_from_list<'a>(list: NonNull<AudioBufferList>) -> Vec<(&'a [f32], usize)> {
    // SAFETY: `list` points at a valid AudioBufferList (the caller's promise); the count is
    // read through the pointer, creating no reference to the list.
    let count = unsafe { (*list.as_ptr()).mNumberBuffers } as usize;
    // SAFETY: the address of a field of the list `list` points at, taken without reading it or
    // making a reference, so it keeps `list`'s provenance over every buffer.
    let first = unsafe { core::ptr::addr_of!((*list.as_ptr()).mBuffers) }.cast::<AudioBuffer>();
    (0..count)
        .map(|i| {
            // SAFETY: `i < mNumberBuffers`, and the list's buffers lie one after another from
            // `first`, inside `list`'s provenance (the caller's promise), valid for `'a`.
            let buffer: &'a AudioBuffer = unsafe { &*first.add(i) };
            let samples: &[f32] = if buffer.mData.is_null() {
                &[]
            } else {
                let len = buffer.mDataByteSize as usize / std::mem::size_of::<f32>();
                // SAFETY: `mData` points at `mDataByteSize` bytes of floats, valid and unwritten
                // for `'a` (the caller's promise).
                unsafe { std::slice::from_raw_parts(buffer.mData as *const f32, len) }
            };
            (samples, buffer.mNumberChannels as usize)
        })
        .collect()
}

#[cfg(test)]
mod tests;
