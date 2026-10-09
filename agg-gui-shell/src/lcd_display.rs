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
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, SM_REMOTESESSION, SPI_GETFONTSMOOTHING, SPI_GETFONTSMOOTHINGORIENTATION,
        SPI_GETFONTSMOOTHINGTYPE,
    };

    // Smoothing on/off and its style are the two the answer really hinges on;
    // if either read fails we know nothing useful and say so rather than
    // filling in a plausible value.
    let smoothing_enabled = system_parameter(SPI_GETFONTSMOOTHING)?;
    let smoothing_type = system_parameter(SPI_GETFONTSMOOTHINGTYPE)?;

    // Orientation is missing on some drivers. RGB is the overwhelmingly common
    // panel layout and is also what Windows itself assumes, so an unreadable
    // orientation is treated as RGB rather than as a reason to give up the
    // whole detection (`1` is FE_FONTSMOOTHINGORIENTATIONRGB).
    let orientation = system_parameter(SPI_GETFONTSMOOTHINGORIENTATION).unwrap_or(1);

    // Safety: GetSystemMetrics takes an index by value and has no pointer
    // arguments; an unknown index returns 0.
    let remote_session = unsafe { GetSystemMetrics(SM_REMOTESESSION) } != 0;

    Some(LcdDisplayEnvironment {
        font_smoothing_enabled: smoothing_enabled != 0,
        font_smoothing_style: LcdFontSmoothingStyle::from_windows(smoothing_type),
        stripe_order: LcdStripeOrder::from_windows(orientation),
        is_remote_session: remote_session,
        display_rotated_quarter_turn: primary_display_is_rotated_quarter_turn(),
    })
}

/// One `SystemParametersInfoW` query whose answer is a single `UINT`/`BOOL`
/// (all three font smoothing actions are), or `None` when the call fails.
#[cfg(windows)]
fn system_parameter(
    action: windows_sys::Win32::UI::WindowsAndMessaging::SYSTEM_PARAMETERS_INFO_ACTION,
) -> Option<u32> {
    use windows_sys::Win32::UI::WindowsAndMessaging::SystemParametersInfoW;

    let mut value: u32 = 0;
    // Safety: each action passed here writes one 32-bit value (a BOOL for
    // SPI_GETFONTSMOOTHING, a UINT for the type and orientation) into
    // `pvParam`; we pass a pointer to a live `u32` and read it only after a
    // non-zero (success) return.
    let ok = unsafe { SystemParametersInfoW(action, 0, (&mut value as *mut u32).cast(), 0) };
    (ok != 0).then_some(value)
}

/// Whether the primary display is turned on its side, which puts its colour
/// stripes on the vertical axis. False when the mode cannot be read - the
/// unrotated case is the overwhelming majority.
#[cfg(windows)]
fn primary_display_is_rotated_quarter_turn() -> bool {
    use windows_sys::Win32::Graphics::Gdi::{
        EnumDisplaySettingsW, DEVMODEW, DMDO_270, DMDO_90, ENUM_CURRENT_SETTINGS,
    };

    // Safety: DEVMODEW is plain old data (integers, u16 arrays and a union of
    // integer structs), so the all-zero bit pattern is a valid value.
    let mut device_mode: DEVMODEW = unsafe { std::mem::zeroed() };
    device_mode.dmSize = std::mem::size_of::<DEVMODEW>() as u16;

    // A null device name asks for the display the calling thread is on, which
    // at startup is the primary display.
    // Safety: `device_mode` is a live DEVMODEW whose dmSize says how much the
    // call may write; a null device name is documented as "the current display".
    let ok =
        unsafe { EnumDisplaySettingsW(std::ptr::null(), ENUM_CURRENT_SETTINGS, &mut device_mode) };
    if ok == 0 {
        return false;
    }

    // Safety: for a display device the union holds the display half
    // (position, orientation, fixed output), which EnumDisplaySettingsW filled.
    let orientation = unsafe { device_mode.Anonymous1.Anonymous2.dmDisplayOrientation };
    orientation == DMDO_90 || orientation == DMDO_270
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
