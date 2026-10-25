//! The page's microphone for the web host (`mod.rs`): `getUserMedia`, then
//! an `AudioWorklet` (`web/capture-worklet.js`) or, where there is none, a
//! `ScriptProcessor`, giving mono f32 that is measured (the key's level) and
//! sent to the worker in ~250 ms `Push`es. Mirrors whistle-rust's demo
//! (`demo/web/src/microphone.ts`).
//!
//! A page may only start audio inside a user gesture (iOS Safari is strict:
//! a context made or resumed outside the tap's event handler stays
//! suspended). So the `AudioContext` is made inside the tap that called
//! `start`, asking for 16 kHz, which most browsers resample the microphone
//! to: by `open` when the microphone opens at once, or by [`warm_up`] when
//! the start waits for the model (the next `open` takes that context). It is
//! resumed again once `getUserMedia` resolves, and a context still
//! suspended then is a microphone error (`MicError::Suspended`, which a tap
//! retries) rather than listening to silence. Where the microphone can't
//! join a 16 kHz context (Firefox) it runs at the device rate and the
//! engine resamples. Permission is asked for on the first tap, not when the
//! keyboard comes up.
//!
//! A start the user stopped or cancelled while the permission prompt was up
//! is aborted: when the prompt resolves, the stream is released at once and
//! nothing is reported. Releasing a microphone clears its handlers before
//! they are dropped, closes the worklet's port, stops the tracks and closes
//! the context ([`Mic`]'s and [`Live`]'s `Drop`); a stop drains the worklet
//! first (`drain.rs`), so the audio it already posted reaches the engine.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use js_sys::{Array, Float32Array, Object, Promise, Reflect};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    AudioContext, AudioContextOptions, AudioContextState, AudioNode, AudioProcessingEvent,
    AudioWorkletNode, DomException, MediaStream, MediaStreamAudioSourceNode,
    MediaStreamConstraints, MediaStreamTrack, MessageEvent, MessagePort, ScriptProcessorNode,
};

use super::{dispatch, drain, post, Shared};
use crate::machine::MicError;
use crate::protocol::ToWorker;

/// Audio per push, in seconds.
const CHUNK_SECONDS: f64 = 0.25;
/// The rate asked of the `AudioContext`: the model's.
const MODEL_RATE: f32 = 16_000.0;
/// The `ScriptProcessor` fallback's buffer, in frames: 64 ms at 16 kHz.
/// Its partial buffer can't be flushed at a stop, so it is kept small.
const PROCESSOR_FRAMES: u32 = 1024;
/// How long a resume may take after `getUserMedia` before the context
/// counts as suspended, in ms.
const RESUME_MS: i32 = 1_000;

type PortHandler = Closure<dyn FnMut(MessageEvent)>;
type ProcessHandler = Closure<dyn FnMut(AudioProcessingEvent)>;

/// The microphone of one session. Dropping it releases everything.
pub(crate) struct Mic {
    pub(super) session: u32,
    ctx: AudioContext,
    /// Set once the stream is wired into the graph.
    live: Option<Live>,
    pub(super) chunker: Rc<RefCell<Chunker>>,
}

impl Drop for Mic {
    fn drop(&mut self) {
        // The graph and the tracks first, then the context they ran in.
        drop(self.live.take());
        let _ = self.ctx.close();
    }
}

impl Mic {
    /// The worklet's port, when the worklet captures.
    pub(super) fn worklet_port(&self) -> Option<&MessagePort> {
        match &self.live.as_ref()?.capture {
            Capture::Worklet { port, .. } => Some(port),
            Capture::Processor { .. } => None,
        }
    }

    /// Stop taking new audio (the graph's input and the tracks) while the
    /// capture node delivers what it has.
    pub(super) fn stop_input(&self) {
        if let Some(live) = &self.live {
            let _ = live.source.disconnect();
            stop_tracks(&live.stream);
        }
    }
}

/// How the graph hands audio to the chunker.
enum Capture {
    Worklet {
        port: MessagePort,
        _handler: PortHandler,
    },
    Processor {
        node: ScriptProcessorNode,
        _handler: ProcessHandler,
    },
}

/// The wired graph. Dropping it disconnects it, clears its handler before
/// the closure goes (a late quantum must not call a dropped closure) and
/// stops the tracks.
struct Live {
    stream: MediaStream,
    source: MediaStreamAudioSourceNode,
    node: AudioNode,
    capture: Capture,
}

impl Drop for Live {
    fn drop(&mut self) {
        let _ = self.source.disconnect();
        let _ = self.node.disconnect();
        match &self.capture {
            Capture::Worklet { port, .. } => {
                port.set_onmessage(None);
                port.close();
            }
            Capture::Processor { node, .. } => node.set_onaudioprocess(None),
        }
        stop_tracks(&self.stream);
    }
}

/// Collects samples into pushes and tracks the level.
pub(super) struct Chunker {
    shared: Weak<Shared>,
    session: u32,
    rate: u32,
    buffer: Vec<f32>,
    chunk: usize,
    level: f32,
}

impl Chunker {
    fn feed(&mut self, samples: &[f32]) {
        if samples.is_empty() {
            return;
        }
        let Some(shared) = self.shared.upgrade() else {
            return;
        };
        let power = samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32;
        // -55 dBFS (room noise) to -15 dBFS (close speech) fills the key;
        // it swells at once and settles over a few quanta.
        let db = 10.0 * (power + 1e-12).log10();
        let level = ((db + 55.0) / 40.0).clamp(0.0, 1.0);
        self.level = level.max(self.level * 0.9);
        let (session, level) = (self.session, self.level);
        if let Ok(mut machine) = shared.machine.try_borrow_mut() {
            machine.on_level(session, level);
        }
        self.buffer.extend_from_slice(samples);
        if self.buffer.len() >= self.chunk {
            self.flush(&shared);
        }
    }

    pub(super) fn flush(&mut self, shared: &Shared) {
        if self.buffer.is_empty() {
            return;
        }
        let samples = std::mem::take(&mut self.buffer);
        post(
            shared,
            &ToWorker::Push {
                session: self.session,
                rate: self.rate,
                samples,
            },
        );
    }
}

fn new_context(rate: Option<f32>) -> Option<AudioContext> {
    match rate {
        Some(rate) => {
            let options = AudioContextOptions::new();
            options.set_sample_rate(rate);
            AudioContext::new_with_context_options(&options).ok()
        }
        None => AudioContext::new().ok(),
    }
}

/// A context for the microphone, at the model's rate if it can be.
fn model_rate_context() -> Option<AudioContext> {
    new_context(Some(MODEL_RATE)).or_else(|| new_context(None))
}

/// Inside the tap of a start that waits for the model: make (or resume) the
/// context the microphone will use, while a gesture allows it to start.
pub(crate) fn warm_up(shared: &Shared) {
    let mut warm = shared.warm.borrow_mut();
    if warm.is_none() {
        *warm = model_rate_context();
    }
    if let Some(ctx) = warm.as_ref() {
        let _ = ctx.resume();
    }
}

/// Close the warmed-up context once nothing wants a microphone (the start
/// it was made for failed or was cancelled).
pub(crate) fn drop_unwanted_warm(shared: &Shared) {
    let wanted = shared
        .machine
        .try_borrow()
        .map(|m| m.wants_mic())
        .unwrap_or(true);
    if !wanted {
        let warm = shared.warm.borrow_mut().take();
        if let Some(ctx) = warm {
            let _ = ctx.close();
        }
    }
}

/// Open the microphone for `session` (the machine's `OpenMic`).
pub(crate) fn open(shared: &Rc<Shared>, session: u32) {
    let old = shared.mic.borrow_mut().take();
    drop(old);
    let warm = shared.warm.borrow_mut().take();
    let Some(ctx) = warm.or_else(model_rate_context) else {
        report_failure(shared, session, MicError::Unsupported);
        return;
    };
    // Inside the user's tap when the model was ready: allowed to start.
    let _ = ctx.resume();
    let chunker = Rc::new(RefCell::new(Chunker {
        shared: Rc::downgrade(shared),
        session,
        rate: ctx.sample_rate() as u32,
        buffer: Vec::new(),
        chunk: (ctx.sample_rate() as f64 * CHUNK_SECONDS) as usize,
        level: 0.0,
    }));
    *shared.mic.borrow_mut() = Some(Mic {
        session,
        ctx,
        live: None,
        chunker,
    });
    let weak = Rc::downgrade(shared);
    wasm_bindgen_futures::spawn_local(async move {
        let outcome = connect(&weak, session).await;
        let Some(shared) = weak.upgrade() else {
            return;
        };
        match outcome {
            Ok(Some(live)) => {
                let mut kept = Some(live);
                if let Some(mic) = shared.mic.borrow_mut().as_mut() {
                    if mic.session == session {
                        mic.live = kept.take();
                    }
                }
                if kept.is_some() {
                    // Aborted while wiring up: dropping it releases it.
                    return;
                }
                dispatch(&shared, &mut |m| m.on_mic_opened(session));
            }
            // Aborted while the prompt was up: already released.
            Ok(None) => return,
            Err(err) => {
                let current = shared.mic.borrow().as_ref().map(|m| m.session) == Some(session);
                if !current {
                    return;
                }
                let mic = shared.mic.borrow_mut().take();
                drop(mic);
                dispatch(&shared, &mut |m| m.on_mic_failed(session, err.clone()));
            }
        }
        agg_gui::animation::request_draw();
    });
}

/// Report a failure after the current dispatch (the machine is borrowed).
fn report_failure(shared: &Rc<Shared>, session: u32, err: MicError) {
    let shared = shared.clone();
    wasm_bindgen_futures::spawn_local(async move {
        dispatch(&shared, &mut |m| m.on_mic_failed(session, err.clone()));
        agg_gui::animation::request_draw();
    });
}

fn mic_error(err: &JsValue) -> MicError {
    match err.dyn_ref::<DomException>() {
        Some(e) => match e.name().as_str() {
            "NotAllowedError" | "SecurityError" | "PermissionDeniedError" => MicError::Denied,
            "NotFoundError" | "OverconstrainedError" | "DevicesNotFoundError" => MicError::NoDevice,
            name => MicError::Other(format!("{name}: {}", e.message())),
        },
        None => MicError::Other(format!("{err:?}")),
    }
}

/// The session's microphone, if it is still the one wanted.
fn wanted(weak: &Weak<Shared>, session: u32) -> Option<(Rc<Shared>, AudioContext)> {
    let shared = weak.upgrade()?;
    let ctx = shared
        .mic
        .borrow()
        .as_ref()
        .filter(|m| m.session == session)
        .map(|m| m.ctx.clone())?;
    Some((shared, ctx))
}

fn stop_tracks(stream: &MediaStream) {
    for track in stream.get_tracks().iter() {
        if let Ok(track) = track.dyn_into::<MediaStreamTrack>() {
            track.stop();
        }
    }
}

/// Resume `ctx` (again: the permission prompt may have suspended it) and
/// say whether it runs, giving the resume [`RESUME_MS`].
async fn runs(ctx: &AudioContext) -> bool {
    if ctx.state() == AudioContextState::Running {
        return true;
    }
    if let (Ok(resume), Some(window)) = (ctx.resume(), web_sys::window()) {
        let timeout = Promise::new(&mut |resolve, _| {
            let _ =
                window.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, RESUME_MS);
        });
        // A rejected resume is a context that doesn't run.
        let _ = JsFuture::from(Promise::race(&Array::of2(&resume, &timeout))).await;
    }
    ctx.state() == AudioContextState::Running
}

/// Ask for the microphone and wire it up. `Ok(None)`: the start was
/// aborted meanwhile, and the stream is released.
async fn connect(weak: &Weak<Shared>, session: u32) -> Result<Option<Live>, MicError> {
    let window = web_sys::window().ok_or(MicError::Unsupported)?;
    let devices = window
        .navigator()
        .media_devices()
        .map_err(|_| MicError::Unsupported)?;
    let constraints = MediaStreamConstraints::new();
    let audio = Object::new();
    for (key, value) in [
        ("channelCount", JsValue::from(1)),
        ("echoCancellation", JsValue::TRUE),
        ("noiseSuppression", JsValue::TRUE),
    ] {
        let _ = Reflect::set(&audio, &key.into(), &value);
    }
    constraints.set_audio(&audio);
    let promise = devices
        .get_user_media_with_constraints(&constraints)
        .map_err(|e| mic_error(&e))?;
    let stream: MediaStream = JsFuture::from(promise)
        .await
        .map_err(|e| mic_error(&e))?
        .unchecked_into();
    let Some((shared, mut ctx)) = wanted(weak, session) else {
        stop_tracks(&stream);
        return Ok(None);
    };
    let source = match ctx.create_media_stream_source(&stream) {
        Ok(source) => source,
        Err(_) => {
            // The microphone can't join a 16 kHz context: use its rate.
            //
            // This context is made after the `getUserMedia` await, outside
            // the tap. That is acceptable: only Firefox refuses a
            // microphone in a context at another rate, and Firefox lets a
            // page start audio once it has had a user gesture or holds the
            // microphone, both true here; iOS Safari, the browser that
            // needs the gesture, resamples the microphone into the 16 kHz
            // context and never comes here. A second, device-rate context
            // ready from the tap would run two contexts on every dictation
            // for this one path. Should a browser keep it suspended
            // anyway, `runs` below reports that rather than listening to
            // silence.
            let Some(device_rate) = new_context(None) else {
                stop_tracks(&stream);
                return Err(MicError::Unsupported);
            };
            let _ = device_rate.resume();
            if let Some(mic) = shared.mic.borrow_mut().as_mut() {
                let _ = mic.ctx.close();
                mic.ctx = device_rate.clone();
                let mut chunker = mic.chunker.borrow_mut();
                chunker.rate = device_rate.sample_rate() as u32;
                chunker.chunk = (device_rate.sample_rate() as f64 * CHUNK_SECONDS) as usize;
            }
            ctx = device_rate;
            ctx.create_media_stream_source(&stream).map_err(|e| {
                stop_tracks(&stream);
                mic_error(&e)
            })?
        }
    };
    let worklet_url = shared.worklet_url.clone();
    drop(shared);
    if !runs(&ctx).await {
        stop_tracks(&stream);
        return Err(MicError::Suspended);
    }
    // This session's chunker only: a newer session may hold the slot now.
    let Some(chunker) = weak.upgrade().and_then(|s| {
        s.mic
            .borrow()
            .as_ref()
            .filter(|m| m.session == session)
            .map(|m| m.chunker.clone())
    }) else {
        stop_tracks(&stream);
        return Ok(None);
    };
    let (node, capture) = match worklet_node(&ctx, &worklet_url, &chunker).await {
        Ok((node, port, handler)) => (
            node,
            Capture::Worklet {
                port,
                _handler: handler,
            },
        ),
        Err(why) => {
            warn_fallback(weak, &why);
            let (node, handler) = processor_node(&ctx, &chunker).map_err(|e| {
                stop_tracks(&stream);
                mic_error(&e)
            })?;
            (
                node.clone().unchecked_into(),
                Capture::Processor {
                    node,
                    _handler: handler,
                },
            )
        }
    };
    let live = Live {
        stream,
        source,
        node,
        capture,
    };
    if wanted(weak, session).is_none() {
        return Ok(None);
    }
    let wire = live
        .source
        .connect_with_audio_node(&live.node)
        // Some engines only pull a node that reaches the destination; it
        // outputs silence.
        .and_then(|_| live.node.connect_with_audio_node(&ctx.destination()));
    if let Err(err) = wire {
        return Err(mic_error(&err));
    }
    Ok(Some(live))
}

/// Say once per provider, in the console, that the worklet failed and why.
fn warn_fallback(weak: &Weak<Shared>, why: &str) {
    let Some(shared) = weak.upgrade() else {
        return;
    };
    if !shared.worklet_warned.replace(true) {
        web_sys::console::warn_1(
            &format!("agg-gui-whistle: no AudioWorklet ({why}); capturing with a ScriptProcessor")
                .into(),
        );
    }
}

fn js_text(err: &JsValue) -> String {
    err.as_string()
        .or_else(|| {
            Reflect::get(err, &"message".into())
                .ok()
                .and_then(|m| m.as_string())
        })
        .unwrap_or_else(|| format!("{err:?}"))
}

/// The worklet capture, where the browser has `AudioWorklet`; why not
/// otherwise.
async fn worklet_node(
    ctx: &AudioContext,
    url: &str,
    chunker: &Rc<RefCell<Chunker>>,
) -> Result<(AudioNode, MessagePort, PortHandler), String> {
    let worklet = ctx
        .audio_worklet()
        .map_err(|_| "this browser has none".to_string())?;
    let loading = worklet.add_module(url).map_err(|e| js_text(&e))?;
    JsFuture::from(loading)
        .await
        .map_err(|e| format!("{url} did not load: {}", js_text(&e)))?;
    let node = AudioWorkletNode::new(ctx, "agg-gui-whistle-capture").map_err(|e| js_text(&e))?;
    let port = node.port().map_err(|e| js_text(&e))?;
    let chunker = chunker.clone();
    let handler = Closure::<dyn FnMut(MessageEvent)>::new(move |e: MessageEvent| {
        let data = e.data();
        if let Some(samples) = data.dyn_ref::<Float32Array>() {
            chunker.borrow_mut().feed(&samples.to_vec());
        } else if data.as_string().as_deref() == Some(drain::FLUSHED) {
            let (shared, session) = {
                let c = chunker.borrow();
                (c.shared.clone(), c.session)
            };
            drain::drained(shared, session);
        }
    });
    port.set_onmessage(Some(handler.as_ref().unchecked_ref()));
    Ok((node.unchecked_into(), port, handler))
}

/// The `ScriptProcessor` fallback.
fn processor_node(
    ctx: &AudioContext,
    chunker: &Rc<RefCell<Chunker>>,
) -> Result<(ScriptProcessorNode, ProcessHandler), JsValue> {
    let node = ctx
        .create_script_processor_with_buffer_size_and_number_of_input_channels_and_number_of_output_channels(
            PROCESSOR_FRAMES, 1, 1,
        )?;
    let chunker = chunker.clone();
    let handler =
        Closure::<dyn FnMut(AudioProcessingEvent)>::new(move |e: AudioProcessingEvent| {
            if let Ok(buffer) = e.input_buffer() {
                if let Ok(samples) = buffer.get_channel_data(0) {
                    chunker.borrow_mut().feed(&samples);
                }
            }
        });
    node.set_onaudioprocess(Some(handler.as_ref().unchecked_ref()));
    Ok((node, handler))
}

/// Close the session's microphone (the machine's `CloseMic`). With `flush`
/// its last audio goes to the worker before the `Finish` the machine sends
/// next: a worklet is drained first (`drain.rs`, which holds that
/// `Finish` back until then), a `ScriptProcessor`'s buffered chunk goes at
/// once.
pub(crate) fn close(shared: &Rc<Shared>, session: u32, flush: bool) {
    let mic = {
        let mut slot = shared.mic.borrow_mut();
        match slot.as_ref() {
            Some(mic) if mic.session == session => slot.take(),
            _ => None,
        }
    };
    let Some(mic) = mic else {
        return;
    };
    if flush {
        if mic.worklet_port().is_some() {
            drain::start(shared, mic);
            return;
        }
        mic.chunker.borrow_mut().flush(shared);
    }
    drop(mic);
}
