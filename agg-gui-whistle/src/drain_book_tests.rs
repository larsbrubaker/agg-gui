//! Tests for `drain_book.rs`: the stop-time drain's bookkeeping through its
//! races — the worklet's answer against the timeout, a cancel, and a second
//! drain starting while one waits (the case the web host once panicked on,
//! holding the drain's `RefCell` borrowed while it finished the old one).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use super::*;
use crate::protocol::ToWorker;

/// A stand-in for the web host's microphone that counts its releases.
struct Resource {
    session: u32,
    released: Rc<Cell<u32>>,
}

impl Drop for Resource {
    fn drop(&mut self) {
        self.released.set(self.released.get() + 1);
    }
}

fn resource(session: u32, released: &Rc<Cell<u32>>) -> Resource {
    Resource {
        session,
        released: released.clone(),
    }
}

fn finish(session: u32) -> ToWorker {
    ToWorker::Finish { session }
}

fn cancel(session: u32) -> ToWorker {
    ToWorker::Cancel { session }
}

#[test]
fn the_finish_is_held_until_the_worklet_answers_then_goes_once() {
    let released = Rc::new(Cell::new(0));
    let mut book = DrainBook::default();
    assert!(book.start(1, resource(1, &released)).is_none());
    assert!(matches!(book.hold(&finish(1)), Hold::Held));
    let done = book.drained(1).expect("the drain ends");
    assert_eq!(done.resource.session, 1);
    assert_eq!(done.held, vec![finish(1)]);
    drop(done);
    assert_eq!(released.get(), 1);
    // The timeout firing after the answer finds nothing.
    assert!(book.drained(1).is_none());
    assert!(book.is_idle());
}

#[test]
fn a_timeout_ends_the_drain_and_a_late_answer_finds_nothing() {
    let released = Rc::new(Cell::new(0));
    let mut book = DrainBook::default();
    book.start(4, resource(4, &released));
    assert!(matches!(book.hold(&finish(4)), Hold::Held));
    // The timeout.
    assert_eq!(book.drained(4).expect("ends").held, vec![finish(4)]);
    // The worklet's late "flushed".
    assert!(book.drained(4).is_none());
    assert_eq!(released.get(), 1);
}

#[test]
fn a_drain_that_ends_before_its_finish_arrives_lets_the_finish_go() {
    let released = Rc::new(Cell::new(0));
    let mut book = DrainBook::default();
    book.start(2, resource(2, &released));
    assert_eq!(book.drained(2).expect("ends").held, Vec::<ToWorker>::new());
    assert!(matches!(book.hold(&finish(2)), Hold::Post));
}

#[test]
fn a_cancel_drops_the_drain_and_its_finish_and_goes() {
    let released = Rc::new(Cell::new(0));
    let mut book = DrainBook::default();
    book.start(3, resource(3, &released));
    assert!(matches!(book.hold(&finish(3)), Hold::Held));
    match book.hold(&cancel(3)) {
        Hold::PostAfterDropping(dropped) => {
            assert_eq!(dropped.resource.session, 3);
            assert_eq!(dropped.held, vec![finish(3)], "never posted");
        }
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(released.get(), 1);
    // Neither the answer nor the timeout finds it any more.
    assert!(book.drained(3).is_none());
    assert!(book.is_idle());
}

/// A drain still waiting when the next session's stop starts another (its
/// engine failed, or its worker was lost, during the drain; the user tapped
/// again and stopped within the wait): the old drain ends first, its finish
/// handed back to be posted, and the new one waits on its own.
#[test]
fn a_second_drain_starting_ends_the_first_one() {
    let released = Rc::new(Cell::new(0));
    let mut book = DrainBook::default();
    book.start(5, resource(5, &released));
    assert!(matches!(book.hold(&finish(5)), Hold::Held));
    let ended = book
        .start(6, resource(6, &released))
        .expect("the first ends");
    assert_eq!(ended.resource.session, 5);
    assert_eq!(ended.held, vec![finish(5)]);
    drop(ended);
    assert_eq!(released.get(), 1);
    // The first drain's answer or timeout is stale now.
    assert!(book.drained(5).is_none());
    assert!(matches!(book.hold(&finish(6)), Hold::Held));
    assert_eq!(book.drained(6).expect("ends").held, vec![finish(6)]);
    assert_eq!(released.get(), 2);
}

/// The web host keeps the book in a `RefCell` and its callbacks re-enter
/// it: each step must borrow it only for the step itself.
#[test]
fn the_host_pattern_never_borrows_across_a_step() {
    let released = Rc::new(Cell::new(0));
    let book = RefCell::new(DrainBook::default());
    let posted = RefCell::new(Vec::new());
    let start = |session| {
        let ended = book
            .borrow_mut()
            .start(session, resource(session, &released));
        if let Some(done) = ended {
            // Finishing it takes the book again (the chunker flushes, the
            // finish goes out): it must not still be borrowed.
            assert!(book.try_borrow_mut().is_ok());
            posted.borrow_mut().extend(done.held.clone());
        }
    };
    start(7);
    assert!(matches!(book.borrow_mut().hold(&finish(7)), Hold::Held));
    start(8);
    assert_eq!(*posted.borrow(), vec![finish(7)]);
}

#[test]
fn messages_for_other_sessions_pass() {
    let released = Rc::new(Cell::new(0));
    let mut book = DrainBook::default();
    book.start(9, resource(9, &released));
    assert!(matches!(book.hold(&finish(10)), Hold::Post));
    assert!(matches!(book.hold(&cancel(10)), Hold::Post));
    assert!(matches!(
        book.hold(&ToWorker::Push {
            session: 9,
            rate: 16_000,
            samples: vec![0.0],
        }),
        Hold::Post
    ));
    assert!(book.drained(10).is_none());
    assert_eq!(released.get(), 0, "the drain for 9 still waits");
    assert!(book.take().is_some());
    assert_eq!(released.get(), 1);
}

/// A full pass the machine sends after the session's finish waits with it
/// and goes after it, in order; another session's full pass goes at once,
/// and a cancel drops both held messages.
#[test]
fn a_full_pass_after_the_finish_is_held_behind_it() {
    let released = Rc::new(Cell::new(0));
    let full_pass = |session| ToWorker::FullPass { session };
    let mut book = DrainBook::default();
    book.start(8, resource(8, &released));
    assert!(matches!(book.hold(&finish(8)), Hold::Held));
    assert!(matches!(book.hold(&full_pass(8)), Hold::Held));
    assert!(matches!(book.hold(&full_pass(2)), Hold::Post));
    assert_eq!(
        book.drained(8).expect("ends").held,
        vec![finish(8), full_pass(8)]
    );

    book.start(9, resource(9, &released));
    book.hold(&finish(9));
    book.hold(&full_pass(9));
    match book.hold(&cancel(9)) {
        Hold::PostAfterDropping(dropped) => assert_eq!(dropped.held.len(), 2, "never posted"),
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(released.get(), 2);
}
