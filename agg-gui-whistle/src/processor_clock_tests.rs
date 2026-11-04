//! Tests for `processor_clock.rs`: which `ScriptProcessor` events still hold
//! their own audio, from when the main thread ran them.

use super::*;

const PERIOD: f64 = 1024.0;

fn verdicts(times: &[f64]) -> Vec<bool> {
    let mut clock = ProcessorClock::new(PERIOD, None);
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

/// What Chrome did on a main thread that ran every event of a dictation's
/// start late (times recorded in the demo, the node wired at 2097 ms):
/// events 0 and 1 ran 1181 and 158 ms late, so by the cadence alone event
/// 2, 1023 ms late, looked 865 ms late and its buffer, whose head already
/// held event 4's speech, was pushed ahead of the speech before it. When
/// the processor was wired bounds when event 0 was due, so it is stale.
#[test]
fn events_are_due_no_later_than_a_buffer_after_the_wiring() {
    let times = [4302.0, 4303.0, 6192.0, 6193.0, 8467.0, 8468.0];
    let mut clock = ProcessorClock::new(PERIOD, Some(2097.0));
    let verdicts: Vec<bool> = times.iter().map(|t| clock.fresh(*t)).collect();
    assert_eq!(verdicts, vec![false, true, false, true, false, true]);
}
