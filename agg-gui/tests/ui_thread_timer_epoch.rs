//! Rust-only check of `agg_gui::ui_thread::current_timer_ms` on a virtual
//! clock that started before the process first read the timer. The timer
//! counts from a process-wide epoch; a headless driver puts its thread on the
//! virtual clock (standing at "now") and only reads the timer once its first
//! frame drains, after building its whole widget tree. Real time spent in
//! between must not be lost: were the epoch taken at the first read, the
//! virtual clock would sit before it and the timer would read 0 until the
//! frames had made up that build time, so every `run_on_idle_after`-style
//! delay keyed on the timer would stall that long. One test in its own
//! binary, because the epoch is process state that another test could set
//! first.

use std::time::Duration;

use agg_gui::{clock, ui_thread};
use web_time::Instant;

#[test]
fn rust_only_the_timer_counts_virtual_time_from_where_the_virtual_clock_started() {
    // The driver's clock stands at the moment it starts, before anything has
    // read the UI timer.
    clock::set_virtual(Instant::now());
    // Building the app takes real time that the virtual clock does not see.
    std::thread::sleep(Duration::from_millis(200));
    let first = ui_thread::current_timer_ms();
    // One frame later the timer has moved by that frame, not by zero.
    clock::advance(Duration::from_millis(20));
    let second = ui_thread::current_timer_ms();
    assert_eq!(
        second - first,
        20,
        "the timer follows the virtual clock from its start (read {first} then {second})"
    );
    clock::use_real();
}
