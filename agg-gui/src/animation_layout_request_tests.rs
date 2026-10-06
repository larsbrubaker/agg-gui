//! Tests for the layout-request channel in `animation.rs` ([`request_layout`],
//! [`layout_requested`], [`take_layout_request`]), pulled in via `#[path]` as
//! its `layout_request_tests` child module so the parent stays under the
//! 800-line limit.  The end-to-end `App::layout` / `App::paint` behaviour is
//! covered in `crate::tests::layout_request`.
//!
//! The flags are thread-local and test threads are reused, so every test
//! clears both channels up front and consumes its own request before ending.

use super::*;

fn reset() {
    take_layout_request();
    clear_draw_request();
}

/// The core guarantee: `App::paint` starts with `clear_draw_request()`, and a
/// layout request must survive it where a plain `request_draw` does not.
#[test]
fn layout_request_survives_clear_draw_request() {
    reset();
    request_layout();
    clear_draw_request();
    assert!(layout_requested(), "the paint-time clear keeps the request");
    assert!(
        wants_draw(),
        "a pending layout request keeps the host drawing"
    );
    assert!(take_layout_request(), "take reports the pending request");
    assert!(!layout_requested(), "take consumes it");
    clear_draw_request();
    assert!(!wants_draw(), "nothing pending once consumed and cleared");
}

/// A layout request advances the invalidation epoch like `request_draw`, so a
/// host that keys its layout skip on the epoch alone still re-lays out.
#[test]
fn layout_request_advances_invalidation_epoch() {
    reset();
    let before = invalidation_epoch();
    request_layout();
    assert_ne!(invalidation_epoch(), before);
    take_layout_request();
    clear_draw_request();
}

/// `request_draw` alone never raises a layout request — its semantics are
/// unchanged for existing consumers.
#[test]
fn request_draw_does_not_request_layout() {
    reset();
    request_draw();
    assert!(!layout_requested());
    clear_draw_request();
    assert!(!wants_draw());
}
