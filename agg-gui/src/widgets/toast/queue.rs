//! The toast timing state machine behind [`super::Toasts`]: which toasts are
//! up, how far each has faded in or out, when each expires, and the hover
//! pause.  Every method takes the current UI-clock instant (`crate::clock`),
//! so the same code runs on real time in an app and on the virtual clock in
//! tests.
//!
//! A toast's life: it fades in over [`FADE_IN`] from when it was shown; it
//! counts down its duration only while the queue is not paused (the pointer
//! is over a toast, see `host.rs`); when the countdown ends — or it is
//! dismissed — it starts *closing*, fades out over [`FADE_OUT`], and is then
//! dropped.  Closing is latched: pausing never revives a fading toast.

use std::time::Duration;

use web_time::Instant;

use super::{Toast, ToastId, ToastKind};

/// Fade-in time of a new toast.
pub const FADE_IN: Duration = Duration::from_millis(180);
/// Fade-out time of an expired or dismissed toast.
pub const FADE_OUT: Duration = Duration::from_millis(250);

/// One toast in the queue.
#[derive(Clone, Debug)]
pub(super) struct Entry {
    pub id: ToastId,
    pub text: String,
    pub kind: Option<ToastKind>,
    /// `None`: stays until dismissed.
    pub duration: Option<Duration>,
    pub shown_at: Instant,
    /// Time spent paused (hovered) since it was shown, excluding a pause
    /// still in progress.
    paused_for: Duration,
    /// When it started fading out.
    pub closing_since: Option<Instant>,
}

/// All toasts, oldest first, plus the hover pause.
#[derive(Debug)]
pub(super) struct Queue {
    pub entries: Vec<Entry>,
    next_id: u64,
    /// `Some(start)` while the pointer is over a toast.
    paused_since: Option<Instant>,
    /// Most toasts up at once (not counting ones fading out); older ones
    /// close when a new one would exceed it.
    pub max_visible: usize,
}

impl Default for Queue {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            next_id: 1,
            paused_since: None,
            max_visible: 5,
        }
    }
}

/// Ease-out cubic of `t` in `0..=1`.
fn ease_out(t: f64) -> f64 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

impl Entry {
    /// Countdown time used so far (shown time minus paused time).
    fn active_elapsed(&self, now: Instant, paused_since: Option<Instant>) -> Duration {
        let mut paused = self.paused_for;
        if let Some(p) = paused_since {
            paused += now.saturating_duration_since(p.max(self.shown_at));
        }
        now.saturating_duration_since(self.shown_at)
            .saturating_sub(paused)
    }

    /// Opacity in `0..=1`: the lesser of the fade-in and fade-out ramps.
    pub fn alpha(&self, now: Instant) -> f64 {
        let fade_in =
            now.saturating_duration_since(self.shown_at).as_secs_f64() / FADE_IN.as_secs_f64();
        let fade_out = match self.closing_since {
            Some(c) => {
                1.0 - now.saturating_duration_since(c).as_secs_f64() / FADE_OUT.as_secs_f64()
            }
            None => 1.0,
        };
        ease_out(fade_in.min(fade_out))
    }

    /// Whether it is still fading in or out at `now`.
    fn is_fading(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.shown_at) < FADE_IN || self.closing_since.is_some()
    }
}

impl Queue {
    /// Add `toast` at `now` and return its id; closes the oldest open
    /// toasts beyond [`Self::max_visible`].
    pub fn push(&mut self, toast: Toast, now: Instant) -> ToastId {
        let id = ToastId(self.next_id);
        self.next_id += 1;
        let duration = toast.resolved_duration();
        self.entries.push(Entry {
            id,
            text: toast.text,
            kind: toast.kind,
            duration,
            shown_at: now,
            paused_for: Duration::ZERO,
            closing_since: None,
        });
        let open: Vec<usize> = (0..self.entries.len())
            .filter(|&i| self.entries[i].closing_since.is_none())
            .collect();
        let excess = open.len().saturating_sub(self.max_visible.max(1));
        for &i in &open[..excess] {
            self.entries[i].closing_since = Some(now);
        }
        id
    }

    /// Start fading out `id` (no-op when it is unknown or already closing).
    /// Returns whether it changed.
    pub fn dismiss(&mut self, id: ToastId, now: Instant) -> bool {
        match self.entries.iter_mut().find(|e| e.id == id) {
            Some(e) if e.closing_since.is_none() => {
                e.closing_since = Some(now);
                true
            }
            _ => false,
        }
    }

    /// Start fading out every open toast.
    pub fn dismiss_all(&mut self, now: Instant) {
        for e in &mut self.entries {
            e.closing_since.get_or_insert(now);
        }
    }

    pub fn is_paused(&self) -> bool {
        self.paused_since.is_some()
    }

    /// Pause (pointer over a toast) or resume every countdown.  Expiries due
    /// before `now` are latched first, so a pause can't revive them.
    pub fn set_paused(&mut self, paused: bool, now: Instant) {
        self.tick(now);
        match (self.paused_since, paused) {
            (None, true) => self.paused_since = Some(now),
            (Some(start), false) => {
                for e in &mut self.entries {
                    e.paused_for += now.saturating_duration_since(start.max(e.shown_at));
                }
                self.paused_since = None;
            }
            _ => {}
        }
    }

    /// Latch expiries (closing from the exact instant the countdown ended)
    /// and drop toasts that finished fading out.  Returns whether anything
    /// was dropped.
    pub fn tick(&mut self, now: Instant) -> bool {
        let paused = self.paused_since;
        for e in &mut self.entries {
            if e.closing_since.is_some() || paused.is_some() {
                continue;
            }
            if let Some(d) = e.duration {
                let used = e.active_elapsed(now, None);
                if used >= d {
                    // `now - (used - d)`: when the countdown reached zero.
                    e.closing_since = Some(now - (used - d));
                }
            }
        }
        let before = self.entries.len();
        self.entries.retain(|e| match e.closing_since {
            Some(c) => now.saturating_duration_since(c) < FADE_OUT,
            None => true,
        });
        self.entries.len() != before
    }

    /// Whether any toast is fading in or out (needs a frame per tick).
    pub fn is_animating(&self, now: Instant) -> bool {
        self.entries.iter().any(|e| e.is_fading(now))
    }

    /// The next instant something changes without input: a countdown ends
    /// or a fade finishes.  `None` when nothing is pending (empty, paused
    /// with only open toasts, or only sticky ones).
    pub fn next_deadline(&self, now: Instant) -> Option<Instant> {
        self.entries
            .iter()
            .filter_map(|e| {
                if let Some(c) = e.closing_since {
                    return Some(c + FADE_OUT);
                }
                if self.paused_since.is_some() {
                    return None;
                }
                let left = e.duration?.saturating_sub(e.active_elapsed(now, None));
                Some(now + left)
            })
            .min()
    }
}
