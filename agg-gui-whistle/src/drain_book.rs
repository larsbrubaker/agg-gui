//! The bookkeeping of the web host's stop-time drain (`web/drain.rs`),
//! apart from the browser so its races are tested natively
//! (`drain_book_tests.rs`).
//!
//! At a stop the host keeps the session's microphone (the resource `R`)
//! until the capture worklet has delivered what it posted, and holds back
//! the session's `Finish` meanwhile (and the `FullPass` the machine may
//! send after it, which must reach the engine after the finish). [`DrainBook`] says what each event
//! means: the worklet's answer or the timeout ([`DrainBook::drained`]) ends
//! the drain once, whichever comes first; a `Cancel` for the session drops
//! it with its held `Finish` ([`DrainBook::hold`]); a drain started while
//! one waits ends the old one first ([`DrainBook::start`]). Every method
//! hands back what the host must then do, so the host borrows the book
//! only for the call, never while it acts.

use crate::protocol::ToWorker;

/// An ended drain: release `resource` after delivering its last audio,
/// then post what it `held` (the session's `Finish` and `FullPass`, those
/// the machine sent), in order.
pub(crate) struct Done<R> {
    pub resource: R,
    pub held: Vec<ToWorker>,
}

/// What to do with a message for the worker.
pub(crate) enum Hold<R> {
    /// Post it.
    Post,
    /// Don't post it now: the drain posts it when it ends.
    Held,
    /// A cancel: drop the drain (what it held is never posted), then
    /// post the cancel.
    PostAfterDropping(Done<R>),
}

impl<R> std::fmt::Debug for Hold<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Hold::Post => "Post",
            Hold::Held => "Held",
            Hold::PostAfterDropping(_) => "PostAfterDropping",
        })
    }
}

/// The drain in progress, if any (see the module docs).
pub(crate) struct DrainBook<R> {
    current: Option<(u32, Done<R>)>,
}

impl<R> Default for DrainBook<R> {
    fn default() -> Self {
        DrainBook { current: None }
    }
}

impl<R> DrainBook<R> {
    /// Start draining `session`'s `resource`. A drain still waiting ends
    /// now: it is returned, to be finished first.
    pub fn start(&mut self, session: u32, resource: R) -> Option<Done<R>> {
        let ended = self.current.take().map(|(_, done)| done);
        self.current = Some((
            session,
            Done {
                resource,
                held: Vec::new(),
            },
        ));
        ended
    }

    /// The worklet answered for `session`, or its wait is over: the drain,
    /// the first time; `None` for a drain already ended or replaced.
    pub fn drained(&mut self, session: u32) -> Option<Done<R>> {
        match &self.current {
            Some((s, _)) if *s == session => self.current.take().map(|(_, done)| done),
            _ => None,
        }
    }

    /// Before `msg` goes to the worker (see [`Hold`]).
    pub fn hold(&mut self, msg: &ToWorker) -> Hold<R> {
        let (session, finishing) = match msg {
            ToWorker::Finish { session } | ToWorker::FullPass { session } => (*session, true),
            ToWorker::Cancel { session } => (*session, false),
            _ => return Hold::Post,
        };
        match &mut self.current {
            Some((s, done)) if *s == session => {
                if finishing {
                    done.held.push(msg.clone());
                    Hold::Held
                } else {
                    match self.current.take() {
                        Some((_, done)) => Hold::PostAfterDropping(done),
                        None => Hold::Post,
                    }
                }
            }
            _ => Hold::Post,
        }
    }

    /// The drain in progress, taken (the host shutting down).
    pub fn take(&mut self) -> Option<Done<R>> {
        self.current.take().map(|(_, done)| done)
    }

    /// No drain is in progress.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn is_idle(&self) -> bool {
        self.current.is_none()
    }
}

#[cfg(test)]
#[path = "drain_book_tests.rs"]
mod tests;
