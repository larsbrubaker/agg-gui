//! Fixed-timestep scheduler for deterministic time-stepped simulation.
//!
//! Rendering can happen at any cadence, but physics, gameplay, or any other
//! deterministic simulation runs in fixed slices (60 Hz by default). When
//! rendering falls behind, the scheduler catches up by running multiple
//! simulation steps before the next draw, capped so a long pause or slow frame
//! never turns into one huge step.
//!
//! Useful well beyond games: gesture inertia, physics-based UI animations
//! (fling-scroll, spring-damped panels), demos/visualizations, and any retained
//! widget that wants stable simulation independent of render rate.
//!
//! # Example
//! ```
//! use agg_gui::timestep::FixedTimestep;
//! let mut timestep = FixedTimestep::new();
//! // Inside the host's per-frame callback, with `elapsed` the wall-clock
//! // delta since the previous frame:
//! # let elapsed = 1.0_f32 / 60.0;
//! let batch = timestep.advance(elapsed);
//! for _ in 0..batch.steps {
//!     // step_simulation(batch.dt);
//! }
//! ```

/// Default simulation frequency (Hz).
pub const SIMULATION_HZ: f32 = 60.0;

/// Default fixed simulation step in seconds (`1.0 / SIMULATION_HZ`).
pub const FIXED_DT: f32 = 1.0 / SIMULATION_HZ;

/// Default maximum simulation work before one draw.
///
/// Four 60 Hz updates per draw is equivalent to drawing at 15 fps. If the app
/// falls further behind, excess wall-clock time is dropped and the simulation
/// slows down instead of making collision-unsafe jumps.
pub const MAX_STEPS_PER_DRAW: u32 = 4;

const MAX_ACCUMULATED_TIME: f32 = FIXED_DT * MAX_STEPS_PER_DRAW as f32;

/// One scheduling decision: how many fixed steps to run, with what `dt`, plus
/// any wall-clock time that was dropped to avoid catastrophic catch-up.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StepBatch {
    pub steps: u32,
    pub dt: f32,
    pub dropped_time: f32,
}

/// Accumulating fixed-timestep scheduler.
///
/// Defaults to 60 Hz with a 4-step catch-up cap. Pass elapsed wall time into
/// [`FixedTimestep::advance`]; it returns a [`StepBatch`] describing the
/// simulation work for the upcoming draw.
#[derive(Debug, Clone)]
pub struct FixedTimestep {
    accumulated: f32,
}

impl FixedTimestep {
    pub fn new() -> Self {
        Self { accumulated: 0.0 }
    }

    /// Accumulate elapsed wall time and return how many fixed-`FIXED_DT` updates
    /// to run before the next draw.
    pub fn advance(&mut self, elapsed_seconds: f32) -> StepBatch {
        let elapsed = elapsed_seconds.max(0.0);
        self.accumulated += elapsed;

        let dropped_time = if self.accumulated > MAX_ACCUMULATED_TIME {
            let dropped = self.accumulated - MAX_ACCUMULATED_TIME;
            self.accumulated = MAX_ACCUMULATED_TIME;
            dropped
        } else {
            0.0
        };

        let steps = ((self.accumulated / FIXED_DT).floor() as u32).min(MAX_STEPS_PER_DRAW);
        self.accumulated -= steps as f32 * FIXED_DT;

        StepBatch {
            steps,
            dt: FIXED_DT,
            dropped_time,
        }
    }

    pub fn reset(&mut self) {
        self.accumulated = 0.0;
    }
}

impl Default for FixedTimestep {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq(a: f32, b: f32) {
        assert!((a - b).abs() < 0.000_01, "{a} != {b}");
    }

    #[test]
    fn runs_one_step_for_one_sixtieth() {
        let mut timestep = FixedTimestep::new();
        let batch = timestep.advance(FIXED_DT);

        assert_eq!(batch.steps, 1);
        approx_eq(batch.dt, FIXED_DT);
        approx_eq(batch.dropped_time, 0.0);
    }

    #[test]
    fn accumulates_fractional_frames() {
        let mut timestep = FixedTimestep::new();

        assert_eq!(timestep.advance(FIXED_DT * 0.5).steps, 0);
        assert_eq!(timestep.advance(FIXED_DT * 0.5).steps, 1);
    }

    #[test]
    fn catches_up_to_fifteen_fps_and_drops_the_rest() {
        let mut timestep = FixedTimestep::new();
        let batch = timestep.advance(1.0);

        assert_eq!(batch.steps, MAX_STEPS_PER_DRAW);
        assert!(batch.dropped_time > 0.9);
        assert_eq!(timestep.advance(0.0).steps, 0);
    }

    #[test]
    fn ignores_negative_elapsed_time() {
        let mut timestep = FixedTimestep::new();
        let batch = timestep.advance(-1.0);

        assert_eq!(batch.steps, 0);
        approx_eq(batch.dropped_time, 0.0);
    }
}
