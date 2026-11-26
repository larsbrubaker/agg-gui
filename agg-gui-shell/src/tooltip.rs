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

    // A failed read or a zero hover time keeps agg-gui's defaults.
    let hover_ms = agg_os::windows::desktop_settings::mouse_hover_time_ms()?;
    if hover_ms == 0 {
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
