//! Tests for `teardown.rs` on every platform: handing a slow teardown off returns at once, the
//! next start's wait returns only once it is done, and a teardown that panics doesn't take the
//! waiting start down with it.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::*;

#[test]
fn a_hand_off_returns_at_once_and_the_wait_returns_once_it_is_done() {
    let teardown = Teardown::default();
    let done = Arc::new(AtomicBool::new(false));
    let finished = Arc::clone(&done);
    let handing = Instant::now();
    teardown.hand_off(move || {
        // A slow engine stop, as a real one can be (tens of ms).
        std::thread::sleep(Duration::from_millis(60));
        finished.store(true, Ordering::SeqCst);
    });
    let handed = handing.elapsed();
    eprintln!("hand-off took {handed:?}");
    assert!(
        handed < Duration::from_millis(10),
        "hand-off took {handed:?}"
    );
    assert!(!done.load(Ordering::SeqCst), "the teardown runs on its own");
    teardown.wait();
    assert!(
        done.load(Ordering::SeqCst),
        "the wait returns once it is done"
    );
    // Nothing pending: the wait returns at once.
    teardown.wait();
}

#[test]
fn a_teardown_that_panics_does_not_take_the_wait_down() {
    let teardown = Teardown::default();
    teardown.hand_off(|| panic!("a teardown bug"));
    teardown.wait();
}
