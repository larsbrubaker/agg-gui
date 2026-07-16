//! Time-coalescing undo/redo history — a faithful port of egui's
//! `egui::util::undoer::Undoer<State>` (see
//! `egui-reference/crates/egui/src/util/undoer.rs`).
//!
//! A single [`Undoer`] snapshots a *whole* state value `State` with the
//! time-based coalescing egui demonstrates: an undo point is created once the
//! state has been stable for `stable_time`, or forcibly every
//! `auto_save_interval` if the user never stops changing it.
//!
//! Lives in the [`crate::undo`] module alongside the command-history
//! [`UndoBuffer`](super::UndoBuffer): they are complementary models (snapshot
//! vs. command). The RichTextEdit widget snapshots `{RichDoc, caret}` through
//! it, and the dialogs demo snapshots its `{toggle, text}`.

use std::collections::VecDeque;

/// Tuning knobs for [`Undoer`]. Mirrors egui's `Settings`.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// Maximum number of undo points retained.
    pub max_undos: usize,
    /// Once the state has been unchanged for this many seconds, create a new
    /// undo point (if one is needed).
    pub stable_time: f32,
    /// If the state keeps changing and never stabilises, still create a save
    /// point every this many seconds so there is always something to undo to.
    pub auto_save_interval: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            max_undos: 100,
            stable_time: 1.0,
            auto_save_interval: 30.0,
        }
    }
}

/// Represents how the current state is changing between feeds.
#[derive(Clone)]
struct Flux<State> {
    start_time: f64,
    latest_change_time: f64,
    latest_state: State,
}

/// Automatic undo system. Feed it the latest state every frame; it decides
/// when to snapshot a new undo point.
#[derive(Clone)]
pub struct Undoer<State> {
    settings: Settings,
    /// New undo points are pushed to the back. Two adjacent points are never
    /// equal; the latest may (often) equal the current state.
    undos: VecDeque<State>,
    /// Redos, populated by `undo` and cleared whenever the state changes.
    redos: Vec<State>,
    flux: Option<Flux<State>>,
}

impl<State> Default for Undoer<State>
where
    State: Clone + PartialEq,
{
    fn default() -> Self {
        Self {
            settings: Settings::default(),
            undos: VecDeque::new(),
            redos: Vec::new(),
            flux: None,
        }
    }
}

impl<State> Undoer<State>
where
    State: Clone + PartialEq,
{
    /// Create an [`Undoer`] with custom [`Settings`] (used by tests to shrink
    /// the coalescing windows).
    #[allow(dead_code)]
    pub fn with_settings(settings: Settings) -> Self {
        Self {
            settings,
            ..Default::default()
        }
    }

    /// Do we have an undo point different from the given state?
    pub fn has_undo(&self, current_state: &State) -> bool {
        match self.undos.len() {
            0 => false,
            1 => self.undos.back() != Some(current_state),
            _ => true,
        }
    }

    /// Do we have a redo point available for the given state?
    pub fn has_redo(&self, current_state: &State) -> bool {
        !self.redos.is_empty() && self.undos.back() == Some(current_state)
    }

    /// Is the state currently mid-change (a flux is being tracked)?
    pub fn is_in_flux(&self) -> bool {
        self.flux.is_some()
    }

    /// Undo one step, returning the state to revert to (if any).
    pub fn undo(&mut self, current_state: &State) -> Option<&State> {
        if self.has_undo(current_state) {
            self.flux = None;

            if self.undos.back() == Some(current_state) {
                // The latest undo point IS the current state — move it to redos.
                if let Some(popped) = self.undos.pop_back() {
                    self.redos.push(popped);
                }
            } else {
                self.redos.push(current_state.clone());
            }

            // Keep the (new) latest undo point intact.
            self.undos.back()
        } else {
            None
        }
    }

    /// Redo one step, returning the state to advance to (if any).
    pub fn redo(&mut self, current_state: &State) -> Option<&State> {
        if !self.undos.is_empty() && self.undos.back() != Some(current_state) {
            // State changed since the last undo — redos are stale, clear them.
            self.redos.clear();
            None
        } else if let Some(state) = self.redos.pop() {
            self.undos.push_back(state);
            self.undos.back()
        } else {
            None
        }
    }

    /// Add an undo point iff the state differs from the latest point.
    pub fn add_undo(&mut self, current_state: &State) {
        if self.undos.back() != Some(current_state) {
            self.undos.push_back(current_state.clone());
        }
        while self.undos.len() > self.settings.max_undos {
            self.undos.pop_front();
        }
        self.flux = None;
    }

    /// Feed the most recent state (call every frame). `current_time` is in
    /// seconds. Implements the two coalescing rules described on egui's
    /// `Undoer`.
    pub fn feed_state(&mut self, current_time: f64, current_state: &State) {
        match self.undos.back() {
            None => {
                // First feed ever — always snapshot.
                self.add_undo(current_state);
            }
            Some(latest_undo) => {
                if latest_undo == current_state {
                    self.flux = None;
                } else {
                    self.redos.clear();

                    match self.flux.as_mut() {
                        None => {
                            self.flux = Some(Flux {
                                start_time: current_time,
                                latest_change_time: current_time,
                                latest_state: current_state.clone(),
                            });
                        }
                        Some(flux) => {
                            if &flux.latest_state == current_state {
                                let time_since_latest_change =
                                    (current_time - flux.latest_change_time) as f32;
                                if time_since_latest_change >= self.settings.stable_time {
                                    self.add_undo(current_state);
                                }
                            } else {
                                let time_since_flux_start =
                                    (current_time - flux.start_time) as f32;
                                if time_since_flux_start >= self.settings.auto_save_interval {
                                    self.add_undo(current_state);
                                } else {
                                    flux.latest_change_time = current_time;
                                    flux.latest_state = current_state.clone();
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, PartialEq, Debug)]
    struct S(i32);

    fn undoer() -> Undoer<S> {
        Undoer::with_settings(Settings {
            max_undos: 100,
            stable_time: 1.0,
            auto_save_interval: 30.0,
        })
    }

    #[test]
    fn first_feed_creates_baseline_and_no_undo() {
        let mut u = undoer();
        u.feed_state(0.0, &S(0));
        assert!(!u.has_undo(&S(0)));
        assert!(!u.has_redo(&S(0)));
    }

    #[test]
    fn stable_time_coalesces_rapid_changes_into_one_point() {
        let mut u = undoer();
        u.feed_state(0.0, &S(0)); // baseline S(0)

        // Rapid changes within stable_time keep the state "in flux" — the
        // intermediate values S(1), S(2) must NOT each become undo points.
        u.feed_state(0.1, &S(1));
        assert!(u.is_in_flux());
        u.feed_state(0.2, &S(2));
        u.feed_state(0.3, &S(3));

        // State holds steady at S(3); once stable_time elapses, ONE point forms.
        u.feed_state(0.4, &S(3)); // arms the stability timer at 0.4
        u.feed_state(1.5, &S(3)); // 1.1s later ≥ stable_time → snapshot S(3)
        assert!(u.has_undo(&S(3)));
        assert!(!u.is_in_flux());

        // Coalescing proof: a single undo jumps straight back to the baseline
        // S(0) (the intermediates never became their own points), after which
        // there is nothing left to undo.
        assert_eq!(u.undo(&S(3)).cloned(), Some(S(0)));
        assert!(!u.has_undo(&S(0)));
    }

    #[test]
    fn undo_then_redo_round_trips() {
        let mut u = undoer();
        u.feed_state(0.0, &S(0)); // baseline S(0)

        // Change to S(5) and let it stabilise into an undo point.
        u.feed_state(0.1, &S(5));
        u.feed_state(0.2, &S(5));
        u.feed_state(2.0, &S(5)); // stable ≥ 1s → snapshot S(5)
        assert!(u.has_undo(&S(5)));

        // Undo back to S(0).
        let reverted = u.undo(&S(5)).cloned();
        assert_eq!(reverted, Some(S(0)));
        assert!(u.has_redo(&S(0)));

        // Redo forward to S(5).
        let advanced = u.redo(&S(0)).cloned();
        assert_eq!(advanced, Some(S(5)));
        assert!(!u.has_redo(&S(5)));
    }

    #[test]
    fn auto_save_interval_forces_point_during_continuous_change() {
        let mut u = undoer();
        u.feed_state(0.0, &S(0));
        // Never stabilise: every feed is a new value.
        for i in 1..=40 {
            u.feed_state(i as f64, &S(i)); // 1s apart, always changing
        }
        // By 30s of continuous flux, at least one point must have been forced.
        assert!(u.has_undo(&S(40)));
    }

    #[test]
    fn changing_state_clears_redos() {
        let mut u = undoer();
        u.feed_state(0.0, &S(0));
        u.feed_state(0.1, &S(1));
        u.feed_state(2.0, &S(1)); // snapshot S(1)
        u.undo(&S(1)); // now redo available
        assert!(u.has_redo(&S(0)));

        // Diverge to a brand-new value: redo history must be discarded.
        u.feed_state(2.1, &S(9));
        assert!(!u.has_redo(&S(9)));
    }
}
