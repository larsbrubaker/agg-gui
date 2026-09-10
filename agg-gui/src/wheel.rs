//! Wheel-delta normalization: OS / DOM scroll deltas → agg-gui **notches**.
//!
//! agg-gui's [`Event::MouseWheel`](crate::event::Event::MouseWheel)
//! carries a *notch* count, not pixels: every consumer scales it itself
//! ([`ScrollView`](crate::widgets::scroll_view) multiplies by
//! [`PIXELS_PER_NOTCH`], text areas and tree views do the same, and zoom
//! consumers raise their per-notch factor to the power of the delta).
//! Shells are therefore responsible for converting whatever their
//! platform reports into notches, and this module is that conversion in
//! one testable place — the native shells (`LineDelta` through,
//! `PixelDelta / 40`) and the web shell (`WheelEvent.deltaMode`) had
//! drifted apart, and a shim that forwarded raw pixels multiplied every
//! scroll in the app by ~40-100×.
//!
//! # Precision deltas are forwarded fractionally
//!
//! A macOS trackpad (and a smooth-scrolling mouse) reports pixel deltas of
//! 1-10 px per event, i.e. a *fraction* of a notch. Those fractions are
//! passed straight through: 4 px of finger travel is 0.1 notch, which a
//! `ScrollView` turns back into 4 px, so precision scrolling stays 1:1
//! **and** continuous — no banking into 40 px jumps. A classic wheel still
//! arrives as whole notches (one `LineDelta` line, or ~100 px per click
//! in browsers that report wheels in pixels), so consumers that want a
//! crisp per-click step get one; consumers must therefore scale by the
//! delta's magnitude rather than reading only its sign, or a trackpad
//! would take a full step per 2-pixel event. Every consumer in agg-gui
//! does (scroll containers multiply, zooms use `step.powf(delta)`).
//!
//! An earlier revision banked sub-notch travel and emitted only whole
//! notches. That made trackpads stutter in 40 px steps on the web while
//! the native shell — which never banked — was smooth; the two shells now
//! agree, via [`to_notches`] on both.

/// Pixels one wheel notch is worth. The scale
/// [`ScrollView`](crate::widgets::scroll_view) scrolls by, and the
/// divisor the native shells apply to a trackpad's pixel delta.
pub const PIXELS_PER_NOTCH: f64 = 40.0;

/// Notches one "page" (DOM `deltaMode` 2) is worth.
///
/// Browsers only report page deltas for Page Up / Page Down style wheel
/// devices, and the CSS spec leaves the size to the user agent. Eight
/// notches — 320 px through a `ScrollView` — is deliberately conservative:
/// enough to read as a page jump, not so much that a stray event throws
/// the user out of the document.
pub const NOTCHES_PER_PAGE: f64 = 8.0;

/// How a platform expressed its wheel delta. The discriminants match the
/// DOM's `WheelEvent.deltaMode`, so a web shell can convert with
/// [`WheelDeltaMode::from_dom`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WheelDeltaMode {
    /// Pixels — a precision device (trackpad, smooth wheel) or a browser
    /// that reports even a classic wheel this way (~100 px per notch).
    Pixel,
    /// Lines. One line is one notch: this is what a classic wheel step
    /// means, and what winit's `LineDelta` already carries.
    Line,
    /// Pages.
    Page,
}

impl WheelDeltaMode {
    /// DOM `WheelEvent.deltaMode` → this enum. Unknown values are treated
    /// as pixels, which is what every browser in practice reports.
    pub fn from_dom(delta_mode: u32) -> WheelDeltaMode {
        match delta_mode {
            1 => WheelDeltaMode::Line,
            2 => WheelDeltaMode::Page,
            _ => WheelDeltaMode::Pixel,
        }
    }
}

/// Raw delta → notches, before any accumulation. Pure; see
/// [`WheelNormalizer`] for the part that keeps state.
pub fn to_notches(delta: f64, mode: WheelDeltaMode) -> f64 {
    if !delta.is_finite() {
        return 0.0;
    }
    match mode {
        WheelDeltaMode::Pixel => delta / PIXELS_PER_NOTCH,
        WheelDeltaMode::Line => delta,
        WheelDeltaMode::Page => delta * NOTCHES_PER_PAGE,
    }
}

/// Stateless per-event converter a shell keeps beside its wheel listener.
///
/// Sign convention is the caller's — the normalizer only changes units,
/// so a shell that flips the browser's "positive = scroll down" into
/// agg-gui's "positive = wheel forward" keeps doing that at its own call
/// site. Kept as a type (rather than two calls to [`to_notches`]) so the
/// shells have one obvious place to convert both axes.
#[derive(Debug, Clone, Copy, Default)]
pub struct WheelNormalizer;

impl WheelNormalizer {
    pub fn new() -> WheelNormalizer {
        WheelNormalizer
    }

    /// Convert one event's deltas to notches — fractional for precision
    /// devices, whole for a classic wheel. Non-finite input becomes `0.0`.
    pub fn normalize(&mut self, delta_x: f64, delta_y: f64, mode: WheelDeltaMode) -> (f64, f64) {
        (to_notches(delta_x, mode), to_notches(delta_y, mode))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A classic wheel notch arrives whole in every mode a browser can
    /// report it in.
    #[test]
    fn one_notch_in_each_mode() {
        let mut n = WheelNormalizer::new();
        // Chrome/Edge/Safari: 100 CSS px per notch.
        assert_eq!(n.normalize(0.0, 100.0, WheelDeltaMode::Pixel).1, 2.5);
        // Firefox: 3 lines per notch.
        assert_eq!(n.normalize(0.0, 3.0, WheelDeltaMode::Line).1, 3.0);
        // A page device.
        assert_eq!(
            n.normalize(0.0, 1.0, WheelDeltaMode::Page).1,
            NOTCHES_PER_PAGE
        );
    }

    /// The precision case: a 4 px trackpad event is a tenth of a notch
    /// and is delivered as exactly that, every event, so a `ScrollView`
    /// moves 4 px per event instead of 40 px every tenth event.
    #[test]
    fn sub_notch_deltas_pass_through_fractionally() {
        let mut n = WheelNormalizer::new();
        for _ in 0..10 {
            assert_eq!(n.normalize(0.0, 4.0, WheelDeltaMode::Pixel).1, 0.1);
        }
        // 400 px of travel is ten notches, however it is chopped up.
        let mut fine = WheelNormalizer::new();
        let total: f64 = (0..400)
            .map(|_| fine.normalize(0.0, 1.0, WheelDeltaMode::Pixel).1)
            .sum();
        assert!((total - 10.0).abs() < 1e-9, "{total}");
        let mut coarse = WheelNormalizer::new();
        assert_eq!(coarse.normalize(0.0, 400.0, WheelDeltaMode::Pixel).1, 10.0);
    }

    /// No state: reversing direction answers at once, and the two axes
    /// never influence each other.
    #[test]
    fn direction_changes_and_axes_are_independent() {
        let mut n = WheelNormalizer::new();
        assert_eq!(n.normalize(0.0, 30.0, WheelDeltaMode::Pixel).1, 0.75);
        assert_eq!(n.normalize(0.0, -30.0, WheelDeltaMode::Pixel).1, -0.75);
        assert_eq!(n.normalize(30.0, 20.0, WheelDeltaMode::Pixel), (0.75, 0.5));
        assert_eq!(n.normalize(10.0, 0.0, WheelDeltaMode::Pixel), (0.25, 0.0));
    }

    /// Nonsense in, nothing out: a `NaN` delta (seen from at least one
    /// browser extension) becomes zero rather than propagating.
    #[test]
    fn non_finite_deltas_are_ignored() {
        let mut n = WheelNormalizer::new();
        assert_eq!(
            n.normalize(f64::NAN, f64::INFINITY, WheelDeltaMode::Pixel),
            (0.0, 0.0)
        );
        assert_eq!(n.normalize(0.0, 40.0, WheelDeltaMode::Pixel).1, 1.0);
    }

    /// DOM deltaMode mapping, including the "anything else is pixels"
    /// fallback.
    #[test]
    fn dom_delta_modes_map_across() {
        assert_eq!(WheelDeltaMode::from_dom(0), WheelDeltaMode::Pixel);
        assert_eq!(WheelDeltaMode::from_dom(1), WheelDeltaMode::Line);
        assert_eq!(WheelDeltaMode::from_dom(2), WheelDeltaMode::Page);
        assert_eq!(WheelDeltaMode::from_dom(7), WheelDeltaMode::Pixel);
    }
}
