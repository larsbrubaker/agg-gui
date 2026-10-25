//! Draining the capture node at a stop, for the web host (`mod.rs`,
//! `mic.rs`): the render quanta the worklet already posted are still in
//! its port's queue when the user taps stop, and dropping the microphone
//! then would lose them (up to a few tens of milliseconds of speech); a
//! `ScriptProcessor` holds up to a buffer's worth (about a second) that it
//! hands over only with its next event.
//!
//! So a stop disconnects the microphone's input and stops its tracks at
//! once, and keeps the microphone until its audio is in: a worklet gets
//! [`FLUSH`] (`web/capture-worklet.js`) and answers [`FLUSHED`] (messages on
//! a port arrive in order, so every quantum posted before is in by then); a
//! `ScriptProcessor`'s first event a buffer's time after the stop ends it
//! (`mic.rs`). The machine's `Finish` for the session, which follows the
//! `CloseMic`, is held back meanwhile ([`hold`]) and posted after the last
//! `Push`. A node that doesn't deliver within [`DRAIN_MS`] (beyond the
//! processor's buffer) is released anyway; a `Cancel` for the session drops
//! the drain and what it held. What each
//! event means is `drain_book.rs`'s (tested natively); this file does it,
//! borrowing the book only for each call, never while it acts (a callback
//! it runs may come back to it).

use std::rc::{Rc, Weak};

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use super::mic::Mic;
use super::{post, Shared};
use crate::drain_book::{Done, Hold};
use crate::protocol::ToWorker;

/// Asks the worklet to answer once everything before it is posted.
pub(crate) const FLUSH: &str = "flush";
/// The worklet's answer.
pub(crate) const FLUSHED: &str = "flushed";
/// How long a stop waits for the worklet, in ms; for a `ScriptProcessor`,
/// beyond its buffer's time.
const DRAIN_MS: i32 = 500;

/// A stopped microphone delivering its last quanta, and its timeout.
pub(crate) struct Draining {
    mic: Mic,
    timer: Option<i32>,
    _on_timeout: Closure<dyn FnMut()>,
}

impl Drop for Draining {
    fn drop(&mut self) {
        // The timer must not call its closure once it is gone.
        if let (Some(timer), Some(window)) = (self.timer, web_sys::window()) {
            window.clear_timeout_with_handle(timer);
        }
    }
}

/// Start draining `mic` (taken from the host); see the module docs.
pub(crate) fn start(shared: &Rc<Shared>, mic: Mic) {
    mic.stop_input();
    let session = mic.session;
    let wait = match mic.worklet_port() {
        Some(port) => {
            let _ = port.post_message(&FLUSH.into());
            DRAIN_MS
        }
        None => mic.drain_processor() + DRAIN_MS,
    };
    let weak = Rc::downgrade(shared);
    let on_timeout = Closure::<dyn FnMut()>::new(move || drained(weak.clone(), session));
    let timer = web_sys::window().and_then(|w| {
        w.set_timeout_with_callback_and_timeout_and_arguments_0(
            on_timeout.as_ref().unchecked_ref(),
            wait,
        )
        .ok()
    });
    let draining = Draining {
        mic,
        timer,
        _on_timeout: on_timeout,
    };
    // A drain still waiting (its session's engine failed or its worker was
    // lost meanwhile, and the next session stopped within the wait) ends
    // first, after the book is released.
    let ended = shared.drain.borrow_mut().start(session, draining);
    if let Some(done) = ended {
        deliver(shared, done);
    }
}

/// The capture node delivered, or the wait is over: finish the drain after
/// the handler that called this returns (finishing drops that handler).
pub(crate) fn drained(shared: Weak<Shared>, session: u32) {
    wasm_bindgen_futures::spawn_local(async move {
        if let Some(shared) = shared.upgrade() {
            let done = shared.drain.borrow_mut().drained(session);
            if let Some(done) = done {
                deliver(&shared, done);
            }
            agg_gui::animation::request_draw();
        }
    });
}

/// An ended drain: its last chunk, then its held `Finish` (and `FullPass`);
/// then the microphone is released (dropped).
fn deliver(shared: &Shared, done: Done<Draining>) {
    done.resource.mic.chunker.borrow_mut().flush(shared);
    for msg in &done.held {
        post(shared, msg);
    }
}

/// Before `msg` goes to the worker: `true` when the drain holds it (a
/// `Finish` or `FullPass` for the session draining). A `Cancel` for that session drops
/// the drain, and goes.
pub(crate) fn hold(shared: &Shared, msg: &ToWorker) -> bool {
    let hold = shared.drain.borrow_mut().hold(msg);
    match hold {
        Hold::Held => true,
        Hold::Post => false,
        Hold::PostAfterDropping(dropped) => {
            drop(dropped);
            false
        }
    }
}
