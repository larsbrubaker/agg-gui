//! When the canvas's backing store takes a new size: at once, or held until a
//! tick that paints.
//!
//! Setting a canvas's `width`/`height` discards its image, and the browser
//! composites the cleared canvas (black) until something is drawn into it.
//! While a window or the devtools panel is dragged, the canvas's CSS size
//! changes every animation frame, and many of those ticks cannot paint: the
//! GPU still holds the frames allowed in flight ([`crate::frames_in_flight`]).
//! Firefox reports finished frames late, so there the gate is full on most
//! resize ticks, and a resize applied on each of them showed black for
//! several frames in a row. So the frame loop in the crate's `web::frame`
//! module changes the backing store only on a tick that can paint; on a tick
//! the GPU is busy it keeps the old size and scale (the browser stretches the
//! last presented frame to the new CSS box) and the whole tick, the geometry
//! the host is told and the paint size included, stays at that size. The
//! frame loop marks itself dirty, so it keeps ticking until the GPU frees up
//! and the first tick that can paint applies the size and paints it.
//!
//! Without a GPU that can paint (init pending, a rebuild in flight, a lost
//! device, the fatal state) a cleared canvas costs nothing, as nothing would
//! have been drawn anyway: the size applies at once, so layout follows the
//! window.
//!
//! Platform-neutral so `cargo test` reaches the production decision.

/// Whether the tick sizing the canvas can paint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PaintReadiness {
    /// No usable GPU: init pending, a rebuild in flight, a lost device, or
    /// failed for good.
    NoGpu,
    /// The GPU is ready and the frames-in-flight gate can take a frame.
    CanPaint,
    /// The GPU is ready but still holds the frames allowed in flight.
    GpuBusy,
}

/// A backing-store size and the physical px per CSS px it was built at.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Backing {
    pub size: (u32, u32),
    pub scale: f64,
}

/// What a tick does with the backing store.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BackingPlan {
    /// The size and scale the tick runs at: given to the canvas, the host's
    /// geometry, the device scale, input mapping and the paint.
    pub backing: Backing,
    /// Write `backing.size` to the canvas this tick (it differs from the
    /// canvas's current size).
    pub resize: bool,
    /// A new size waits for a tick that can paint; the loop must keep ticking.
    pub deferred: bool,
}

/// The tick's backing-store decision: `current` is what the canvas holds now
/// (and the scale it was built at), `wanted` what its CSS size and DPR ask for.
pub(crate) fn plan_backing(
    current: Backing,
    wanted: Backing,
    readiness: PaintReadiness,
) -> BackingPlan {
    let changed = current.size != wanted.size;
    if changed && readiness == PaintReadiness::GpuBusy {
        return BackingPlan {
            backing: current,
            resize: false,
            deferred: true,
        };
    }
    BackingPlan {
        backing: wanted,
        resize: changed,
        deferred: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OLD: Backing = Backing {
        size: (800, 600),
        scale: 1.0,
    };
    const NEW: Backing = Backing {
        size: (1000, 700),
        scale: 1.0,
    };

    #[test]
    fn busy_gpu_keeps_the_presented_size_instead_of_clearing_the_canvas() {
        let plan = plan_backing(OLD, NEW, PaintReadiness::GpuBusy);
        assert_eq!(plan.backing, OLD, "the tick runs at the size on the canvas");
        assert!(!plan.resize, "a resize now would show black until a paint");
        assert!(plan.deferred, "the new size must stay pending");
    }

    #[test]
    fn busy_gpu_keeps_the_old_scale_with_a_dpr_change() {
        let wanted = Backing {
            size: (1600, 1200),
            scale: 2.0,
        };
        let plan = plan_backing(OLD, wanted, PaintReadiness::GpuBusy);
        assert_eq!(plan.backing, OLD);
        assert!(plan.deferred);
    }

    #[test]
    fn deferred_size_applies_on_the_first_tick_that_can_paint() {
        let mut canvas = OLD;
        for _ in 0..3 {
            let plan = plan_backing(canvas, NEW, PaintReadiness::GpuBusy);
            assert!(plan.deferred && !plan.resize);
            canvas = plan.backing;
        }
        let plan = plan_backing(canvas, NEW, PaintReadiness::CanPaint);
        assert_eq!(plan.backing, NEW);
        assert!(plan.resize);
        assert!(!plan.deferred);
    }

    #[test]
    fn no_gpu_resizes_at_once() {
        let plan = plan_backing(OLD, NEW, PaintReadiness::NoGpu);
        assert_eq!(plan.backing, NEW);
        assert!(plan.resize);
        assert!(!plan.deferred);
    }

    #[test]
    fn unchanged_size_takes_the_new_scale_without_resizing_or_deferring() {
        // A backing store the texture limit capped can keep its size while
        // the DPR moves; nothing is cleared, so the scale follows at once.
        let wanted = Backing {
            size: OLD.size,
            scale: 1.5,
        };
        for readiness in [
            PaintReadiness::NoGpu,
            PaintReadiness::CanPaint,
            PaintReadiness::GpuBusy,
        ] {
            let plan = plan_backing(OLD, wanted, readiness);
            assert_eq!(plan.backing, wanted);
            assert!(!plan.resize);
            assert!(!plan.deferred);
        }
    }
}
