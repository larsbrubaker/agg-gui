//! When a DOM input event happened, on agg-gui's UI clock.
//!
//! A DOM event's `timeStamp` is milliseconds on the page's
//! `performance.now()` timeline, the same timeline the real UI clock reads
//! in a browser, so the event happened `performance.now() - timeStamp` ago.
//! When frames are slow a browser hands the app a finger's moves long after
//! they happened; the touch entry points judge a long press by this time
//! instead of the handling time (`agg_gui::touch_emulation`). The wasm
//! listeners (`web::input`) read the two numbers and call [`event_instant`];
//! it is pure arithmetic so it is unit tested natively.
//!
//! A browser also folds the moves of a slow frame into one `pointermove`
//! carrying the newest sample's position and time; the samples it folded are
//! the event's `getCoalescedEvents()`. A drag that crossed the slop early in
//! the frame would otherwise arrive as one move stamped after the long-press
//! deadline and fire the long press. The listener forwards every sample in
//! order with its own time ([`samples_or_event`]).

use std::time::Duration;

use web_time::Instant;

/// The oldest an event's time may be and still be believed: a stamp older
/// than this is not on the `performance.now()` timeline, and the handling
/// time stands in. (A stamp in epoch milliseconds, as old engines gave, lies
/// in the future and reads as now.)
const MAX_EVENT_AGE: Duration = Duration::from_secs(60);

/// The UI-clock time of an event stamped `stamp_ms`, given that the clock
/// reads `now` while `performance.now()` reads `now_ms`. An event stamped in
/// the future happened now; `None` (use the handling time) when a number is
/// not finite or the age is beyond [`MAX_EVENT_AGE`].
pub(crate) fn event_instant(now: Instant, now_ms: f64, stamp_ms: f64) -> Option<Instant> {
    let age_ms = now_ms - stamp_ms;
    // Checked before `max`, which would turn NaN into 0, and compared in
    // milliseconds: a huge finite age would overflow a `Duration`.
    if !age_ms.is_finite() || age_ms > MAX_EVENT_AGE.as_secs_f64() * 1000.0 {
        return None;
    }
    let age_ms = age_ms.max(0.0);
    now.checked_sub(Duration::from_secs_f64(age_ms / 1000.0))
}

/// The samples a coalesced `pointermove` folded, oldest first, or the event
/// itself when it reports none (an engine without `getCoalescedEvents`, or
/// an event that folded nothing).
pub(crate) fn samples_or_event<T>(samples: Vec<T>, event: T) -> Vec<T> {
    if samples.is_empty() {
        vec![event]
    } else {
        samples
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_event_is_as_old_as_its_stamp_says() {
        let now = Instant::now();
        assert_eq!(
            event_instant(now, 10_700.0, 10_050.0),
            now.checked_sub(Duration::from_millis(650)),
            "stamped 650 ms before performance.now()"
        );
    }

    #[test]
    fn a_stamp_in_the_future_is_now() {
        let now = Instant::now();
        assert_eq!(event_instant(now, 1_000.0, 1_005.0), Some(now));
        // An epoch-milliseconds stamp against the page's timeline.
        assert_eq!(event_instant(now, 5_000.0, 1.7e12), Some(now));
    }

    #[test]
    fn an_unbelievable_stamp_leaves_the_handling_time() {
        let now = Instant::now();
        assert_eq!(
            event_instant(now, 120_000.0, 1_000.0),
            None,
            "two minutes old"
        );
        assert_eq!(event_instant(now, f64::NAN, 1.0), None);
        assert_eq!(event_instant(now, 1.0, f64::INFINITY), None);
    }

    #[test]
    fn a_huge_finite_age_is_unbelievable_not_a_panic() {
        let now = Instant::now();
        assert_eq!(event_instant(now, 1e300, 0.0), None);
        assert_eq!(event_instant(now, f64::MAX, -f64::MAX / 2.0), None);
    }

    #[test]
    fn a_coalesced_move_forwards_its_samples_or_itself() {
        assert_eq!(samples_or_event(vec![1, 2, 3], 3), vec![1, 2, 3]);
        assert_eq!(samples_or_event(Vec::new(), 7), vec![7]);
    }
}
