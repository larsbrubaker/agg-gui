//! Port of agg-sharp `Tests/Agg.Tests/Agg Automation Tests/AutomationRunnerTests.cs`.
//!
//! Only the tests whose subjects have landed are here; the rest of the class
//! (window bring-up, timeouts, name lookup, clicks) arrives with the runner
//! slices listed in `docs/design/gui-automation.md`.

use std::time::Instant;

use agg_gui_automation::static_delay;

#[test]
fn static_delay_expires_on_total_elapsed_time_not_the_seconds_component() {
    let timer = Instant::now();

    let satisfied = static_delay(|| false, 0.2, 10);

    assert!(!satisfied);
    assert!(
        timer.elapsed().as_secs_f64() < 0.9,
        "a .2 second wait must expire on total elapsed time, not on whole seconds ticking over"
    );
}
