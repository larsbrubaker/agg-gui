//! The rich editor's caret blink — a port of agg-sharp's `RichCaretBlink.cs`.
//!
//! Timed as the plain text field does it: shown for 0.6 s, hidden for 0.6 s,
//! read from agg-gui's UI clock ([`crate::clock`]) rather than from counted
//! callbacks, so a test on the virtual clock steps it exactly.  Any caret move
//! restarts it showing, so the caret never vanishes the moment it lands.
//!
//! C# kept a chain of idle callbacks alive (a generation number retiring a
//! chain an earlier focus left behind) that repainted the owner at each edge.
//! agg-gui wakes the frame loop with a deadline instead: [`RichCaretBlink::restart`]
//! and [`RichCaretBlink::tick`] ask for a draw at the next edge
//! ([`crate::animation::request_draw_after_tagged`]), and the widget calls
//! `tick` from each paint, which re-arms the next edge while the editor is
//! active or stops the blink when it is not.  A deadline is re-armed only by
//! the live blink, so a stale chain cannot exist.

use std::time::Duration;

use web_time::Instant;

const ON_MS: u128 = 600;
const OFF_MS: u128 = 600;
const CYCLE_MS: u128 = ON_MS + OFF_MS;

/// The caret blink (C#'s `RichCaretBlink`); stopped until first restarted.
#[derive(Clone, Copy, Debug, Default)]
pub struct RichCaretBlink {
    started: Option<Instant>,
}

impl RichCaretBlink {
    /// A stopped blink: the caret shows.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the blink is running.
    pub fn is_running(&self) -> bool {
        self.started.is_some()
    }

    fn elapsed_ms(&self) -> Option<u128> {
        self.started
            .map(|started| crate::clock::since(started).as_millis())
    }

    /// Whether the caret is in the showing half of its cycle; always true
    /// while the blink is stopped.
    pub fn showing(&self) -> bool {
        self.elapsed_ms().map_or(true, |ms| ms % CYCLE_MS < ON_MS)
    }

    /// Shows the caret now and blinks from here while the editor is `active`
    /// (focused and open); stops the blink when it is not.  Returns whether
    /// the owner must repaint (C#'s `owner.Invalidate()`).
    pub fn restart(&mut self, active: bool) -> bool {
        if !active {
            self.started = None;
            return false;
        }

        self.started = Some(crate::clock::now());
        self.schedule();
        true
    }

    /// Called from each paint of the owner: while `active`, asks for a draw at
    /// the next edge of the blink; otherwise stops it.
    pub fn tick(&mut self, active: bool) {
        if self.started.is_none() {
            return;
        }
        if active {
            self.schedule();
        } else {
            self.started = None;
        }
    }

    /// Time until the next edge of the blink — when the caret must be painted
    /// the other way — at least 1 ms; `None` while stopped.
    pub fn until_edge(&self) -> Option<Duration> {
        let position = self.elapsed_ms()? % CYCLE_MS;
        let until = if position < ON_MS {
            ON_MS - position
        } else {
            CYCLE_MS - position
        };
        Some(Duration::from_millis(until.max(1) as u64))
    }

    // Wakes at the next edge of the blink and no more often.
    fn schedule(&self) {
        if let Some(delay) = self.until_edge() {
            crate::animation::request_draw_after_tagged(delay, "markdown_editing::caret_blink");
        }
    }
}
