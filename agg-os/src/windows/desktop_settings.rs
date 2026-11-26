//! Reads of the Windows desktop's settings (feature `desktop-settings`), one
//! system call each with its pointer handling inside.
//!
//! agg-gui-shell is the consumer: `tooltip.rs` (hover time), `click_policy.rs`
//! (double-click time and rectangle), `input.rs` (the live cursor position
//! during a file drop) and `lcd_display.rs` (font smoothing, remote session,
//! display rotation). Each function only reports what Windows says; the
//! defaults, clamping and fallbacks stay with those callers.

use windows_sys_052::Win32::Foundation::POINT;
use windows_sys_052::Win32::Graphics::Gdi::{
    EnumDisplaySettingsW, DEVMODEW, DMDO_270, DMDO_90, ENUM_CURRENT_SETTINGS,
};
use windows_sys_052::Win32::UI::Input::KeyboardAndMouse::GetDoubleClickTime;
use windows_sys_052::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetSystemMetrics, SystemParametersInfoW, SM_CXDOUBLECLK, SM_CYDOUBLECLK,
    SM_REMOTESESSION, SPI_GETFONTSMOOTHING, SPI_GETFONTSMOOTHINGORIENTATION,
    SPI_GETFONTSMOOTHINGTYPE, SPI_GETMOUSEHOVERTIME, SYSTEM_PARAMETERS_INFO_ACTION,
};

/// The user's mouse hover time in milliseconds (`SPI_GETMOUSEHOVERTIME`), or
/// `None` when the call fails. A zero is passed on as read.
pub fn mouse_hover_time_ms() -> Option<u32> {
    system_parameter(SPI_GETMOUSEHOVERTIME)
}

/// The double-click time in milliseconds (`GetDoubleClickTime`).
pub fn double_click_time_ms() -> u32 {
    // SAFETY: GetDoubleClickTime takes no arguments and only reads a system
    // setting.
    unsafe { GetDoubleClickTime() }
}

/// The width and height, in physical pixels, of the rectangle around a first
/// click that the second must land in (`SM_CXDOUBLECLK`, `SM_CYDOUBLECLK`).
/// Zero means unavailable.
pub fn double_click_rect_px() -> (i32, i32) {
    // SAFETY: GetSystemMetrics takes an index by value and has no pointer
    // arguments; an unknown index returns 0.
    unsafe {
        (
            GetSystemMetrics(SM_CXDOUBLECLK),
            GetSystemMetrics(SM_CYDOUBLECLK),
        )
    }
}

/// The cursor position in screen coordinates (physical pixels)
/// (`GetCursorPos`), or `None` when the call fails.
pub fn cursor_pos() -> Option<(i32, i32)> {
    let mut pt = POINT { x: 0, y: 0 };
    // SAFETY: GetCursorPos writes into the POINT we own; no other
    // preconditions.
    if unsafe { GetCursorPos(&mut pt) } == 0 {
        return None;
    }
    Some((pt.x, pt.y))
}

/// Whether font smoothing is on (`SPI_GETFONTSMOOTHING`), or `None` when the
/// call fails.
pub fn font_smoothing_enabled() -> Option<bool> {
    system_parameter(SPI_GETFONTSMOOTHING).map(|value| value != 0)
}

/// The font smoothing type (`SPI_GETFONTSMOOTHINGTYPE`: 1 standard, 2
/// ClearType), or `None` when the call fails.
pub fn font_smoothing_type() -> Option<u32> {
    system_parameter(SPI_GETFONTSMOOTHINGTYPE)
}

/// The font smoothing orientation (`SPI_GETFONTSMOOTHINGORIENTATION`: 0 BGR,
/// 1 RGB), or `None` when the call fails (some drivers do not report it).
pub fn font_smoothing_orientation() -> Option<u32> {
    system_parameter(SPI_GETFONTSMOOTHINGORIENTATION)
}

/// Whether this process runs in a remote desktop session
/// (`GetSystemMetrics(SM_REMOTESESSION)` is non-zero).
pub fn is_remote_session() -> bool {
    // SAFETY: GetSystemMetrics takes an index by value and has no pointer
    // arguments; an unknown index returns 0.
    let remote = unsafe { GetSystemMetrics(SM_REMOTESESSION) };
    remote != 0
}

/// Whether the display the calling thread is on (at startup, the primary
/// display) is turned a quarter turn (`DMDO_90` or `DMDO_270`), read through
/// `EnumDisplaySettingsW`; `None` when its mode cannot be read.
pub fn primary_display_rotated_quarter_turn() -> Option<bool> {
    // SAFETY: DEVMODEW is plain old data (integers, u16 arrays and a union of
    // integer structs), so the all-zero bit pattern is a valid value.
    let mut device_mode: DEVMODEW = unsafe { std::mem::zeroed() };
    // The struct's size, a couple of hundred bytes: the cast cannot overflow.
    device_mode.dmSize = std::mem::size_of::<DEVMODEW>() as u16;

    // A null device name asks for the display the calling thread is on, which
    // at startup is the primary display.
    // SAFETY: `device_mode` is a live DEVMODEW whose dmSize says how much the
    // call may write; a null device name is documented as "the current display".
    let ok =
        unsafe { EnumDisplaySettingsW(std::ptr::null(), ENUM_CURRENT_SETTINGS, &mut device_mode) };
    if ok == 0 {
        return None;
    }

    // SAFETY: for a display device the union holds the display half
    // (position, orientation, fixed output), which EnumDisplaySettingsW filled;
    // every field of either half is an integer, so any bit pattern is valid.
    let orientation = unsafe { device_mode.Anonymous1.Anonymous2.dmDisplayOrientation };
    Some(orientation == DMDO_90 || orientation == DMDO_270)
}

/// One `SystemParametersInfoW` query whose answer is a single `UINT`/`BOOL`,
/// or `None` when the call fails.
///
/// Private on purpose: it passes a 4-byte buffer, which is sound only for the
/// actions that write one 32-bit value. Every caller above passes such an
/// action; another action (one that writes a struct) would write past it.
fn system_parameter(action: SYSTEM_PARAMETERS_INFO_ACTION) -> Option<u32> {
    let mut value: u32 = 0;
    // SAFETY: each action passed here writes one 32-bit value into `pvParam`
    // (a UINT for SPI_GETMOUSEHOVERTIME, a BOOL for SPI_GETFONTSMOOTHING, a
    // UINT for the smoothing type and orientation); we pass a pointer to a
    // live `u32` and read it only after a non-zero (success) return.
    let ok = unsafe { SystemParametersInfoW(action, 0, (&mut value as *mut u32).cast(), 0) };
    (ok != 0).then_some(value)
}
