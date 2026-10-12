//! Ports agg-sharp `Tests/Agg.Tests/Agg.UI/DisplayScaleTests.cs`
//! (`AnUnusableScaleFallsBackToOne`, `AnUnusableScreenSizeLeavesTheLastGoodOne`)
//! and `UiScaleTests.cs` (`UnusableScalesFallBackToOne`,
//! `TheWindowSizeIsScaledAndClampedToTheDesktop`) over `display_metrics.rs`.
//! C#'s `SystemWindow.SetDisplayScale` and `UiScale.UpdateForDisplayScale`
//! both reduce to `usable_display_scale`, which the shell applies before
//! `agg_gui::set_device_scale`; the tests drive it the same way.

use super::*;

/// What the shell does with a host report: compose the usable scale into
/// agg-gui's device scale, exactly as `run.rs` and `shell_loop.rs` do.
fn report(display_scale: f64) {
    agg_gui::set_device_scale(usable_display_scale(display_scale));
}

#[test]
fn an_unusable_scale_falls_back_to_one() {
    // A zero or NaN scale would silently divide the whole UI to nothing. The
    // hosts read theirs from the OS, so this is defence against a monitor
    // hot-plug reporting garbage mid-transition, not against a caller mistake.
    report(2.0);
    report(f64::NAN);
    assert_eq!(agg_gui::device_scale(), 1.0);

    report(2.0);
    report(0.0);
    assert_eq!(agg_gui::device_scale(), 1.0);

    report(2.0);
    report(-1.5);
    assert_eq!(agg_gui::device_scale(), 1.0);

    report(2.0);
    report(f64::INFINITY);
    assert_eq!(agg_gui::device_scale(), 1.0);
}

#[test]
fn unusable_scales_fall_back_to_one() {
    for unusable in [0.0, -2.0, f64::NAN, f64::INFINITY] {
        report(2.0);
        report(unusable);

        assert_eq!(
            agg_gui::device_scale(),
            1.0,
            "a display scale of {unusable} is a 1x display"
        );
    }
}

#[test]
fn an_unusable_screen_size_leaves_the_last_good_one() {
    // A window dragged off screen, or caught mid hot-plug, measures nothing.
    // Taking that as the new truth would throw away the only description of
    // the monitor the window is on and send the application back to guessing
    // from the primary display - which is the bug this exists to fix.
    let mut display = DisplayUsableSize::new();

    display.set(1440.0, 810.0);

    display.set(0.0, 0.0);
    assert_eq!(display.get(), Some((1440.0, 810.0)));

    display.set(-100.0, 810.0);
    assert_eq!(display.get(), Some((1440.0, 810.0)));

    display.set(1440.0, f64::NAN);
    assert_eq!(display.get(), Some((1440.0, 810.0)));

    display.set(f64::INFINITY, 810.0);
    assert_eq!(display.get(), Some((1440.0, 810.0)));
}

#[test]
fn the_window_size_is_scaled_and_clamped_to_the_desktop() {
    assert_eq!(
        scaled_window_size(1200.0, 800.0, 2.0, (3000, 2000)),
        (2400, 1600)
    );
    assert_eq!(
        scaled_window_size(1200.0, 800.0, 1.5, (1920, 1040)),
        (1800, 1040)
    );
    assert_eq!(
        scaled_window_size(1200.0, 800.0, 2.0, (0, 0)),
        (2400, 1600),
        "a desktop the host could not measure is not clamped to"
    );
}

#[test]
fn rust_only_a_scaled_size_rounds_half_to_even_as_csharp_does() {
    // C#'s Math.Round is banker's rounding: 100.5 -> 100, 101.5 -> 102.
    assert_eq!(scaled_window_size(201.0, 203.0, 0.5, (0, 0)), (100, 102));
}
