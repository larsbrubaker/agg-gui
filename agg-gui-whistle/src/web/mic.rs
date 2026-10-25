//! The page's microphone for the web host (`mod.rs`): `getUserMedia`, then
//! an `AudioWorklet` (`web/capture-worklet.js`) or, where there is none, a
//! `ScriptProcessor`, giving mono f32 that is measured (the key's level) and
//! sent to the worker in ~250 ms `Push`es. Mirrors whistle-rust's demo
//! (`demo/web/src/microphone.ts`).
//!
//! The `AudioContext` is made at once, inside the tap that called `start`
//! (a page may only start audio from a user gesture), asking for 16 kHz,
//! which most browsers resample the microphone to; where the microphone
//! can't join a 16 kHz context (Firefox) it runs at the device rate and the
//! engine resamples. Permission is asked for then, on the first tap, not
//! when the keyboard comes up.
//!
//! A start the user stopped or cancelled while the permission prompt was up
//! is aborted: when the prompt resolves, the stream is released at once and
//! nothing is reported.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use js_sys::{Float32Array, Object, Reflect};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    AudioContext, AudioContextOptions, AudioNode, AudioProcessingEvent, AudioWorkletNode,
    DomException, MediaStream, MediaStreamAudioSourceNode, MediaStreamConstraints,
    MediaStreamTrack, MessageEvent,
};

use super::{dispatch, post, Shared};
use crate::machine::MicError;
use crate::protocol::ToWorker;

/// Audio per push, in seconds.
const CHUNK_SECONDS: f64 = 0.25;
/// The rate asked of the `AudioContext`: the model's.
const MODEL_RATE: f32 = 16_000.0;

type PortHandler = Closure<dyn FnMut(MessageEvent)>;
type ProcessHandler = Closure<dyn FnMut(AudioProcessingEvent)>;

/// The microphone of one session.
pub(crate) struct Mic {
    session: u32,
    ctx: AudioContext,
    /// Set once the stream is wired into the graph.
    live: Option<Live>,
    chunker: Rc<RefCell<Chunker>>,
}

/// The wired graph and the handler feeding the chunker.
struct Live {
    stream: MediaStream,
    source: MediaStreamAudioSourceNode,
    node: AudioNode,
    _port_handler: Option<PortHandler>,
    _process_handler: Option<ProcessHandler>,
}

/// Collects samples into pushes and tracks the level.
struct Chunker {
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

    fn flush(&mut self, shared: &Shared) {
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

/// Open the microphone for `session` (the machine's `OpenMic`).
pub(crate) fn open(shared: &Rc<Shared>, session: u32) {
    if let Some(old) = shared.mic.borrow_mut().take() {
        release(old, false, shared);
    }
    let Some(ctx) = new_context(Some(MODEL_RATE)).or_else(|| new_context(None)) else {
        report_failure(shared, session, MicError::Unsupported);
        return;
    };
    // Inside the user's tap: allowed to start.
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
                if let Some(live) = kept {
                    // Aborted while wiring up.
                    stop_live(&live);
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
                if let Some(mic) = shared.mic.borrow_mut().take() {
                    let _ = mic.ctx.close();
                }
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
    let chunker = match shared.mic.borrow().as_ref() {
        Some(mic) => mic.chunker.clone(),
        None => {
            stop_tracks(&stream);
            return Ok(None);
        }
    };
    let worklet_url = shared.worklet_url.clone();
    drop(shared);
    let live = match worklet_node(&ctx, &worklet_url, &chunker).await {
        Some((node, handler)) => Live {
            stream,
            source,
            node,
            _port_handler: Some(handler),
            _process_handler: None,
        },
        None => {
            let (node, handler) = processor_node(&ctx, &chunker).map_err(|e| {
                stop_tracks(&stream);
                mic_error(&e)
            })?;
            Live {
                stream,
                source,
                node,
                _port_handler: None,
                _process_handler: Some(handler),
            }
        }
    };
    if wanted(weak, session).is_none() {
        stop_live(&live);
        return Ok(None);
    }
    let wire = live
        .source
        .connect_with_audio_node(&live.node)
        // Some engines only pull a node that reaches the destination; it
        // outputs silence.
        .and_then(|_| live.node.connect_with_audio_node(&ctx.destination()));
    if let Err(err) = wire {
        stop_live(&live);
        return Err(mic_error(&err));
    }
    Ok(Some(live))
}

/// The worklet capture, where the browser has `AudioWorklet`.
async fn worklet_node(
    ctx: &AudioContext,
    url: &str,
    chunker: &Rc<RefCell<Chunker>>,
) -> Option<(AudioNode, PortHandler)> {
    let worklet = ctx.audio_worklet().ok()?;
    JsFuture::from(worklet.add_module(url).ok()?).await.ok()?;
    let node = AudioWorkletNode::new(ctx, "agg-gui-whistle-capture").ok()?;
    let chunker = chunker.clone();
    let handler = Closure::<dyn FnMut(MessageEvent)>::new(move |e: MessageEvent| {
        if let Some(samples) = e.data().dyn_ref::<Float32Array>() {
            chunker.borrow_mut().feed(&samples.to_vec());
        }
    });
    node.port()
        .ok()?
        .set_onmessage(Some(handler.as_ref().unchecked_ref()));
    Some((node.unchecked_into(), handler))
}

/// The `ScriptProcessor` fallback.
fn processor_node(
    ctx: &AudioContext,
    chunker: &Rc<RefCell<Chunker>>,
) -> Result<(AudioNode, ProcessHandler), JsValue> {
    let node = ctx
        .create_script_processor_with_buffer_size_and_number_of_input_channels_and_number_of_output_channels(
            4096, 1, 1,
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
    Ok((node.unchecked_into(), handler))
}

fn stop_live(live: &Live) {
    let _ = live.source.disconnect();
    let _ = live.node.disconnect();
    stop_tracks(&live.stream);
}

fn release(mic: Mic, flush: bool, shared: &Shared) {
    if flush {
        mic.chunker.borrow_mut().flush(shared);
    }
    if let Some(live) = &mic.live {
        stop_live(live);
    }
    let _ = mic.ctx.close();
}

/// Close the session's microphone (the machine's `CloseMic`); with `flush`
/// its last partial chunk goes to the worker first, before the `Finish`
/// the machine sends next.
pub(crate) fn close(shared: &Rc<Shared>, session: u32, flush: bool) {
    let mic = {
        let mut slot = shared.mic.borrow_mut();
        match slot.as_ref() {
            Some(mic) if mic.session == session => slot.take(),
            _ => None,
        }
    };
    if let Some(mic) = mic {
        release(mic, flush, shared);
    }
}
