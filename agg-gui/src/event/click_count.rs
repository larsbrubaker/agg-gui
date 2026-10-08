//! The click count of the mouse button event being processed — agg-sharp's
//! `MouseEventArgs.Clicks` and `GuiWidget.IsDoubleClick`, which agg-gui's
//! `Event::MouseDown`/`MouseUp` do not carry (adding a field would break
//! every exhaustive pattern on them downstream).
//!
//! [`crate::widget::App`] records each press's count here as it dispatches
//! it (`App::on_mouse_down_clicks`, or the count the shells'
//! [`InputForwarder`](crate::shell_input::InputForwarder) decided) and
//! marks the release, so a widget asks while handling either event. Counts
//! follow the platform rule C# relies on: the second press of a double
//! click reports 2, and every release reports 1 — which is why
//! [`is_double_click`] remembers the press until its release has been
//! dispatched.
//!
//! A count is *stated* when whoever sent the press said it outright
//! (`App::on_mouse_down_clicks`, `ClickCount::Explicit`: simulated input);
//! a count the forwarder worked out from the spacing of presses is not.
//! [`crate::widgets::multi_click::MultiClickTracker`] honours only a stated
//! count, so real-mouse multi-clicks keep each editor's own counting.

use std::cell::Cell;
use std::time::Duration;

use web_time::Instant;

/// C#'s double-click window in `GuiWidget.IsDoubleClick`: the press must
/// have been processed less than 550 ms ago.
pub const DOUBLE_CLICK_WINDOW: Duration = Duration::from_millis(550);

#[derive(Clone, Copy, Default)]
struct ClickState {
    /// `MouseEventArgs.Clicks` of the event being processed (or the last one).
    event_clicks: u32,
    /// The count of the press in progress, cleared once its release has been
    /// dispatched (C#'s `lastMouseDownClicks`).
    down_clicks: u32,
    /// When the last press was processed (C#'s `LastMouseDownMs`).
    down_at: Option<Instant>,
    /// Whether the press in progress stated its count.
    stated: bool,
}

thread_local! {
    static CLICKS: Cell<ClickState> = Cell::new(ClickState::default());
}

fn update(f: impl FnOnce(&mut ClickState)) {
    CLICKS.with(|c| {
        let mut state = c.get();
        f(&mut state);
        c.set(state);
    });
}

/// C# `MouseEventArgs.Clicks` of the mouse button event being processed:
/// the press's click count during a `MouseDown` (1 for a single click, 2 for
/// the second press of a double click, ...), and 1 during a `MouseUp`. Keeps
/// the last event's value between events; 0 before any.
pub fn current_click_count() -> u32 {
    CLICKS.with(|c| c.get().event_clicks)
}

/// The press in progress's click count when its sender stated it (see the
/// module docs); `None` for a counted real press, and once it is released.
pub fn stated_click_count() -> Option<u32> {
    CLICKS.with(|c| {
        let state = c.get();
        (state.stated && state.down_clicks > 0).then_some(state.down_clicks)
    })
}

/// C# `GuiWidget.IsDoubleClick`: whether the event being processed belongs
/// to a double click. Works from both halves of the second click — the
/// press reports 2, and for its release (which reports 1) the press's count
/// is remembered — and only within [`DOUBLE_CLICK_WINDOW`] of the press, so
/// a press held longer than a real double click does not count.
pub fn is_double_click() -> bool {
    CLICKS.with(|c| {
        let state = c.get();
        (state.event_clicks == 2 || state.down_clicks == 2)
            && state.down_at.is_some_and(|at| {
                crate::clock::now().saturating_duration_since(at) < DOUBLE_CLICK_WINDOW
            })
    })
}

/// A press with `clicks` is about to be dispatched.
pub(crate) fn begin_press(clicks: u32, stated: bool) {
    let now = crate::clock::now();
    update(|s| {
        s.event_clicks = clicks;
        s.down_clicks = clicks;
        s.down_at = Some(now);
        s.stated = stated;
    });
}

/// A release is about to be dispatched: it reports 1.
pub(crate) fn begin_release() {
    update(|s| s.event_clicks = 1);
}

/// The release has been dispatched: the press is over, so the remembered
/// count no longer describes a live click. Without this a single click
/// arriving shortly after a double click could still see the stale 2.
pub(crate) fn end_release() {
    update(|s| {
        s.down_clicks = 0;
        s.stated = false;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_only_a_double_click_answers_from_its_release_until_the_release_is_done() {
        let _clock = crate::clock::scoped_virtual(None);
        begin_press(2, true);
        assert_eq!(current_click_count(), 2);
        assert_eq!(stated_click_count(), Some(2));
        assert!(is_double_click());
        begin_release();
        assert_eq!(current_click_count(), 1, "every release reports 1");
        assert!(is_double_click(), "the release remembers its press");
        end_release();
        assert!(!is_double_click(), "the press is over");
        assert_eq!(stated_click_count(), None);
    }

    #[test]
    fn rust_only_a_press_held_past_the_double_click_window_is_not_a_double_click() {
        let _clock = crate::clock::scoped_virtual(None);
        begin_press(2, false);
        assert_eq!(stated_click_count(), None, "a counted press is not stated");
        crate::clock::advance(DOUBLE_CLICK_WINDOW);
        begin_release();
        assert!(!is_double_click());
        end_release();
    }
}
