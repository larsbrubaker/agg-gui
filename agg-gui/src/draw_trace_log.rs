//! Non-consuming read of draw-request provenance — the companion of
//! [`crate::animation::drain_draw_trace`].
//!
//! `drain_draw_trace` is consume-on-read: the quiescence guards in tests and
//! [`crate::debug_draw_report`] take the tags they find, so a second reader
//! (an app's Performance window counting *why* frames were drawn) would
//! steal them from those consumers, or have them stolen.  This log is fed by
//! the same `request_draw*` calls but read by cursor: each reader keeps its
//! own [`DrawTraceCursor`], asks [`draw_trace_since`] for the tags recorded
//! after it, and nothing is removed for anybody else.
//!
//! Untagged requests ([`crate::animation::request_draw`] and
//! [`crate::animation::request_draw_without_invalidation`]) are recorded as
//! [`UNTAGGED_DRAW_REQUEST`], so a reader can tell "something asked for a
//! frame without saying why" from "nothing asked at all".
//!
//! Debug builds only, like the drained trace: in release recording compiles
//! out and every read is empty.

/// The tag an untagged draw request is logged under.
pub const UNTAGGED_DRAW_REQUEST: &str = "untagged";

/// Number of entries the log retains; older ones are overwritten. A reader
/// that falls further behind than this learns how many it missed from
/// [`DrawTraceRead::missed`].
pub const DRAW_TRACE_LOG_CAP: usize = 1024;

/// A reader's position in the log: the sequence number of the next entry it
/// has not seen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DrawTraceCursor(u64);

/// What [`draw_trace_since`] found after a cursor.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DrawTraceRead {
    /// Tags recorded after the cursor, oldest first.
    pub tags: Vec<&'static str>,
    /// Entries recorded after the cursor that the log had already
    /// overwritten (the reader fell more than [`DRAW_TRACE_LOG_CAP`] behind).
    pub missed: u64,
    /// The cursor to pass next time.
    pub next: DrawTraceCursor,
}

#[cfg(debug_assertions)]
struct Log {
    entries: Vec<&'static str>,
    /// Sequence number of the next entry; `entries[seq % CAP]` holds entry `seq`.
    next_seq: u64,
}

#[cfg(debug_assertions)]
std::thread_local! {
    static LOG: std::cell::RefCell<Log> = const {
        std::cell::RefCell::new(Log { entries: Vec::new(), next_seq: 0 })
    };
}

/// Append `tag` (called by the `request_draw*` helpers in `animation`).
#[cfg(debug_assertions)]
pub(crate) fn record(tag: &'static str) {
    LOG.with(|log| {
        let mut log = log.borrow_mut();
        let slot = (log.next_seq % DRAW_TRACE_LOG_CAP as u64) as usize;
        if slot < log.entries.len() {
            log.entries[slot] = tag;
        } else {
            log.entries.push(tag);
        }
        log.next_seq += 1;
    });
}

#[cfg(not(debug_assertions))]
#[inline(always)]
pub(crate) fn record(_tag: &'static str) {}

/// A cursor at the end of the log: a reader starting here sees only what is
/// requested from now on.
pub fn draw_trace_cursor() -> DrawTraceCursor {
    #[cfg(debug_assertions)]
    {
        LOG.with(|log| DrawTraceCursor(log.borrow().next_seq))
    }
    #[cfg(not(debug_assertions))]
    {
        DrawTraceCursor(0)
    }
}

/// The tags recorded after `cursor`, without removing them for any other
/// reader (and without touching what [`crate::animation::drain_draw_trace`]
/// returns).
pub fn draw_trace_since(cursor: DrawTraceCursor) -> DrawTraceRead {
    #[cfg(debug_assertions)]
    {
        LOG.with(|log| {
            let log = log.borrow();
            let cap = DRAW_TRACE_LOG_CAP as u64;
            let start = cursor.0.min(log.next_seq);
            let oldest_kept = log.next_seq.saturating_sub(cap);
            let first = start.max(oldest_kept);
            let tags = (first..log.next_seq)
                .map(|seq| log.entries[(seq % cap) as usize])
                .collect();
            DrawTraceRead {
                tags,
                missed: first - start,
                next: DrawTraceCursor(log.next_seq),
            }
        })
    }
    #[cfg(not(debug_assertions))]
    {
        DrawTraceRead {
            tags: Vec::new(),
            missed: 0,
            next: cursor,
        }
    }
}

#[cfg(all(test, debug_assertions))]
mod tests {
    use super::*;
    use crate::animation::{
        clear_draw_request, drain_draw_trace, request_draw, request_draw_tagged,
    };

    #[test]
    fn readers_do_not_steal_from_each_other_or_from_the_drain() {
        let _ = drain_draw_trace();
        let a = draw_trace_cursor();
        let b = draw_trace_cursor();
        request_draw_tagged("test.one");
        request_draw();
        let read_a = draw_trace_since(a);
        assert_eq!(read_a.tags, vec!["test.one", UNTAGGED_DRAW_REQUEST]);
        assert_eq!(read_a.missed, 0);
        // A second reader still sees both, and the drained trace keeps its tag.
        assert_eq!(draw_trace_since(b).tags, read_a.tags);
        assert_eq!(drain_draw_trace(), vec!["test.one"]);
        // Reading on from the returned cursor sees nothing new.
        assert!(draw_trace_since(read_a.next).tags.is_empty());
        clear_draw_request();
    }

    #[test]
    fn a_reader_left_behind_learns_how_many_it_missed() {
        let start = draw_trace_cursor();
        for _ in 0..DRAW_TRACE_LOG_CAP + 5 {
            record("test.flood");
        }
        let read = draw_trace_since(start);
        assert_eq!(read.missed, 5);
        assert_eq!(read.tags.len(), DRAW_TRACE_LOG_CAP);
    }
}
