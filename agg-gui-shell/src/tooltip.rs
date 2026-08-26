//! OS tooltip hover timing.
//!
//! agg-gui ships defaults for the tooltip initial/reshow/autopop delays, but a
//! desktop app should feel like the rest of the desktop: Windows exposes the
//! user's hover time through `SPI_GETMOUSEHOVERTIME`, and the other delays are
//! derived from it per the platform's tooltip conventions. Every other
//! platform returns `None`, leaving the library defaults in place.

/// Query the OS for tooltip hover timing, or `None` to keep agg-gui's defaults.
#[cfg(windows)]
pub(crate) fn os_tooltip_timings() -> Option<agg_gui::TooltipTimings> {
    use std::time::Duration;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SystemParametersInfoW, SPI_GETMOUSEHOVERTIME,
    };

    let mut hover_ms: u32 = 0;
    // Safety: SPI_GETMOUSEHOVERTIME writes a single `u32` (the hover time in
    // milliseconds) into `pvParam`; we pass a pointer to a live `u32` and read
    // it only after a non-zero (success) return.
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_GETMOUSEHOVERTIME,
            0,
            (&mut hover_ms as *mut u32).cast(),
            0,
        )
    };
    if ok == 0 || hover_ms == 0 {
        return None;
    }
    Some(agg_gui::TooltipTimings::from_initial_delay(
        Duration::from_millis(hover_ms as u64),
    ))
}

#[cfg(not(windows))]
pub(crate) fn os_tooltip_timings() -> Option<agg_gui::TooltipTimings> {
    None
}
