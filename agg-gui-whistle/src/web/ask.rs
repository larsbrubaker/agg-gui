//! The microphone request for the web host's microphone (`mic.rs`): one
//! `getUserMedia` per dictation, made inside the tap (in its task, from a
//! microtask the tap queues, so the browser sees the user's gesture: the
//! prompt shows at once, and iOS can't refuse a request after a denial for
//! want of one), and never two in flight on the page. The microtask runs
//! when the tap's handler returns: the device starts after the tap's own
//! work, not while that work still holds the main thread, which would lose
//! what the user says until the stream can be wired.
//!
//! The request in flight is kept page-wide (a thread-local: the UI thread
//! is the page's), with the session that owns its answer. A session that
//! opens while one is still unanswered (an earlier session's prompt still
//! up, that session since cancelled) takes that request over instead of
//! making a second one: the answer goes to the latest owner, and every
//! earlier session waiting on it steps aside without touching the stream.
//! An owner no longer wanted when the answer comes releases the stream
//! (`mic.rs`).

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use js_sys::{Object, Promise, Reflect};
use wasm_bindgen::prelude::*;
use web_sys::MediaStreamConstraints;

use super::{mic, Shared};
use crate::machine::MicError;

/// The `getUserMedia` in flight and whose answer it is.
struct Asking {
    promise: Promise,
    owner: Weak<Shared>,
    session: u32,
}

thread_local! {
    static ASKING: RefCell<Option<Asking>> = const { RefCell::new(None) };
}

/// Ask for the microphone for `session`, now: called in the tap's task. A
/// request still in flight is taken over rather than made again. The
/// promise resolves to the `MediaStream`.
pub(super) fn request(shared: &Rc<Shared>, session: u32) -> Result<Promise, MicError> {
    let owner = Rc::downgrade(shared);
    ASKING.with(|asking| {
        let mut asking = asking.borrow_mut();
        if let Some(ask) = asking.as_mut() {
            ask.owner = owner;
            ask.session = session;
            return Ok(ask.promise.clone());
        }
        let promise = get_user_media()?;
        *asking = Some(Asking {
            promise: promise.clone(),
            owner,
            session,
        });
        Ok(promise)
    })
}

/// After `promise` settled: whether `session` of `owner` owns its answer
/// (it was the last to ask). The owner takes it, which ends the request;
/// anyone else steps aside.
pub(super) fn take_answer(promise: &Promise, owner: &Weak<Shared>, session: u32) -> bool {
    ASKING.with(|asking| {
        let mut asking = asking.borrow_mut();
        let mine = asking.as_ref().is_some_and(|ask| {
            Object::is(&ask.promise, promise) && ask.owner.ptr_eq(owner) && ask.session == session
        });
        if mine {
            *asking = None;
        }
        mine
    })
}

/// `getUserMedia` for the dictation: mono, with the browser's echo
/// cancellation and noise suppression.
fn get_user_media() -> Result<Promise, MicError> {
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
    devices
        .get_user_media_with_constraints(&constraints)
        .map_err(|e| mic::mic_error(&e))
}
