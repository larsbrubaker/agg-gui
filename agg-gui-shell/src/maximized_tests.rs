//! Tests for `maximized.rs`: which window styles may be asked whether they
//! are maximized, and that the platform's answer is passed through.

use super::maximized_query;
use std::cell::Cell;

/// `maximized_query` with an `ask` that records whether it ran and answers
/// `answer`; returns (result, asked).
fn query(decorated: bool, resizable: bool, answer: bool) -> (bool, bool) {
    let asked = Cell::new(false);
    let result = maximized_query(decorated, resizable, || {
        asked.set(true);
        answer
    });
    (result, asked.get())
}

#[test]
fn a_titled_resizable_window_is_asked() {
    assert_eq!(query(true, true, true), (true, true));
    assert_eq!(query(true, true, false), (false, true));
}

#[cfg(target_os = "macos")]
#[test]
fn macos_never_restyles_a_borderless_or_fixed_window_to_ask() {
    // Asking would toggle the title bar / resize handles, fire a resize and
    // be asked again: the feedback loop the module docs describe.
    assert_eq!(query(false, false, true), (false, false));
    assert_eq!(query(false, true, true), (false, false));
    assert_eq!(query(true, false, true), (false, false));
}

#[cfg(not(target_os = "macos"))]
#[test]
fn other_platforms_always_ask() {
    assert_eq!(query(false, false, true), (true, true));
    assert_eq!(query(false, true, true), (true, true));
    assert_eq!(query(true, false, true), (true, true));
}
