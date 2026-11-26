//! The OS reader behind [`agg_gui::lcd_display_detection`]: what the desktop
//! says about font smoothing and the display, for deciding whether LCD
//! subpixel text suits this machine.
//!
//! Port of agg-sharp's `PlatformWin32/win32/WindowsLcdDisplayEnvironmentProvider.cs`.
//! Windows is the only platform that reports these facts; everywhere else the
//! provider answers "cannot say" (`None`), as C#'s does off Windows, and the
//! decision falls back to grayscale. That includes macOS, which has drawn its
//! own text without subpixel antialiasing since 10.14 and has no API for a
//! panel's stripe order, so there is nothing to read that could say yes.
//!
//! Read once, at startup, by whoever seeds the app's LCD setting - there is
//! deliberately no `WM_SETTINGCHANGE` listener, so a user who changes ClearType
//! while the app is running sees the change next launch (or immediately, by
//! flipping the app's own toggle).

use agg_gui::lcd_display_detection::{LcdDisplayEnvironment, LcdDisplayEnvironmentProvider};

/// Reads the Windows desktop's font smoothing configuration and the primary
/// display's geometry. Off Windows it cannot say, and returns `None`.
#[derive(Clone, Copy, Debug, Default)]
pub struct WindowsLcdDisplayEnvironmentProvider;

impl LcdDisplayEnvironmentProvider for WindowsLcdDisplayEnvironmentProvider {
    fn try_get_environment(&self) -> Option<LcdDisplayEnvironment> {
        read_environment()
    }
}

#[cfg(windows)]
fn read_environment() -> Option<LcdDisplayEnvironment> {
    use agg_gui::lcd_display_detection::{LcdFontSmoothingStyle, LcdStripeOrder};
    use agg_os::windows::desktop_settings::{
        font_smoothing_enabled, font_smoothing_orientation, font_smoothing_type, is_remote_session,
        primary_display_rotated_quarter_turn,
    };

    // Smoothing on/off and its style are the two the answer really hinges on;
    // if either read fails we know nothing useful and say so rather than
    // filling in a plausible value.
    let smoothing_enabled = font_smoothing_enabled()?;
    let smoothing_type = font_smoothing_type()?;

    // Orientation is missing on some drivers. RGB is the overwhelmingly common
    // panel layout and is also what Windows itself assumes, so an unreadable
    // orientation is treated as RGB rather than as a reason to give up the
    // whole detection (`1` is FE_FONTSMOOTHINGORIENTATIONRGB).
    let orientation = font_smoothing_orientation().unwrap_or(1);

    let remote_session = is_remote_session();

    // Whether the primary display is turned on its side, which puts its colour
    // stripes on the vertical axis. False when the mode cannot be read - the
    // unrotated case is the overwhelming majority.
    let rotated = primary_display_rotated_quarter_turn().unwrap_or(false);

    Some(LcdDisplayEnvironment {
        font_smoothing_enabled: smoothing_enabled,
        font_smoothing_style: LcdFontSmoothingStyle::from_windows(smoothing_type),
        stripe_order: LcdStripeOrder::from_windows(orientation),
        is_remote_session: remote_session,
        display_rotated_quarter_turn: rotated,
    })
}

/// C#'s provider returns false when `!OperatingSystem.IsWindows()`: nothing on
/// this platform reports the font smoothing facts the decision needs.
#[cfg(not(windows))]
fn read_environment() -> Option<LcdDisplayEnvironment> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Off Windows the reader must say it cannot read the display, never guess;
    /// on Windows it must not panic, whatever the desktop's settings are. Never
    /// changes a system setting.
    #[test]
    fn reader_answers_without_guessing() {
        let read = WindowsLcdDisplayEnvironmentProvider.try_get_environment();
        if cfg!(not(windows)) {
            assert_eq!(read, None);
        }
    }
}
