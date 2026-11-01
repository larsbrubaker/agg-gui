//! [`WakeTimer`]: the deadline timer behind a bounded wait (a timeout raced against an answer),
//! such as the search walk's wait for a folder's listing (`search_walk`). Natively a timer thread
//! wakes the waiting future when the timeout passes; the thread waits on a channel whose sender
//! the `WakeTimer` holds, so dropping the timer (the answer arrived, the search was cancelled,
//! or the wait was abandoned) ends the thread at once instead of leaving it asleep for the rest
//! of the timeout. In the browser there are no threads and nothing to start: a fetch there
//! always ends in an answer or a failure, which wakes the wait itself.
//!
//! The thread is the only thing that leaves the caller's thread, and it touches nothing but the
//! `Send` waker: the futures stay local, and the crate keeps no executor or runtime.

use std::sync::atomic::AtomicUsize;
use std::task::Waker;
use std::time::Duration;

/// A running deadline timer; dropping it ends its thread.
pub struct WakeTimer {
    /// Dropped with the timer, which wakes the timer thread to end it.
    #[cfg(not(target_arch = "wasm32"))]
    _cancel: std::sync::mpsc::Sender<()>,
}

impl WakeTimer {
    /// Wakes `waker` once `timeout` passes, unless the timer is dropped first. `live` counts the
    /// timer threads running for one kind of wait, so a test can see that each ends with its
    /// wait.
    #[cfg_attr(target_arch = "wasm32", allow(unused_variables))]
    pub fn start(waker: Waker, timeout: Duration, live: &'static AtomicUsize) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        {
            use std::sync::atomic::Ordering;
            use std::sync::mpsc::{channel, RecvTimeoutError};
            let (cancel, cancelled) = channel::<()>();
            live.fetch_add(1, Ordering::SeqCst);
            std::thread::spawn(move || {
                // Only the timeout wakes the wait; a dropped sender means it is already over.
                if let Err(RecvTimeoutError::Timeout) = cancelled.recv_timeout(timeout) {
                    waker.wake();
                }
                live.fetch_sub(1, Ordering::SeqCst);
            });
            Self { _cancel: cancel }
        }
        #[cfg(target_arch = "wasm32")]
        {
            Self {}
        }
    }
}
