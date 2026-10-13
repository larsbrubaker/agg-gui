//! Window-position persistence: the saved position type, the restore-time
//! clamp onto a connected monitor, and the tracker that remembers the last
//! *windowed* position.
//!
//! The counterpart of the size handling in [`crate::bounds`]. C#'s
//! `RootSystemWindow` stores `DesktopPosition` next to the window size and only
//! floors it at -10 (`SystemWindow.DesktopPosition` floors at 0); a window last
//! left on a monitor that is no longer connected therefore reopens off screen.
//! [`clamp_restored_position`] moves it back onto a monitor instead, which is
//! why it needs the monitor list: winit 0.30 exposes it from the event loop
//! before the window exists.

/// A persisted window position: the window's outer top-left corner in the
/// virtual desktop, in **physical pixels** (what winit's `Moved` reports and
/// `WindowAttributes::with_position` takes as `PhysicalPosition`). Negative
/// values are normal on a multi-monitor desktop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SavedPosition {
    pub x: i32,
    pub y: i32,
}

/// One connected monitor's rectangle in the virtual desktop, physical pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MonitorRect {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

impl MonitorRect {
    fn right(&self) -> i64 {
        i64::from(self.x) + i64::from(self.width)
    }

    fn bottom(&self) -> i64 {
        i64::from(self.y) + i64::from(self.height)
    }
}

/// How far above a monitor's top edge a window may sit and still count as on
/// it (C#'s `Math.Max(savedY, -10)`): Windows reports a window with an
/// invisible border slightly above the top of the screen.
const TOP_SLACK: i64 = 10;

/// The part of the window that must be on one monitor for the user to be able
/// to grab it: a title-bar-sized strip, but never more than the window itself.
const MIN_VISIBLE_W: i64 = 128;
const MIN_VISIBLE_H: i64 = 48;

/// Where to open a window of `size` (physical px) that was last at `saved`,
/// given the connected `monitors`.
///
/// - No monitors known: `None`, so the OS places the window.
/// - The window (at `saved`) shows a title-bar-sized part on some monitor and
///   does not start above its top edge: `saved` unchanged.
/// - Otherwise the window moves onto the monitor it overlaps most, or the
///   nearest one when it overlaps none (an unplugged secondary), keeping as
///   much of its saved offset as fits.
pub(crate) fn clamp_restored_position(
    saved: SavedPosition,
    size: (u32, u32),
    monitors: &[MonitorRect],
) -> Option<SavedPosition> {
    if monitors.is_empty() {
        return None;
    }
    let win = Rect::new(saved.x, saved.y, size.0, size.1);
    let need_w = MIN_VISIBLE_W.min(i64::from(size.0));
    let need_h = MIN_VISIBLE_H.min(i64::from(size.1));
    let reachable = monitors.iter().any(|m| {
        let (w, h) = win.overlap(m);
        w >= need_w && h >= need_h && i64::from(saved.y) >= i64::from(m.y) - TOP_SLACK
    });
    if reachable {
        return Some(saved);
    }

    // Largest overlap wins; with no overlap at all, the closest monitor
    // (distance between rectangle edges), the earlier one on a tie.
    let target = monitors
        .iter()
        .enumerate()
        .max_by_key(|(i, m)| {
            let (w, h) = win.overlap(m);
            (w * h, -distance(&win, m), -(*i as i64))
        })
        .map(|(_, m)| *m)?;

    let fit = |pos: i32, extent: u32, origin: i32, span: u32| -> i32 {
        let lo = i64::from(origin);
        // A window larger than the monitor is pinned to its origin.
        let hi = (lo + i64::from(span) - i64::from(extent)).max(lo);
        i64::from(pos).clamp(lo, hi) as i32
    };
    Some(SavedPosition {
        x: fit(saved.x, size.0, target.x, target.width),
        y: fit(saved.y, size.1, target.y, target.height),
    })
}

struct Rect {
    left: i64,
    top: i64,
    right: i64,
    bottom: i64,
}

impl Rect {
    fn new(x: i32, y: i32, w: u32, h: u32) -> Self {
        Self {
            left: i64::from(x),
            top: i64::from(y),
            right: i64::from(x) + i64::from(w),
            bottom: i64::from(y) + i64::from(h),
        }
    }

    /// Width and height of the intersection with `m`, zero when disjoint.
    fn overlap(&self, m: &MonitorRect) -> (i64, i64) {
        let w = self.right.min(m.right()) - self.left.max(i64::from(m.x));
        let h = self.bottom.min(m.bottom()) - self.top.max(i64::from(m.y));
        (w.max(0), h.max(0))
    }
}

/// Manhattan gap between the window rectangle and a monitor (0 if touching or
/// overlapping on both axes).
fn distance(win: &Rect, m: &MonitorRect) -> i64 {
    let dx = (i64::from(m.x) - win.right)
        .max(win.left - m.right())
        .max(0);
    let dy = (i64::from(m.y) - win.bottom)
        .max(win.top - m.bottom())
        .max(0);
    dx + dy
}

/// The last window position seen while the window was normal: not maximized,
/// not fullscreen and not minimized. Maximizing moves the window to the
/// work-area origin, fullscreen to the monitor origin, and Windows parks a
/// minimized window at (-32000, -32000); none of those is where the user put
/// the window, so none is persisted (C# also saves the position only when not
/// maximized).
#[derive(Clone, Copy, Debug)]
pub(crate) struct WindowedPositionTracker {
    position: Option<SavedPosition>,
}

impl WindowedPositionTracker {
    /// Seed with the position the window was opened at, or `None` when the
    /// platform cannot report one (Wayland).
    pub(crate) fn new(initial: Option<SavedPosition>) -> Self {
        Self { position: initial }
    }

    pub(crate) fn note_move(
        &mut self,
        position: SavedPosition,
        maximized: bool,
        fullscreen: bool,
        minimized: bool,
    ) {
        if !maximized && !fullscreen && !minimized {
            self.position = Some(position);
        }
    }

    pub(crate) fn position(&self) -> Option<SavedPosition> {
        self.position
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mon(x: i32, y: i32, width: u32, height: u32) -> MonitorRect {
        MonitorRect {
            x,
            y,
            width,
            height,
        }
    }

    fn pos(x: i32, y: i32) -> SavedPosition {
        SavedPosition { x, y }
    }

    const MAIN: MonitorRect = MonitorRect {
        x: 0,
        y: 0,
        width: 1920,
        height: 1080,
    };

    #[test]
    fn a_position_on_a_monitor_is_kept() {
        assert_eq!(
            clamp_restored_position(pos(300, 200), (1280, 720), &[MAIN]),
            Some(pos(300, 200))
        );
    }

    #[test]
    fn no_monitors_leaves_placement_to_the_os() {
        assert_eq!(
            clamp_restored_position(pos(300, 200), (800, 600), &[]),
            None
        );
    }

    #[test]
    fn a_window_partly_off_the_edge_stays_while_its_title_bar_is_reachable() {
        // Hanging 700 px off the right edge: 580 px (more than the strip) remain.
        assert_eq!(
            clamp_restored_position(pos(1300, 100), (1280, 720), &[MAIN]),
            Some(pos(1300, 100))
        );
        // A drop below the bottom still shows 100 px of window.
        assert_eq!(
            clamp_restored_position(pos(100, 980), (800, 600), &[MAIN]),
            Some(pos(100, 980))
        );
    }

    #[test]
    fn an_invisible_border_above_the_screen_is_kept_but_a_lost_title_bar_is_not() {
        assert_eq!(
            clamp_restored_position(pos(0, -8), (800, 600), &[MAIN]),
            Some(pos(0, -8))
        );
        assert_eq!(
            clamp_restored_position(pos(0, -200), (800, 600), &[MAIN]),
            Some(pos(0, 0))
        );
    }

    #[test]
    fn a_window_on_an_unplugged_monitor_moves_to_the_nearest_one() {
        // Saved on a second monitor to the right that is no longer there.
        let out = clamp_restored_position(pos(2500, 300), (1280, 720), &[MAIN]);
        assert_eq!(out, Some(pos(640, 300)));
        // Far to the left and above: pinned to the top-left corner.
        let out = clamp_restored_position(pos(-5000, -3000), (1280, 720), &[MAIN]);
        assert_eq!(out, Some(pos(0, 0)));
    }

    #[test]
    fn a_window_between_two_monitors_goes_to_the_one_it_overlaps_most() {
        let left = mon(-1920, 0, 1920, 1080);
        // Straddling both, with its title bar above the screen: 300 px of it
        // is over the left monitor and 700 over the main one.
        let out = clamp_restored_position(pos(-300, -200), (1000, 600), &[left, MAIN]);
        assert_eq!(out, Some(pos(0, 0)));
        // A reachable window on the left monitor is kept where it is.
        let out = clamp_restored_position(pos(-1500, 50), (1000, 600), &[left, MAIN]);
        assert_eq!(out, Some(pos(-1500, 50)));
    }

    #[test]
    fn with_two_monitors_a_lost_one_falls_to_the_closest_remaining() {
        let main = mon(0, 0, 1920, 1080);
        let right = mon(1920, 0, 1920, 1080);
        // The window sat on a third monitor far to the right.
        let out = clamp_restored_position(pos(6000, 100), (800, 600), &[main, right]);
        assert_eq!(out, Some(pos(1920 + 1920 - 800, 100)));
    }

    #[test]
    fn a_window_larger_than_the_monitor_is_pinned_to_its_origin() {
        let small = mon(0, 0, 1024, 768);
        assert_eq!(
            clamp_restored_position(pos(4000, 4000), (2000, 1500), &[small]),
            Some(pos(0, 0))
        );
    }

    #[test]
    fn a_tiny_window_only_needs_itself_to_be_visible() {
        // 100x40 window fully on screen: the strip requirement shrinks to it.
        assert_eq!(
            clamp_restored_position(pos(10, 10), (100, 40), &[MAIN]),
            Some(pos(10, 10))
        );
    }

    #[test]
    fn a_negative_origin_monitor_keeps_a_position_on_it() {
        let top = mon(0, -1080, 1920, 1080);
        assert_eq!(
            clamp_restored_position(pos(200, -900), (1000, 600), &[MAIN, top]),
            Some(pos(200, -900))
        );
    }

    #[test]
    fn the_tracker_follows_normal_moves_only() {
        let mut t = WindowedPositionTracker::new(Some(pos(100, 100)));
        t.note_move(pos(200, 150), false, false, false);
        assert_eq!(t.position(), Some(pos(200, 150)));
        // Maximize, fullscreen and minimize each move the window somewhere
        // that is not where the user put it.
        t.note_move(pos(0, 0), true, false, false);
        t.note_move(pos(0, 0), false, true, false);
        t.note_move(pos(-32000, -32000), false, false, true);
        assert_eq!(t.position(), Some(pos(200, 150)));
    }

    #[test]
    fn the_tracker_starts_empty_when_the_platform_reports_no_position() {
        let mut t = WindowedPositionTracker::new(None);
        assert_eq!(t.position(), None);
        t.note_move(pos(5, 6), false, false, false);
        assert_eq!(t.position(), Some(pos(5, 6)));
    }
}
