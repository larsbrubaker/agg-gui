//! Deterministic frame capture for [`crate::native_shell::run`].
//!
//! Split out of `native_shell.rs` (800-line guardrail). Owns the two
//! pieces the shell needs for `NativeShellConfig::with_screenshot`: the
//! "is this the frame to capture?" predicate and the PNG write.

use std::path::Path;

/// Should the frame that just finished painting be captured?
///
/// `painted` is the count of frames painted so far *including* the one
/// just rendered (1-based); `settle_frames` is the caller's requested
/// settle count, clamped to at least 1 so `0` still captures a frame.
/// Only ever true once, so the capture cannot fire twice.
pub(crate) fn should_capture(painted: u32, settle_frames: u32) -> bool {
    painted == settle_frames.max(1)
}

/// Encode `rgba` as a PNG and write it to `path`, creating the parent
/// directory. Returns a human-readable message on failure.
pub(crate) fn write_png(
    path: &Path,
    rgba: &[u8],
    width: u32,
    height: u32,
) -> Result<(), String> {
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
