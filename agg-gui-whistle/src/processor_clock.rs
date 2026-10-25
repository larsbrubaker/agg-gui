//! When a `ScriptProcessor` event still holds its own audio, for the web
//! host's fallback capture (`web/mic.rs`), apart from the browser so it is
//! tested natively (`processor_clock_tests.rs`).
//!
//! A `ScriptProcessor` hands its input to the main thread from a double
//! buffer: when a buffer is full the audio thread queues its event and
//! fills the other one, then starts refilling the first a buffer's time
//! later. An event the main thread runs later than that reads a buffer
//! already (partly or wholly) holding newer audio: replayed and shuffled
//! speech, which a recogniser turns into words nobody said. A stalled main
//! thread (CI's software-rendered one lags by seconds) does exactly that,
//! and the events that waited then run back to back.
//!
//! Neither the context's clock nor its output timestamp tells how late an
//! event runs (the main thread's copies are refreshed with the event), but
//! the events' cadence does: event `k` is due `k` buffers after the first.
//! [`ProcessorClock`] takes the earliest-running event seen as on time and
//! calls an event stale when it runs later than a buffer (less a margin)
//! after it was due; the capture then drops it, so a stall costs words, not
//! made-up ones (and adds no digital silence amid speech, in which a
//! recogniser hears words of its own). A main thread already more
//! than a buffer behind at the first event, and never catching up, would
//! go unnoticed: nothing on the page says when that event was due.

/// How much sooner than a full buffer's lateness an event counts as stale,
/// in ms: the refill may start a little before the event's nominal time.
const MARGIN_MS: f64 = 50.0;

/// See the module docs.
pub(crate) struct ProcessorClock {
    period_ms: f64,
    /// Events seen so far.
    count: u64,
    /// The earliest of `arrival - k * period` seen: when event 0 was due.
    baseline: Option<f64>,
}

impl ProcessorClock {
    /// For a processor whose buffer lasts `period_ms`.
    pub fn new(period_ms: f64) -> ProcessorClock {
        ProcessorClock {
            period_ms,
            count: 0,
            baseline: None,
        }
    }

    /// The next event runs now (`now_ms`, the page's clock): whether its
    /// buffer still holds its own audio.
    pub fn fresh(&mut self, now_ms: f64) -> bool {
        let nominal = now_ms - self.count as f64 * self.period_ms;
        self.count += 1;
        let baseline = self.baseline.map_or(nominal, |b| b.min(nominal));
        self.baseline = Some(baseline);
        nominal - baseline < self.period_ms - MARGIN_MS
    }
}

#[cfg(test)]
#[path = "processor_clock_tests.rs"]
mod tests;
