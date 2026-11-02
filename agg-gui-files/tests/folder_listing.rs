//! `folder_listing`: a folder loaded and its listing awaited. A folder that lists within `load`
//! answers at once; one that lists later is waited for until it raises content-changed with its
//! load no longer pending, then answers its listing or the kind of failure it ended in; the wait
//! takes its listener off the folder whether it answers or is dropped.

#[path = "support/late_folder.rs"]
mod late_folder;

use std::future::Future;
use std::pin::pin;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};

use agg_gui_files::{folder_listing, Item, ListingFailure, MemoryFolder};

use late_folder::{LateFolder, LateLink};

/// Counts its wakes.
#[derive(Default)]
struct CountingWaker(AtomicUsize);

impl Wake for CountingWaker {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn a_folder_that_lists_in_load_answers_at_once() {
    let memory = MemoryFolder::new("Parts");
    memory.add_file("gear.stl", b"g".to_vec());
    let mut wait = pin!(folder_listing(Box::new(memory)));
    let mut cx = Context::from_waker(Waker::noop());
    let Poll::Ready(Ok(folder)) = wait.as_mut().poll(&mut cx) else {
        panic!("a memory folder lists within load");
    };
    assert_eq!(folder.items().len(), 1);
}

#[test]
fn a_folder_that_lists_later_is_waited_for_and_woken_when_it_lands() {
    let late = LateFolder::new("Remote");
    let state = Rc::clone(&late.state);
    let counter = Arc::new(CountingWaker::default());
    let waker = Waker::from(Arc::clone(&counter));
    let mut cx = Context::from_waker(&waker);
    let mut wait = pin!(folder_listing(Box::new(late)));
    assert!(wait.as_mut().poll(&mut cx).is_pending());
    assert_eq!(state.loads.get(), 1, "the wait starts the listing");
    assert_eq!(state.listeners.get(), 1);
    assert!(
        wait.as_mut().poll(&mut cx).is_pending(),
        "nothing has landed"
    );

    let gear: Rc<dyn Item> = Rc::new(LateLink(LateFolder::new("gear")));
    state.land(vec![gear]);
    assert_eq!(
        counter.0.load(Ordering::SeqCst),
        1,
        "landing wakes the wait"
    );
    let Poll::Ready(Ok(folder)) = wait.as_mut().poll(&mut cx) else {
        panic!("the listing has landed");
    };
    assert_eq!(folder.items()[0].name(), "gear");
    assert_eq!(state.loads.get(), 1, "loaded once");
    assert_eq!(state.listeners.get(), 0, "the listener came off");
}

#[test]
fn a_failed_listing_answers_its_failure() {
    let late = LateFolder::new("Remote");
    let state = Rc::clone(&late.state);
    let mut cx = Context::from_waker(Waker::noop());
    let mut wait = pin!(folder_listing(Box::new(late)));
    assert!(wait.as_mut().poll(&mut cx).is_pending());
    state.fail(ListingFailure::SignIn);
    assert!(matches!(
        wait.as_mut().poll(&mut cx),
        Poll::Ready(Err(ListingFailure::SignIn))
    ));
    assert_eq!(state.listeners.get(), 0);
}

#[test]
fn a_dropped_wait_takes_its_listener_off() {
    let late = LateFolder::new("Remote");
    let state = Rc::clone(&late.state);
    {
        let mut cx = Context::from_waker(Waker::noop());
        let mut wait = Box::pin(folder_listing(Box::new(late)));
        assert!(wait.as_mut().poll(&mut cx).is_pending());
        assert_eq!(state.listeners.get(), 1);
    }
    assert_eq!(state.listeners.get(), 0);
}
