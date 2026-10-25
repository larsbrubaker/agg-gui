//! The native host (feature `provider`, not wasm): runs the state machine's
//! commands with an engine thread and, with the `native-mic` feature, a
//! recorder (`mic.rs`, cpal).
//!
//! The engine thread starts with the first `Load`: it loads the model
//! (`model.rs`: the configured path, the cache, then a download), builds the
//! engine once per process, and answers session messages from its channel
//! in order (`protocol::next_job` skips cancelled work). A dictation started
//! while the model loads opens the microphone at once; its stream and audio
//! wait in the channel and run once the model is built. The thread takes
//! them in with each step of the download, dropping cancelled sessions'
//! work and keeping a session's latest 30 s past 60 s ([`absorb`]), so a
//! long load holds about a minute of audio at most.
//! Replies and microphone events come back over one channel that every call
//! drains first, and each sender wakes the UI thread through the queue
//! captured when the provider was made (`ui_thread::current_queue`), so an
//! idle app paints the news. A load that fails ends the thread; the next
//! `Load` starts another.
//!
//! Without `native-mic` a tap on the key says that there is no microphone,
//! and nothing starts: the seam for another audio source is `mic.rs`'s
//! `Recorder` (open a session, stream `Push`es to the engine, close with or
//! without its last chunk).

#[cfg(feature = "native-mic")]
mod mic;
mod model;

use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::OnceLock;

use agg_gui::ui_thread::{current_queue, UiQueue};
use whistle::Whistle;

use crate::config::Config;
use crate::engine::Engine;
use crate::machine::{Command, Machine, MicError};
use crate::protocol::{cap_backlog, drop_cancelled, next_job, FromWorker, LoadFailure, ToWorker};
use crate::provider::Host;

/// What the threads tell the UI thread (the microphone events only come
/// with `native-mic`).
#[cfg_attr(not(feature = "native-mic"), allow(dead_code))]
pub(crate) enum Event {
    Worker(FromWorker),
    MicOpened(u32),
    MicFailed(u32, MicError),
    Level(u32, f32),
    /// The engine thread is gone (it panicked, or a push found its inbox
    /// closed).
    EngineLost(String),
}

/// A sender of [`Event`]s that wakes the UI thread.
#[derive(Clone)]
pub(crate) struct Notifier {
    events: Sender<Event>,
    queue: UiQueue,
}

impl Notifier {
    pub(crate) fn send(&self, event: Event) {
        if self.events.send(event).is_ok() {
            self.queue.signal_async_state_change();
        }
    }
}

struct Inner {
    machine: Machine,
    events: Receiver<Event>,
    notifier: Notifier,
    /// The engine thread's inbox, while it runs.
    engine: Option<Sender<ToWorker>>,
    #[cfg(feature = "native-mic")]
    recorder: mic::Recorder,
    model: model::Source,
}

/// See the module docs.
pub(crate) struct NativeHost {
    inner: RefCell<Inner>,
}

impl NativeHost {
    pub(crate) fn new(config: Config) -> NativeHost {
        let (tx, rx) = mpsc::channel();
        let notifier = Notifier {
            events: tx,
            queue: current_queue(),
        };
        let machine = if cfg!(feature = "native-mic") {
            Machine::new(config.model_urls.clone(), config.language.clone())
        } else {
            Machine::unsupported("Built without microphone support")
        };
        NativeHost {
            inner: RefCell::new(Inner {
                machine,
                events: rx,
                #[cfg(feature = "native-mic")]
                recorder: mic::Recorder::new(notifier.clone()),
                notifier,
                engine: None,
                model: model::Source::from_config(&config),
            }),
        }
    }
}

impl Host for NativeHost {
    fn dispatch(&self, f: &mut dyn FnMut(&mut Machine)) {
        let Ok(mut inner) = self.inner.try_borrow_mut() else {
            return;
        };
        let inner = &mut *inner;
        while let Ok(event) = inner.events.try_recv() {
            match event {
                Event::Worker(msg) => {
                    if matches!(msg, FromWorker::LoadFailed { .. }) {
                        // The thread ended; the next load starts another.
                        inner.engine = None;
                    }
                    inner.machine.on_worker(msg);
                }
                Event::MicOpened(s) => inner.machine.on_mic_opened(s),
                Event::MicFailed(s, err) => inner.machine.on_mic_failed(s, err),
                // The first level turns the key to listening; the event
                // already woke the UI thread to show it.
                Event::Level(s, level) => {
                    inner.machine.on_level(s, level);
                }
                // Already gone (a failed load ended the thread, and a push
                // found it so): the machine was told how, by the load.
                Event::EngineLost(_) if inner.engine.is_none() => {}
                Event::EngineLost(detail) => {
                    inner.engine = None;
                    inner.machine.worker_lost(&detail);
                }
            }
        }
        f(&mut inner.machine);
        // A command can feed the machine an event (a microphone that can't
        // open), which may queue more.
        loop {
            let commands = inner.machine.take_commands();
            if commands.is_empty() {
                break;
            }
            for command in commands {
                inner.run(command);
            }
        }
    }
}

impl Inner {
    fn run(&mut self, command: Command) {
        match command {
            Command::Worker(msg) => {
                if matches!(msg, ToWorker::Load { .. }) && self.engine.is_none() {
                    self.engine = Some(spawn_engine(self.model.clone(), self.notifier.clone()));
                }
                let lost = match &self.engine {
                    Some(engine) => engine.send(msg).is_err(),
                    None => false,
                };
                if lost {
                    self.engine = None;
                    self.machine.worker_lost("the engine thread stopped");
                }
            }
            #[cfg(feature = "native-mic")]
            Command::OpenMic { session } => match &self.engine {
                Some(engine) => self.recorder.open(session, engine.clone()),
                None => self
                    .machine
                    .on_mic_failed(session, MicError::Other("no engine".into())),
            },
            #[cfg(feature = "native-mic")]
            Command::CloseMic { session, flush } => self.recorder.close(session, flush),
            // Never queued: without a microphone the machine is unsupported.
            #[cfg(not(feature = "native-mic"))]
            Command::OpenMic { .. } | Command::CloseMic { .. } => {}
        }
    }
}

/// Where an engine thread keeps the model once built (it is leaked: see
/// `engine.rs`).
type ModelCell = &'static OnceLock<&'static Whistle>;

/// The model, built once per process.
static MODEL: OnceLock<&'static Whistle> = OnceLock::new();

fn spawn_engine(source: model::Source, notifier: Notifier) -> Sender<ToWorker> {
    spawn_engine_with(source, &MODEL, notifier)
}

/// [`spawn_engine`] keeping the model in `model`: a test that needs a load
/// to really fail passes its own empty cell, since one already holding the
/// model (another test's) answers every load from memory.
fn spawn_engine_with(
    source: model::Source,
    model: ModelCell,
    notifier: Notifier,
) -> Sender<ToWorker> {
    let (tx, rx) = mpsc::channel::<ToWorker>();
    let failed = notifier.clone();
    let spawned = std::thread::Builder::new()
        .name("agg-gui-whistle-engine".into())
        .spawn(move || engine_thread(source, model, rx, notifier));
    if let Err(err) = spawned {
        failed.send(Event::Worker(FromWorker::LoadFailed {
            reason: LoadFailure::Startup,
            detail: err.to_string(),
        }));
    }
    tx
}

/// The model, from `model` or loaded now (and kept there).
/// `meanwhile` runs with each step of a download's progress (the engine
/// thread keeps its inbox bounded there, [`absorb`]).
fn whistle(
    source: &model::Source,
    model: ModelCell,
    notifier: &Notifier,
    meanwhile: &dyn Fn(),
) -> Result<(&'static Whistle, String), FromWorker> {
    if let Some(whistle) = model.get() {
        return Ok((whistle, "memory".into()));
    }
    let progress = |loaded, total| {
        notifier.send(Event::Worker(FromWorker::Progress { loaded, total }));
        meanwhile();
    };
    let (bytes, from) = source
        .load(&progress)
        .map_err(|(reason, detail)| FromWorker::LoadFailed { reason, detail })?;
    let built = Whistle::from_cact(&bytes).map_err(|e| FromWorker::LoadFailed {
        reason: LoadFailure::Model,
        detail: e.to_string(),
    })?;
    Ok((*model.get_or_init(|| Box::leak(Box::new(built))), from))
}

/// Reports the engine lost if its thread panics (dropped while unwinding),
/// so a dictation in progress ends with an error instead of waiting.
pub(crate) struct LostGuard(pub(crate) Notifier);

impl Drop for LostGuard {
    fn drop(&mut self) {
        if std::thread::panicking() {
            self.0
                .send(Event::EngineLost("the engine thread panicked".into()));
        }
    }
}

/// Take in what waits in `inbox`, drop the work of sessions cancelled
/// meanwhile, and keep a session's latest 30 s of audio once more than
/// 60 s has queued (`protocol::cap_backlog`).
fn absorb(queue: &mut VecDeque<ToWorker>, inbox: &Receiver<ToWorker>) {
    queue.extend(inbox.try_iter());
    drop_cancelled(queue);
    cap_backlog(queue);
}

fn engine_thread(
    source: model::Source,
    model: ModelCell,
    inbox: Receiver<ToWorker>,
    notifier: Notifier,
) {
    let _guard = LostGuard(notifier.clone());
    let mut engine = None;
    let mut queue = VecDeque::new();
    loop {
        // Block for one message, then take every one already waiting, so
        // the next job is chosen knowing them all.
        match inbox.recv() {
            Ok(msg) => queue.push_back(msg),
            Err(_) => return,
        }
        queue.extend(inbox.try_iter());
        while let Some(job) = next_job(&mut queue) {
            if let ToWorker::Load { .. } = job {
                // A dictation's stream and audio arrive while the model
                // loads: taken in as the download goes, their memory
                // bounded ([`absorb`]).
                let held = RefCell::new(std::mem::take(&mut queue));
                let loaded = whistle(&source, model, &notifier, &|| {
                    absorb(&mut held.borrow_mut(), &inbox)
                });
                queue = held.into_inner();
                match loaded {
                    Ok((whistle, from)) => {
                        engine.get_or_insert_with(|| Engine::new(whistle));
                        notifier.send(Event::Worker(FromWorker::Loaded { source: from }));
                        absorb(&mut queue, &inbox);
                    }
                    Err(failed) => {
                        notifier.send(Event::Worker(failed));
                        return;
                    }
                }
                continue;
            }
            let reply = match engine.as_mut() {
                Some(engine) => engine.handle(job),
                None => None,
            };
            if let Some(reply) = reply {
                notifier.send(Event::Worker(reply));
            }
            queue.extend(inbox.try_iter());
        }
    }
}

#[cfg(test)]
#[path = "native_tests.rs"]
mod tests;
