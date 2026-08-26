//! Deterministic frame capture for [`crate::native_shell::run`].
//!
//! Split out of `native_shell.rs` (800-line guardrail). Owns the three
//! pieces the shell needs for `NativeShellConfig::with_screenshot`: the
//! "is this the frame to capture?" predicate, the give-up budget for a
//! capture whose surface never becomes presentable, and the PNG write.
//!
//! The budget is wall-clock rather than a redraw count on purpose: a
//! pending capture puts the shell on `ControlFlow::Poll` and requests a
//! redraw every idle iteration, so a *healthy* capture burns dozens of
//! `RedrawRequested` events in the milliseconds before the window server
//! first hands out a surface. Any attempt-count budget small enough to
//! catch a hang also kills every real capture — see
//! [`capture_exhausted`].

use std::path::Path;
use std::time::Duration;

/// Should the frame that just finished painting be captured?
///
/// `painted` is the count of frames painted so far *including* the one
/// just rendered (1-based); `settle_frames` is the caller's requested
/// settle count, clamped to at least 1 so `0` still captures a frame.
/// Only ever true once, so the capture cannot fire twice.
pub(crate) fn should_capture(painted: u32, settle_frames: u32) -> bool {
    painted == settle_frames.max(1)
}

/// How long a pending capture may go without a painted frame before the
/// shell gives up. Generous: the first surface configuration can take a
/// while on a cold start, and a window that is merely being dragged or
/// resized still paints inside this.
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(15);

/// Has a still-pending capture waited too long for a presentable surface?
///
/// `since_last_paint` is the time since the capture was armed, or since
/// the last frame that actually painted — whichever is later.
///
/// The budget has to be wall-clock, *not* a count of redraw attempts:
/// while a capture is pending the shell runs `ControlFlow::Poll` and
/// requests a redraw unconditionally, so dozens of `RedrawRequested`
/// events go by in a couple of milliseconds — long before the window
/// server has configured the surface for the first time. An
/// attempt-based budget therefore fires on every healthy capture.
///
/// Without any budget, a surface that never becomes presentable
/// (minimized, occluded, `Lost`) busy-spins forever: only a painted
/// frame advances the settle count, and `paint_frame` returns false
/// whenever `get_current_texture` fails.
pub(crate) fn capture_exhausted(since_last_paint: Duration) -> bool {
    since_last_paint >= CAPTURE_TIMEOUT
}

/// Encode `rgba` as a PNG and write it to `path`, creating the parent
/// directory. Returns a human-readable message on failure.
pub(crate) fn write_png(path: &Path, rgba: &[u8], width: u32, height: u32) -> Result<(), String> {
    if rgba.is_empty() || width == 0 || height == 0 {
        return Err("frame read-back returned no pixels".to_string());
    }
    let png = agg_gui::screenshot::encode_png_rgba(rgba, width, height)
        .map_err(|e| format!("PNG encode failed: {e}"))?;
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("could not create {}: {e}", parent.display()))?;
        }
    }
    std::fs::write(path, &png).map_err(|e| format!("could not write {}: {e}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_exactly_on_the_settle_frame() {
        assert!(!should_capture(1, 3));
        assert!(!should_capture(2, 3));
        assert!(should_capture(3, 3));
        assert!(!should_capture(4, 3));
    }

    #[test]
    fn settle_frames_zero_captures_the_first_frame() {
        assert!(should_capture(1, 0));
        assert!(!should_capture(2, 0));
    }

    #[test]
    fn a_pending_capture_gives_up_after_the_wall_clock_timeout() {
        assert!(!capture_exhausted(Duration::ZERO));
        assert!(!capture_exhausted(
            CAPTURE_TIMEOUT - Duration::from_millis(1)
        ));
        assert!(capture_exhausted(CAPTURE_TIMEOUT));
        assert!(capture_exhausted(CAPTURE_TIMEOUT + Duration::from_secs(60)));
    }

    /// The regression that made every real capture fail: `ControlFlow::Poll`
    /// burns dozens of redraw attempts in a few milliseconds while the
    /// window server is still configuring the surface, so no plausible
    /// attempt count is safe — only elapsed time is.
    #[test]
    fn a_burst_of_attempts_in_a_few_milliseconds_is_not_exhaustion() {
        for ms in [0_u64, 1, 5, 50, 500] {
            assert!(
                !capture_exhausted(Duration::from_millis(ms)),
                "{ms} ms of polling must not end a pending capture"
            );
        }
    }

    #[test]
    fn empty_readback_is_an_error() {
        let dir = std::env::temp_dir().join("agg-gui-native-shell-screenshot-test");
        let path = dir.join("empty.png");
        let err = write_png(&path, &[], 0, 0).unwrap_err();
        assert!(err.contains("no pixels"), "unexpected message: {err}");
        assert!(!path.exists());
    }

    #[test]
    fn writes_a_png_and_creates_the_parent_directory() {
        let dir = std::env::temp_dir()
            .join("agg-gui-native-shell-screenshot-test")
            .join("nested");
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("shot.png");
        write_png(&path, &[255, 0, 0, 255, 0, 255, 0, 255], 2, 1).expect("write png");
        let bytes = std::fs::read(&path).expect("read back png");
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
