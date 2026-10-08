//! Shared multi-click (single / double / triple) detection for the text
//! editors.
//!
//! `TextField`, `TextArea` and `RichTextEdit` all want the same "click again
//! quickly to select more" behaviour: a first press positions the caret, a
//! second selects the word, a third selects the line/paragraph/block. Rather
//! than each widget re-deriving the timing gate, they share this tracker so the
//! window and travel tolerance match each other — and match the Scene's
//! double-click reset (`scene::DBL_CLICK_MS` / `MAX_CLICK_DIST`). A press
//! that states its click count (simulated input through
//! `App::on_mouse_down_clicks`) is taken at that count.

use crate::geometry::Point;
use web_time::Instant;

/// Time window (ms) within which a follow-up press counts as continuing a
/// multi-click sequence. Matches the Scene's `DBL_CLICK_MS`.
const MULTI_CLICK_MS: u128 = 400;

/// [`MULTI_CLICK_MS`] as a `Duration` — the default double-click time of
/// `shell_input::ClickPolicy`, so window-level and editor click counting agree.
pub(crate) const MULTI_CLICK_TIME: std::time::Duration =
    std::time::Duration::from_millis(MULTI_CLICK_MS as u64);

/// Maximum pointer travel (px) between presses that still counts as the same
/// multi-click sequence. Matches the Scene's `MAX_CLICK_DIST`.
pub(crate) const MULTI_CLICK_DIST: f64 = 6.0;

/// Granularity of an in-progress selection drag, decided by the click that
/// began it. A double-click starts a `Word` drag (extends by whole words), a
/// triple-click a `Line` drag; an ordinary press stays `Char`.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum SelectGranularity {
    #[default]
    Char,
    Word,
    Line,
}

/// Tracks consecutive mouse presses to distinguish single, double and triple
/// clicks. One lives per text-editor widget instance.
#[derive(Default)]
pub struct MultiClickTracker {
    last_time: Option<Instant>,
    last_pos: Option<Point>,
    count: u32,
}

impl MultiClickTracker {
    /// Register a press at `pos`, returning the click count in the current
    /// sequence: `1` = single, `2` = double, `3` = triple. A fourth quick click
    /// wraps back to `1`, and any press outside the time window or travel
    /// tolerance restarts the sequence at `1`.
    ///
    /// A press whose sender stated its click count
    /// ([`crate::event::stated_click_count`], e.g. a simulated double
    /// click) is taken at that count instead; see
    /// [`register_with_count`](Self::register_with_count).
    pub fn register(&mut self, pos: Point) -> u32 {
        self.register_with_count(pos, crate::event::stated_click_count())
    }

    /// Register a press at `pos` whose click count is `given` when known.
    /// `Some(n)` is honoured as the press's place in the sequence (wrapping
    /// past triple as the timing does: 4 → 1, 5 → 2, ...); `None` counts by
    /// the time window and travel tolerance, as [`register`](Self::register)
    /// does for a real press.
    pub fn register_with_count(&mut self, pos: Point, given: Option<u32>) -> u32 {
        let now = crate::clock::now();
        if let Some(n) = given {
            self.count = (n.max(1) - 1) % 3 + 1;
            self.last_time = Some(now);
            self.last_pos = Some(pos);
            return self.count;
        }
        let in_time = self
            .last_time
            .map(|t| now.duration_since(t).as_millis() < MULTI_CLICK_MS)
            .unwrap_or(false);
        let near = self
            .last_pos
            .map(|p| {
                let dx = pos.x - p.x;
                let dy = pos.y - p.y;
                dx * dx + dy * dy <= MULTI_CLICK_DIST * MULTI_CLICK_DIST
            })
            .unwrap_or(false);
        self.count = if in_time && near {
            // Cap the sequence at triple; a fourth quick click starts over.
            if self.count >= 3 {
                1
            } else {
                self.count + 1
            }
        } else {
            1
        };
        self.last_time = Some(now);
        self.last_pos = Some(pos);
        self.count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_press_reports_one() {
        let mut t = MultiClickTracker::default();
        assert_eq!(t.register(Point::new(10.0, 10.0)), 1);
    }

    #[test]
    fn two_quick_presses_report_double_then_triple() {
        let mut t = MultiClickTracker::default();
        assert_eq!(t.register(Point::new(10.0, 10.0)), 1);
        assert_eq!(t.register(Point::new(10.0, 10.0)), 2);
        assert_eq!(t.register(Point::new(10.0, 10.0)), 3);
        // Fourth quick click restarts the sequence.
        assert_eq!(t.register(Point::new(10.0, 10.0)), 1);
    }

    #[test]
    fn rust_only_a_given_count_is_honoured_and_wraps_past_triple() {
        let mut t = MultiClickTracker::default();
        let p = Point::new(10.0, 10.0);
        assert_eq!(t.register_with_count(p, Some(2)), 2);
        assert_eq!(t.register_with_count(p, Some(1)), 1);
        assert_eq!(t.register_with_count(p, Some(4)), 1);
        assert_eq!(t.register_with_count(p, Some(5)), 2);
        // A counted press continues from the given count.
        assert_eq!(t.register_with_count(p, None), 3);
    }

    #[test]
    fn far_second_press_restarts_sequence() {
        let mut t = MultiClickTracker::default();
        assert_eq!(t.register(Point::new(10.0, 10.0)), 1);
        // Beyond the 6 px travel tolerance — a fresh single click.
        assert_eq!(t.register(Point::new(40.0, 10.0)), 1);
    }
}
