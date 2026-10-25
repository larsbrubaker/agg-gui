//! Tests for `processor_clock.rs`: which `ScriptProcessor` events still hold
//! their own audio, from when the main thread ran them.

use super::*;

const PERIOD: f64 = 1024.0;

fn verdicts(times: &[f64]) -> Vec<bool> {
    let mut clock = ProcessorClock::new(PERIOD);
    times.iter().map(|t| clock.fresh(*t)).collect()
}

/// Events on time, each a buffer after the last, all hold their audio.
#[test]
fn events_on_time_are_fresh() {
    assert_eq!(verdicts(&[2000.0, 3024.0, 4048.0, 5072.0]), vec![true; 4]);
}

/// A main thread lagging less than a buffer delays events without spoiling
/// them: the audio thread hasn't refilled their buffer yet.
#[test]
fn events_less_than_a_buffer_late_are_fresh() {
    assert_eq!(
        verdicts(&[2000.0, 3824.0, 4048.0, 5872.0, 6096.0]),
        vec![true; 5]
    );
}

/// What Chrome did under 2.5 s main-thread stalls (times recorded in the
/// demo): events that waited out a stall more than a buffer late read a
/// buffer the audio thread was already refilling (one of them carried a
/// later event's audio, pushed twice); the one that came on its heels was
/// in time.
#[test]
fn events_a_stall_held_back_more_than_a_buffer_are_stale() {
    let times = [
        2549.0, 3573.0, 6814.0, 6815.0, 6815.0, 9818.0, 9818.0, 9819.0, 12817.0,
    ];
    assert_eq!(
        verdicts(&times),
        vec![true, true, false, false, true, false, false, true, false]
    );
}

/// A first event that was itself late sets too late a baseline only until
/// an earlier-running one shows the real cadence.
#[test]
fn the_baseline_is_the_earliest_cadence_seen() {
    // Event 0 ran 600 ms late, event 1 on time: event 2, 1 100 ms late by
    // the real cadence, is stale.
    assert_eq!(verdicts(&[2600.0, 3024.0, 5148.0]), vec![true, true, false]);
}
