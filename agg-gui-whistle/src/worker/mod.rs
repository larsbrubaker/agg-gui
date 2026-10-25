//! The transcription Web Worker (wasm, feature `worker`): answers the
//! provider's messages with the engine.
//!
//! `web/worker.js` loads the `agg-gui-whistle-worker` module in a dedicated
//! worker, queues the messages that arrive meanwhile, and calls [`run`] with
//! them. From then on every message goes into one queue that a single task
//! works through in order (`protocol::next_job` picks the next job, skipping
//! cancelled work). `Load` downloads, checks and caches
//! the model (`model_load.rs`) and builds the [`Engine`]; every other message
//! goes to the engine, whose reply is posted back. Between jobs the task
//! yields to the event loop, so a cancel posted while a pass ran joins the
//! queue before the next job is chosen.

mod model_load;

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use js_sys::{Array, Promise};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{DedicatedWorkerGlobalScope, MessageEvent};

use crate::engine::Engine;
use crate::js_fields::{from_object, to_object};
use crate::protocol::{next_job, FromWorker, LoadFailure, Recognised, ToWorker};

struct Worker {
    scope: DedicatedWorkerGlobalScope,
    queue: VecDeque<ToWorker>,
    engine: Option<Engine>,
    /// The task working through the queue is running.
    busy: bool,
}

type Shared = Rc<RefCell<Worker>>;

/// Answer messages from now on; `queued` holds those that arrived before
/// the module was ready, oldest first.
pub fn run(queued: Array) {
    let scope: DedicatedWorkerGlobalScope = js_sys::global().unchecked_into();
    let worker: Shared = Rc::new(RefCell::new(Worker {
        scope: scope.clone(),
        queue: VecDeque::new(),
        engine: None,
        busy: false,
    }));
    for message in queued.iter() {
        enqueue(&worker, &message);
    }
    let on_message = {
        let worker = worker.clone();
        Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
            enqueue(&worker, &event.data());
            kick(&worker);
        })
    };
    scope.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
    // The handler lives as long as the worker.
    on_message.forget();
    kick(&worker);
}

fn enqueue(worker: &Shared, message: &JsValue) {
    match from_object(message, ToWorker::from_fields) {
        Ok(msg) => worker.borrow_mut().queue.push_back(msg),
        Err(err) => web_sys::console::error_1(&JsValue::from_str(&err.to_string())),
    }
}

fn post(scope: &DedicatedWorkerGlobalScope, msg: &FromWorker) {
    let (object, _) = to_object(msg.to_fields());
    if let Err(err) = scope.post_message(&object) {
        web_sys::console::error_2(&JsValue::from_str("agg-gui-whistle worker:"), &err);
    }
}

/// Start the queue's task unless it is running.
fn kick(worker: &Shared) {
    {
        let mut w = worker.borrow_mut();
        if w.busy {
            return;
        }
        w.busy = true;
    }
    let worker = worker.clone();
    wasm_bindgen_futures::spawn_local(async move {
        loop {
            // Not borrowed across the await: `onmessage` runs meanwhile.
            let scope = worker.borrow().scope.clone();
            yield_to_events(&scope).await;
            let Some(job) = ({
                let mut w = worker.borrow_mut();
                let job = next_job(&mut w.queue);
                if job.is_none() {
                    w.busy = false;
                }
                job
            }) else {
                return;
            };
            run_job(&worker, job).await;
        }
    });
}

/// Let the event loop deliver the messages already posted (a zero timeout).
async fn yield_to_events(scope: &DedicatedWorkerGlobalScope) {
    let promise = Promise::new(&mut |resolve, _| {
        let _ = scope.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 0);
    });
    let _ = JsFuture::from(promise).await;
}

async fn run_job(worker: &Shared, job: ToWorker) {
    let scope = worker.borrow().scope.clone();
    if let ToWorker::Load { urls } = job {
        if worker.borrow().engine.is_some() {
            post(
                &scope,
                &FromWorker::Loaded {
                    source: "memory".into(),
                },
            );
            return;
        }
        let progress = |loaded, total| post(&scope, &FromWorker::Progress { loaded, total });
        let reply = match model_load::load(&scope, &urls, &progress).await {
            Ok((bytes, source)) => match Engine::from_cact(&bytes) {
                Ok(engine) => {
                    worker.borrow_mut().engine = Some(engine);
                    FromWorker::Loaded { source }
                }
                Err(err) => FromWorker::LoadFailed {
                    reason: LoadFailure::Model,
                    detail: err.to_string(),
                },
            },
            Err((reason, detail)) => FromWorker::LoadFailed { reason, detail },
        };
        post(&scope, &reply);
        return;
    }
    let reply = match worker.borrow_mut().engine.as_mut() {
        Some(engine) => engine.handle(job),
        None => without_model(job),
    };
    if let Some(reply) = reply {
        post(&scope, &reply);
    }
}

/// A session message before the model loaded: the provider never sends one,
/// but if it does, a start fails and a finish still ends the session.
fn without_model(job: ToWorker) -> Option<FromWorker> {
    match job {
        ToWorker::Start { session, .. } => Some(FromWorker::Failed {
            session,
            detail: "the model is not loaded".into(),
        }),
        ToWorker::Finish { session } => Some(FromWorker::Final {
            session,
            update: Recognised::default(),
        }),
        _ => None,
    }
}
