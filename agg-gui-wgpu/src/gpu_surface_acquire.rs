//! The surface-acquire recovery policy of [`super::Gpu`]: what a frame does
//! with each `Surface::get_current_texture` result — agg-sharp
//! `WebGpuSurfaceTarget.ActionFor`, pinned by the `WebGpuSurfaceAcquireTests`
//! status cases (`Tests/Agg.Tests/Agg.WebGpuRender/WebGpuSurfaceAcquireTests.cs`).
//!
//! Pure, so it is testable without a live surface; the sibling
//! `gpu_acquire.rs` acts on the answer (reconfigure, skip, ask for a redraw).

/// How to handle the result of `Surface::get_current_texture`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum SurfaceAcquire {
    /// Texture is usable — render into it.
    Present,
    /// The swapchain is stale or gone (`Outdated`/`Lost`): reconfigure the
    /// surface and try once more THIS frame.
    Reconfigure,
    /// Transient (`Timeout`): skip the frame, but ask for another one.
    SkipAndRetry,
    /// Skip the frame with no follow-up (`Occluded`/`Validation`): the window
    /// is not visible / the app must fix the validation error, and a
    /// self-requested redraw would just burn the CPU.
    Skip,
}

/// Decide how to handle a surface-acquire status. A pure function so the
/// recovery policy is unit-testable without a live GPU surface (the
/// no-payload variants are constructible in tests).
///
/// `Outdated`/`Lost` fire right after a window resize reconfigures the
/// swapchain, and after a GPU driver reset (TDR), a display-mode change, or an
/// RDP reconnect. Treating them as a plain skip leaves a reactive shell
/// (`ControlFlow::Wait` whenever `wants_draw()` is false) frozen or black until
/// some unrelated event requests another redraw — the resize-black-screen
/// regression. wgpu documents both as "reconfigure the surface and try again".
pub fn surface_acquire_action(status: &wgpu::CurrentSurfaceTexture) -> SurfaceAcquire {
    action_for(AcquireStatus::of(status))
}

/// A `wgpu::CurrentSurfaceTexture` without its texture: the two success
/// variants carry a `SurfaceTexture` only a live surface can make, so the
/// policy is decided on this and every status can be built in a test.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum AcquireStatus {
    Success,
    Suboptimal,
    Timeout,
    Occluded,
    Outdated,
    Lost,
    Validation,
}

impl AcquireStatus {
    fn of(status: &wgpu::CurrentSurfaceTexture) -> Self {
        use wgpu::CurrentSurfaceTexture as T;
        match status {
            T::Success(_) => Self::Success,
            T::Suboptimal(_) => Self::Suboptimal,
            T::Timeout => Self::Timeout,
            T::Occluded => Self::Occluded,
            T::Outdated => Self::Outdated,
            T::Lost => Self::Lost,
            T::Validation => Self::Validation,
        }
    }
}

/// The recovery policy of [`surface_acquire_action`] (agg-sharp
/// `WebGpuSurfaceTarget.ActionFor`).
fn action_for(status: AcquireStatus) -> SurfaceAcquire {
    use AcquireStatus as S;
    match status {
        S::Success | S::Suboptimal => SurfaceAcquire::Present,
        S::Outdated | S::Lost => SurfaceAcquire::Reconfigure,
        S::Timeout => SurfaceAcquire::SkipAndRetry,
        S::Occluded | S::Validation => SurfaceAcquire::Skip,
    }
}

#[cfg(test)]
mod tests {
    use super::{action_for, surface_acquire_action, AcquireStatus, SurfaceAcquire};
    use wgpu::CurrentSurfaceTexture as T;

    #[test]
    fn stale_swapchain_reconfigures_instead_of_skipping() {
        // The resize-black-screen regression, and the frozen-window case after
        // a driver reset (TDR) / display-mode change / RDP reconnect: both must
        // drive a reconfigure-and-retry, NOT a silent skip.
        assert_eq!(
            surface_acquire_action(&T::Outdated),
            SurfaceAcquire::Reconfigure
        );
        assert_eq!(
            surface_acquire_action(&T::Lost),
            SurfaceAcquire::Reconfigure
        );
    }

    #[test]
    fn timeout_skips_the_frame_but_asks_for_another() {
        // Timeout is transient: the next acquire usually succeeds, so a
        // reactive loop has to be woken back up or it waits forever.
        assert_eq!(
            surface_acquire_action(&T::Timeout),
            SurfaceAcquire::SkipAndRetry
        );
    }

    #[test]
    fn occluded_and_validation_skip_without_self_requested_redraw() {
        assert_eq!(surface_acquire_action(&T::Occluded), SurfaceAcquire::Skip);
        assert_eq!(surface_acquire_action(&T::Validation), SurfaceAcquire::Skip);
    }

    /// agg-sharp `WebGpuSurfaceAcquireTests.ASuccessfulAcquirePresents`.
    #[test]
    fn a_successful_acquire_presents() {
        assert_eq!(action_for(AcquireStatus::Success), SurfaceAcquire::Present);
        assert_eq!(
            action_for(AcquireStatus::Suboptimal),
            SurfaceAcquire::Present
        );
    }
}
