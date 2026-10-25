//! The page's microphone for the web host (`mod.rs`): `getUserMedia`, then
//! an `AudioWorklet` (`web/capture-worklet.js`) or, where there is none, a
//! `ScriptProcessor`, giving mono f32 that is measured (the key's level) and
//! sent to the worker in ~250 ms `Push`es. Mirrors whistle-rust's demo
//! (`demo/web/src/microphone.ts`).
//!
//! A page may only start audio inside a user gesture (iOS Safari is strict:
//! a context made or resumed outside the tap's event handler stays
//! suspended). Every session opens its microphone inside the tap that
//! called `start`, whether or not the model is in yet: [`open`] makes the
//! `AudioContext` there, asking for 16 kHz, which most browsers resample
//! the microphone to, and starts the capture module's load
//! (`worklet_module.rs`); the dictation's one `getUserMedia` request
//! (`ask.rs`) follows in a microtask the tap queues, as soon as its handler
//! returns: still the tap's task, so the browser sees the gesture and the
//! permission prompt shows at once, but after the tap's own work, so the
//! device starts when the main thread is free to take its stream. Its stream is the one
//! the dictation uses: while the model loads, the audio goes to the worker
//! as usual and waits in its queue behind the load. The context is resumed
//! again once the graph is wired, and a context still suspended then is a
//! microphone error (`MicError::Suspended`, which a tap retries) rather
//! than listening to silence. Where the microphone can't join a 16 kHz
//! context (Firefox) it runs at the device rate and the engine resamples.
//! Only one `getUserMedia` is ever in flight on the page (`ask.rs`), and
//! after every await the session must still be the one wanted before the
//! microphone is touched.
//!
//! A started microphone's audio is lost until the graph consumes it, and a
//! user may speak the moment they tap. So the source and capture nodes are
//! made and connected the moment the stream arrives: the capture module,
//! prefetched when the provider was made and loaded from those bytes in the
//! tap, is normally in by then; a stream that arrives first waits for it at
//! most `worklet_module::MODULE_WAIT_MS`, then captures with a
//! `ScriptProcessor` instead. The context's resume check comes after the
//! wiring. On a busy main thread each await can take a second, and a graph
//! wired after several of them heard only the end of what was said, or
//! nothing.
//!
//! A start the user stopped or cancelled while the permission prompt was up
//! is aborted: when the prompt resolves, the stream is released at once and
//! nothing is reported. Releasing a microphone clears its handlers before
//! they are dropped, closes the worklet's port, stops the tracks and closes
//! the context ([`Mic`]'s and [`Live`]'s `Drop`); a stop drains the capture
//! node first (`drain.rs`), so the audio it already holds reaches the
//! engine.
//!
//! A `ScriptProcessor` hands its audio to the main thread from a double
//! buffer that the audio thread refills one buffer later, so an event the
//! main thread runs later than that reads newer audio than its own, and the
//! events that waited run back to back on the same buffers: shuffled and
//! repeated speech, which whistle reads as other words or a loop of them.
//! Its buffer is therefore the longest the browser allows
//! ([`PROCESSOR_FRAMES`]), however coarse that makes its pushes and level.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use js_sys::{Array, Float32Array, Promise, Reflect};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    AudioContext, AudioContextOptions, AudioContextState, AudioNode, AudioProcessingEvent,
    AudioWorkletNode, DomException, MediaStream, MediaStreamAudioSourceNode, MediaStreamTrack,
    MessageEvent, MessagePort, ScriptProcessorNode,
};

use super::{ask, dispatch, drain, post, worklet_module, Shared};
use crate::machine::MicError;
use crate::protocol::ToWorker;

/// Audio per push, in seconds.
const CHUNK_SECONDS: f64 = 0.25;
/// The rate asked of the `AudioContext`: the model's.
const MODEL_RATE: f32 = 16_000.0;
/// The `ScriptProcessor` fallback's buffer, in frames: the most the Web
/// Audio API allows, 1.024 s at 16 kHz. The main thread may run an event up
/// to this late before its audio is overwritten (see the module docs);
/// 1024 frames (64 ms) came out garbled on CI's busy main thread, which ran
/// about 800 ms behind. At a stop its partly filled buffer comes with its
/// next event ([`Mic::drain_processor`]).
const PROCESSOR_FRAMES: u32 = 16_384;
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

    /// At a stop, with a `ScriptProcessor` capturing: the buffer it is
    /// filling now is handed over at most one buffer's time from now, so
    /// its first event that late or later ends the drain (`drain.rs`).
    /// Returns that time, in ms.
    pub(super) fn drain_processor(&self) -> i32 {
        let period = f64::from(PROCESSOR_FRAMES) / f64::from(self.ctx.sample_rate());
        self.chunker.borrow_mut().drain_at = Some(self.ctx.current_time() + period);
        (period * 1000.0).ceil() as i32
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
    /// While a `ScriptProcessor` drains: the context time from which its
    /// next event ends the drain ([`Mic::drain_processor`]).
    drain_at: Option<f64>,
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
        let first = shared
            .machine
            .try_borrow_mut()
            .map(|mut machine| machine.on_level(session, level))
            .unwrap_or(false);
        if first {
            // The first audio turns the key to listening: show it now.
            agg_gui::animation::request_draw();
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

/// Open the microphone for `session` (the machine's `OpenMic`).
pub(crate) fn open(shared: &Rc<Shared>, session: u32) {
    let old = shared.mic.borrow_mut().take();
    drop(old);
    let Some(ctx) = model_rate_context() else {
        report_failure(shared, session, MicError::Unsupported);
        return;
    };
    // Inside the user's tap: allowed to start. The capture module's load
    // starts here too (`worklet_module.rs`), and the microphone request
    // with the session's task below (`connect`).
    let _ = ctx.resume();
    let module = worklet_module::start(&ctx, shared);
    let chunker = Rc::new(RefCell::new(Chunker {
        shared: Rc::downgrade(shared),
        session,
        rate: ctx.sample_rate() as u32,
        buffer: Vec::new(),
        chunk: (ctx.sample_rate() as f64 * CHUNK_SECONDS) as usize,
        level: 0.0,
        drain_at: None,
    }));
    *shared.mic.borrow_mut() = Some(Mic {
        session,
        ctx,
        live: None,
        chunker,
    });
    let weak = Rc::downgrade(shared);
    // A microtask: it runs as soon as the tap's handler returns, still in
    // the tap's task and gesture, so the request (its first step) goes out
    // after the tap's own work. Made mid-handler, the device would start
    // while that work still holds the main thread, and what the user says
    // meanwhile would be lost before the stream could be wired.
    wasm_bindgen_futures::spawn_local(async move {
        let outcome = connect(&weak, session, module).await;
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

pub(super) fn mic_error(err: &JsValue) -> MicError {
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

pub(super) fn stop_tracks(stream: &MediaStream) {
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

/// Ask for the microphone (at once: nothing is awaited before the request,
/// so it is made in the tap's task), then wire the stream up. `Ok(None)`:
/// the session was stopped or cancelled meanwhile (the stream released), or
/// a later session took the request over.
async fn connect(
    weak: &Weak<Shared>,
    session: u32,
    module: Promise,
) -> Result<Option<Live>, MicError> {
    let stream = {
        let Some(shared) = weak.upgrade() else {
            return Ok(None);
        };
        if wanted(weak, session).is_none() {
            return Ok(None);
        }
        ask::request(&shared, session)?
    };
    let answer = JsFuture::from(stream.clone()).await;
    if !ask::take_answer(&stream, weak, session) {
        // A later session asked meanwhile: the answer is its.
        return Ok(None);
    }
    let stream: MediaStream = answer.map_err(|e| mic_error(&e))?.unchecked_into();
    if wanted(weak, session).is_none() {
        stop_tracks(&stream);
        return Ok(None);
    }
    // The microphone is live: its graph is wired as soon as the module is
    // in (at once when it already is), or with a `ScriptProcessor` past the
    // wait (see the module docs).
    let module = worklet_module::ready_within(&module, worklet_module::MODULE_WAIT_MS).await;
    let Some((shared, mut ctx)) = wanted(weak, session) else {
        stop_tracks(&stream);
        return Ok(None);
    };
    // From here to the wiring, nothing is awaited: the microphone is live.
    let (source, module) = match ctx.create_media_stream_source(&stream) {
        Ok(source) => (source, module),
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
            // silence. The new context needs the capture module loaded
            // again: from the prefetched bytes it compiles in milliseconds,
            // the one wait this path keeps between the microphone starting
            // and its wiring (at most the module's wait, then a
            // `ScriptProcessor`).
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
            let source = ctx.create_media_stream_source(&stream).map_err(|e| {
                stop_tracks(&stream);
                mic_error(&e)
            })?;
            let module = match module {
                Ok(()) => {
                    let loading = worklet_module::start(&ctx, &shared);
                    worklet_module::ready_within(&loading, worklet_module::MODULE_WAIT_MS).await
                }
                Err(why) => Err(why),
            };
            if wanted(weak, session).is_none() {
                stop_tracks(&stream);
                return Ok(None);
            }
            (source, module)
        }
    };
    // This session's chunker only: a newer session may hold the slot now.
    let Some(chunker) = shared
        .mic
        .borrow()
        .as_ref()
        .filter(|m| m.session == session)
        .map(|m| m.chunker.clone())
    else {
        stop_tracks(&stream);
        return Ok(None);
    };
    drop(shared);
    let worklet = module.and_then(|()| worklet_node(&ctx, &chunker));
    let (node, capture) = match worklet {
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
    let wire = live
        .source
        .connect_with_audio_node(&live.node)
        // Some engines only pull a node that reaches the destination; it
        // outputs silence.
        .and_then(|_| live.node.connect_with_audio_node(&ctx.destination()));
    if let Err(err) = wire {
        return Err(mic_error(&err));
    }
    // Wired: now the context may be waited on (dropping `live` releases
    // the microphone).
    if !runs(&ctx).await {
        return Err(MicError::Suspended);
    }
    if wanted(weak, session).is_none() {
        return Ok(None);
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

pub(super) fn js_text(err: &JsValue) -> String {
    err.as_string()
        .or_else(|| {
            Reflect::get(err, &"message".into())
                .ok()
                .and_then(|m| m.as_string())
        })
        .unwrap_or_else(|| format!("{err:?}"))
}

/// The capture worklet's node, its module already loaded (`worklet_module.rs`):
/// made at once, so the microphone is wired the moment its stream arrives.
fn worklet_node(
    ctx: &AudioContext,
    chunker: &Rc<RefCell<Chunker>>,
) -> Result<(AudioNode, MessagePort, PortHandler), String> {
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

/// The `ScriptProcessor` fallback. After a stop, the first event
/// [`Mic::drain_processor`] waits for ends the drain, once fed.
fn processor_node(
    ctx: &AudioContext,
    chunker: &Rc<RefCell<Chunker>>,
) -> Result<(ScriptProcessorNode, ProcessHandler), JsValue> {
    let node = ctx
        .create_script_processor_with_buffer_size_and_number_of_input_channels_and_number_of_output_channels(
            PROCESSOR_FRAMES, 1, 1,
        )?;
    let chunker = chunker.clone();
    let ctx = ctx.clone();
    let handler =
        Closure::<dyn FnMut(AudioProcessingEvent)>::new(move |e: AudioProcessingEvent| {
            if let Ok(buffer) = e.input_buffer() {
                if let Ok(samples) = buffer.get_channel_data(0) {
                    chunker.borrow_mut().feed(&samples);
                }
            }
            let drained = {
                let mut c = chunker.borrow_mut();
                match c.drain_at {
                    Some(at) if ctx.current_time() >= at => {
                        c.drain_at = None;
                        Some((c.shared.clone(), c.session))
                    }
                    _ => None,
                }
            };
            if let Some((shared, session)) = drained {
                drain::drained(shared, session);
            }
        });
    node.set_onaudioprocess(Some(handler.as_ref().unchecked_ref()));
    Ok((node, handler))
}

/// Close the session's microphone (the machine's `CloseMic`). With `flush`
/// its last audio goes to the worker before the `Finish` the machine sends
/// next: a capture node is drained first (`drain.rs`, which holds that
/// `Finish` back until then).
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
        if mic.live.is_some() {
            drain::start(shared, mic);
            return;
        }
        // Not wired up yet: nothing is held but the chunker's audio.
        mic.chunker.borrow_mut().flush(shared);
    }
    drop(mic);
}
