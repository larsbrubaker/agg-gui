//! The web host (wasm, feature `provider`): runs the state machine's
//! commands with a dedicated Web Worker for the engine and the page's
//! microphone (`mic.rs`), all on the UI thread.
//!
//! The worker (`web/worker.js` and the `agg-gui-whistle-worker` module,
//! `worker/`) starts with the first `Load`, so a page that never shows the
//! keyboard never downloads anything. Its replies arrive in `onmessage`,
//! go to the machine, and wake the app with `agg_gui::animation::request_draw`.
//! A worker that fails to start (its script or module missing) is dropped
//! and started again by the next `Load`, so the keyboard's next show
//! retries.
//!
//! Platforms that can't do this at all (no `getUserMedia`, no module
//! workers, no WebAssembly SIMD for the model's kernels) get a machine that
//! says so when the key is tapped (an error without retry, never before a
//! tap) and never start a worker.
//!
//! A provider dropped without a cancel (the app registering another, or
//! none) releases everything at once: the worker is terminated, the
//! microphone's tracks stopped and its `AudioContext` closed (`Drop` for
//! [`WebHost`] and [`Shared`], and for the microphone in `mic.rs`).

mod drain;
mod mic;

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use agg_gui::widgets::on_screen_keyboard::VoiceContext;
use js_sys::{Reflect, Uint8Array, WebAssembly};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{AudioContext, ErrorEvent, Event, MessageEvent, Worker, WorkerOptions, WorkerType};

use crate::config::Config;
use crate::drain_book::DrainBook;
use crate::js_fields::{from_object, to_object};
use crate::machine::{Command, Machine};
use crate::protocol::{FromWorker, LoadFailure, ToWorker};
use crate::provider::Host;

const NO_AUDIO: &str = "This browser can't record audio";
const NO_ENGINE: &str = "This browser can't run speech recognition";

/// A running worker and the handlers it calls. Dropping it terminates the
/// worker, its handlers cleared first.
struct WorkerHandle {
    worker: Worker,
    _on_message: Closure<dyn FnMut(MessageEvent)>,
    _on_error: Closure<dyn FnMut(Event)>,
}

impl Drop for WorkerHandle {
    fn drop(&mut self) {
        self.worker.set_onmessage(None);
        self.worker.set_onerror(None);
        self.worker.terminate();
    }
}

pub(crate) struct Shared {
    machine: RefCell<Machine>,
    worker: RefCell<Option<WorkerHandle>>,
    mic: RefCell<Option<mic::Mic>>,
    /// A stopped microphone delivering its last audio (`drain.rs`).
    drain: RefCell<DrainBook<drain::Draining>>,
    /// The context a tap made for a start that waits for the model
    /// (`mic::warm_up`).
    warm: RefCell<Option<AudioContext>>,
    /// The console has been told the worklet failed.
    worklet_warned: Cell<bool>,
    /// Absolute URLs.
    worker_url: String,
    worklet_url: String,
}

impl Shared {
    /// Release the worker, the microphone and the audio, now.
    fn shut_down(&self) {
        let taken = (
            self.drain.try_borrow_mut().ok().and_then(|mut d| d.take()),
            self.mic.try_borrow_mut().ok().and_then(|mut m| m.take()),
            self.warm.try_borrow_mut().ok().and_then(|mut w| w.take()),
            self.worker.try_borrow_mut().ok().and_then(|mut w| w.take()),
        );
        if let Some(ctx) = &taken.2 {
            let _ = ctx.close();
        }
        drop(taken);
    }
}

impl Drop for Shared {
    fn drop(&mut self) {
        self.shut_down();
    }
}

/// See the module docs.
pub(crate) struct WebHost {
    shared: Rc<Shared>,
}

impl Drop for WebHost {
    /// Futures still in flight may hold the shared state a little longer;
    /// what it owns goes now.
    fn drop(&mut self) {
        self.shared.shut_down();
    }
}

/// `url` resolved against the page.
fn absolute(url: &str) -> String {
    let base = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.base_uri().ok().flatten());
    match base {
        Some(base) => web_sys::Url::new_with_base(url, &base)
            .map(|u| u.href())
            .unwrap_or_else(|_| url.to_string()),
        None => url.to_string(),
    }
}

/// Why this browser can't take dictation, if it can't.
fn unsupported() -> Option<&'static str> {
    let window = web_sys::window()?;
    let devices = Reflect::get(&window.navigator(), &"mediaDevices".into()).ok()?;
    let has_gum = !devices.is_undefined()
        && Reflect::get(&devices, &"getUserMedia".into())
            .map(|f| f.is_function())
            .unwrap_or(false);
    if !has_gum {
        // Also every page served over plain HTTP: no secure context.
        return Some(NO_AUDIO);
    }
    let has_worker = Reflect::get(&window, &"Worker".into())
        .map(|w| w.is_function())
        .unwrap_or(false);
    // The smallest module using a v128 instruction (wasm-feature-detect's
    // SIMD probe).
    const SIMD_PROBE: &[u8] = &[
        0, 97, 115, 109, 1, 0, 0, 0, 1, 5, 1, 96, 0, 1, 123, 3, 2, 1, 0, 10, 10, 1, 8, 0, 65, 0,
        253, 15, 253, 98, 11,
    ];
    let has_simd = WebAssembly::validate(&Uint8Array::from(SIMD_PROBE).into()).unwrap_or(false);
    if !has_worker || !has_simd {
        return Some(NO_ENGINE);
    }
    None
}

impl WebHost {
    pub(crate) fn new(config: Config) -> WebHost {
        let machine = match unsupported() {
            Some(reason) => Machine::unsupported(reason),
            None => Machine::new(
                config.model_urls.iter().map(|u| absolute(u)).collect(),
                config.language.clone(),
            ),
        };
        WebHost {
            shared: Rc::new(Shared {
                machine: RefCell::new(machine),
                worker: RefCell::new(None),
                mic: RefCell::new(None),
                drain: RefCell::new(DrainBook::default()),
                warm: RefCell::new(None),
                worklet_warned: Cell::new(false),
                worker_url: absolute(&config.worker_url),
                worklet_url: absolute(&config.worklet_url),
            }),
        }
    }
}

impl Host for WebHost {
    fn dispatch(&self, f: &mut dyn FnMut(&mut Machine)) {
        dispatch(&self.shared, f);
    }

    /// Inside the tap: a start that waits for the model gets its audio
    /// context now, while the gesture lets it start (`mic.rs`).
    fn start(&self, ctx: &VoiceContext) {
        dispatch(&self.shared, &mut |m| m.start(ctx));
        let pending = self
            .shared
            .machine
            .try_borrow()
            .map(|m| m.start_pending())
            .unwrap_or(false);
        if pending {
            mic::warm_up(&self.shared);
        }
    }
}

/// Apply `f` to the machine, then run what it queued. Callbacks (worker
/// replies, the microphone) come through here too.
pub(crate) fn dispatch(shared: &Rc<Shared>, f: &mut dyn FnMut(&mut Machine)) {
    let commands = {
        let Ok(mut machine) = shared.machine.try_borrow_mut() else {
            // Re-entered from inside a command (cannot happen: commands only
            // post and spawn); drop rather than panic.
            return;
        };
        f(&mut machine);
        machine.take_commands()
    };
    for command in commands {
        run(shared, command);
    }
    mic::drop_unwanted_warm(shared);
}

fn run(shared: &Rc<Shared>, command: Command) {
    match command {
        Command::Worker(msg) => {
            if matches!(msg, ToWorker::Load { .. }) {
                ensure_worker(shared);
            }
            if drain::hold(shared, &msg) {
                return;
            }
            post(shared, &msg);
        }
        Command::OpenMic { session } => mic::open(shared, session),
        Command::CloseMic { session, flush } => mic::close(shared, session, flush),
    }
}

/// Post `msg` to the worker, transferring its audio.
pub(crate) fn post(shared: &Shared, msg: &ToWorker) {
    let worker = shared.worker.borrow();
    let Some(handle) = worker.as_ref() else {
        return;
    };
    let (object, transfer) = to_object(msg.to_fields());
    if let Err(err) = handle.worker.post_message_with_transfer(&object, &transfer) {
        web_sys::console::error_2(&"agg-gui-whistle: post failed".into(), &err);
    }
}

fn on_reply(weak: &Weak<Shared>, data: JsValue) {
    let Some(shared) = weak.upgrade() else {
        return;
    };
    let msg = match from_object(&data, FromWorker::from_fields) {
        Ok(msg) => msg,
        Err(err) => {
            web_sys::console::error_1(&err.to_string().into());
            return;
        }
    };
    if matches!(
        msg,
        FromWorker::LoadFailed {
            reason: LoadFailure::Startup,
            ..
        }
    ) {
        drop_worker(&shared);
    }
    let mut msg = Some(msg);
    dispatch(&shared, &mut |m| {
        if let Some(msg) = msg.take() {
            m.on_worker(msg);
        }
    });
    agg_gui::animation::request_draw();
}

/// The worker failed: its script did not load (a plain `Event`) or threw
/// (an `ErrorEvent`).
fn on_worker_error(weak: &Weak<Shared>, event: Event) {
    let Some(shared) = weak.upgrade() else {
        return;
    };
    // Stop the browser's own "uncaught error" report; ours follows.
    event.prevent_default();
    drop_worker(&shared);
    let detail = match event.dyn_ref::<ErrorEvent>() {
        Some(e) => format!("{} ({}:{})", e.message(), e.filename(), e.lineno()),
        None => format!("{} did not load", shared.worker_url),
    };
    dispatch(&shared, &mut |m| m.worker_lost(&detail));
    agg_gui::animation::request_draw();
}

fn drop_worker(shared: &Shared) {
    let handle = shared.worker.borrow_mut().take();
    drop(handle);
}

fn ensure_worker(shared: &Rc<Shared>) {
    if shared.worker.borrow().is_some() {
        return;
    }
    let options = WorkerOptions::new();
    options.set_type(WorkerType::Module);
    let worker = match Worker::new_with_options(&shared.worker_url, &options) {
        Ok(worker) => worker,
        Err(err) => {
            web_sys::console::error_2(&"agg-gui-whistle: no worker".into(), &err);
            let shared_for_lost = shared.clone();
            // Report after this dispatch (the machine is borrowed by it).
            wasm_bindgen_futures::spawn_local(async move {
                dispatch(&shared_for_lost, &mut |m| m.worker_lost("Worker() failed"));
                agg_gui::animation::request_draw();
            });
            return;
        }
    };
    let weak = Rc::downgrade(shared);
    let on_message = {
        let weak = weak.clone();
        Closure::<dyn FnMut(MessageEvent)>::new(move |e: MessageEvent| on_reply(&weak, e.data()))
    };
    let on_error = Closure::<dyn FnMut(Event)>::new(move |e: Event| on_worker_error(&weak, e));
    worker.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
    worker.set_onerror(Some(on_error.as_ref().unchecked_ref()));
    *shared.worker.borrow_mut() = Some(WorkerHandle {
        worker,
        _on_message: on_message,
        _on_error: on_error,
    });
}
