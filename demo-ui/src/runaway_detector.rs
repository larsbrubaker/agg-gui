//! Pure, testable runaway-repaint detector for the demo shells.
//!
//! The library has a rare, still-unreproduced bug where the reactive host
//! never quiesces — it keeps painting frame after frame with no input.  The
//! shells feed each rendered frame into a [`RunawayDetector`]; after enough
//! consecutive input-free frames in reactive mode it fires ONCE (latched),
//! so the shell can emit a `debug_draw_report` tagged as auto-detected.  The
//! latch clears when the app finally goes idle, so a later runaway is caught
//! again.
//!
//! Kept free of any I/O or `App` reference so it can be unit-tested in
//! isolation — the shell owns the report emission; this struct only decides
//! *when*.

/// Consecutive input-free reactive frames before a runaway is declared.
/// 240 frames ≈ 4 s at 60 fps — long enough that no legitimate animation
/// (tweens settle in well under a second) trips it, short enough that a real
/// runaway is captured while the user is still looking at it.
pub const DEFAULT_RUNAWAY_THRESHOLD: u32 = 240;

/// Tracks consecutive input-free frames rendered in reactive mode and latches
/// a single "runaway detected" signal once the threshold is crossed.
#[derive(Debug, Clone)]
pub struct RunawayDetector {
    threshold: u32,
    frames_without_input: u32,
    latched: bool,
}

impl RunawayDetector {
    pub fn new(threshold: u32) -> Self {
        Self {
            threshold,
            frames_without_input: 0,
            latched: false,
        }
    }

    /// Record one rendered frame.
    ///
    /// * `reactive` — the host is in reactive (not continuous) run mode.
    ///   Continuous mode paints every frame by design, so it can never be a
    ///   runaway and resets the counter.
    /// * `had_input` — an input event arrived since the previous frame.  Input
    ///   legitimately drives repaints, so it resets the counter.
    ///
    /// Returns `true` exactly once, on the frame the threshold is first
    /// exceeded, until [`note_idle`](Self::note_idle) clears the latch.
    pub fn note_frame(&mut self, reactive: bool, had_input: bool) -> bool {
        if !reactive || had_input {
            self.frames_without_input = 0;
            return false;
        }
        self.frames_without_input = self.frames_without_input.saturating_add(1);
        if self.frames_without_input > self.threshold && !self.latched {
            self.latched = true;
            return true;
        }
        false
    }

    /// The app went idle (a loop iteration rendered no frame).  Resets the
    /// counter and clears the latch so a subsequent runaway re-fires.
    pub fn note_idle(&mut self) {
        self.frames_without_input = 0;
        self.latched = false;
    }

    /// Current consecutive input-free frame count — exposed for diagnostics.
    pub fn frames_without_input(&self) -> u32 {
        self.frames_without_input
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fires_once_after_threshold_then_latches() {
        let mut d = RunawayDetector::new(3);
        // Below + at threshold: no fire.
        assert!(!d.note_frame(true, false)); // 1
        assert!(!d.note_frame(true, false)); // 2
        assert!(!d.note_frame(true, false)); // 3 (== threshold, not yet over)
        // One past the threshold: fires exactly once.
        assert!(d.note_frame(true, false)); // 4 (> threshold)
        assert!(!d.note_frame(true, false)); // latched — no repeat
        assert!(!d.note_frame(true, false));
    }

    #[test]
    fn input_resets_the_counter() {
        let mut d = RunawayDetector::new(3);
        for _ in 0..3 {
            assert!(!d.note_frame(true, false));
        }
        // An input frame resets progress toward the threshold.
        assert!(!d.note_frame(true, true));
        assert!(!d.note_frame(true, false)); // back to 1
        assert!(!d.note_frame(true, false)); // 2
        assert!(!d.note_frame(true, false)); // 3
        assert!(d.note_frame(true, false)); // 4 -> fires
    }

    #[test]
    fn continuous_mode_never_fires() {
        let mut d = RunawayDetector::new(2);
        for _ in 0..100 {
            assert!(!d.note_frame(false, false));
        }
    }

    #[test]
    fn idle_clears_latch_so_a_later_runaway_refires() {
        let mut d = RunawayDetector::new(2);
        assert!(!d.note_frame(true, false)); // 1
        assert!(!d.note_frame(true, false)); // 2
        assert!(d.note_frame(true, false)); // 3 -> fire
        assert!(!d.note_frame(true, false)); // latched
        // App goes idle: latch + counter reset.
        d.note_idle();
        assert_eq!(d.frames_without_input(), 0);
        // A fresh runaway must fire again.
        assert!(!d.note_frame(true, false)); // 1
        assert!(!d.note_frame(true, false)); // 2
        assert!(d.note_frame(true, false)); // 3 -> fire again
    }
}
