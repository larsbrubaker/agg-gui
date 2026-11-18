//! The time of the most recent user input: [`last_input_at`], recorded by
//! [`note_user_input`]. A child module of `animation.rs` (pulled in with
//! `#[path]`), beside the draw signals it complements.
//!
//! The draw signals say *that* something changed; they move for spinners,
//! finished async loads and scheduled draws as much as for the user, so they
//! cannot tell a host whether the user is busy. This one moves only for input
//! the user made. A host that runs expensive work between frames (a long
//! refinement pass, a background bake) waits until the input has been quiet
//! for a while, so the user's next event never queues behind that work.
//!
//! What counts: every `App::on_*` entry point that carries the user's input
//! records it as it starts: pointer move, press, release and leave, wheel,
//! trackpad pinch, magnify and rotate, key down and up, a change in the held
//! modifiers, committed text and input-method composition, touch start, move,
//! end and cancel, and file drag-hover, drag-leave and drops. So does what
//! follows directly from that input though the framework delivers it later:
//! the touch emulator's mouse events and its long press, the on-screen
//! keyboard's typed keys and held-key repeats, a touch toolbar's Paste, and
//! dictated text reaching the focused field (`widget/app/voice.rs`).
//! Platforms also send some of these events unprompted, and those don't
//! count: the held modifier set repeated unchanged (an empty one as the
//! window loses focus), empty committed text, and the empty composition a
//! shell sends when the input method is switched off with no composition
//! showing (focus leaving a text field).
//!
//! What does not: anything the framework does by itself — a hover or cursor
//! re-evaluation after layout or a release (`App::refresh_cursor`), focus
//! moves that no input caused, draw requests, timers and animations — and
//! window activation, which is a change in the window's state (the click that
//! activates a window arrives as its own press). Gamepad and device-tilt
//! readings are polled state a shell publishes every frame
//! ([`crate::gamepad`], [`crate::tilt`]), not events through `App`; an app
//! that treats them as input calls [`note_user_input`] itself.
//!
//! Times are on the UI clock ([`crate::clock::now`]), so a test on the
//! virtual clock sees exactly the virtual time of its input. The record is
//! per thread, like the clock and the draw signals.

use std::cell::Cell;
use web_time::Instant;

std::thread_local! {
    static LAST_INPUT_AT: Cell<Option<Instant>> = const { Cell::new(None) };
}

/// Record that the user made input now (on the UI clock). The `App` input
/// entry points call this; a host with an input source of its own that does
/// not go through `App` calls it for that source.
pub fn note_user_input() {
    let now = crate::clock::now();
    LAST_INPUT_AT.with(|c| c.set(Some(now)));
}

/// When the user last made input on this thread, on the UI clock; `None`
/// before the first input. Compare with [`crate::clock::since`] to find how
/// long the input has been quiet.
pub fn last_input_at() -> Option<Instant> {
    LAST_INPUT_AT.with(|c| c.get())
}
