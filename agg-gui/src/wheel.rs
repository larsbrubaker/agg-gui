//! Wheel-delta normalization: OS / DOM scroll deltas → agg-gui **notches**.
//!
//! agg-gui's [`Event::MouseWheel`](crate::event::Event::MouseWheel)
//! carries a *notch* count, not pixels: every consumer scales it itself
//! ([`ScrollView`](crate::widgets::scroll_view) multiplies by
//! [`PIXELS_PER_NOTCH`], text areas and tree views do the same, and zoom
//! consumers read only the sign). Shells are therefore responsible for
//! converting whatever their platform reports into notches, and this
//! module is that conversion in one testable place — the native shells
//! (`LineDelta` through, `PixelDelta / 40`) and the web shell
//! (`WheelEvent.deltaMode`) had drifted apart, and a shim that forwarded
//! raw pixels multiplied every scroll in the app by ~40-100×.
//!
//! # Precision deltas are accumulated, not forwarded fractionally
//!
//! A macOS trackpad (and a smooth-scrolling mouse) reports pixel deltas of
//! 1-10 px per event, i.e. a *fraction* of a notch. Forwarding fractions
//! is fine for consumers that scale by the magnitude but wrong for the
//! ones that only read the sign — a zoom would take a full step per
//! 2-pixel event. [`WheelNormalizer`] therefore accumulates sub-notch
//! travel and emits only whole notches, keeping the remainder for the
//! next event: nothing is dropped (40 px of finger travel always produces
//! exactly one notch, which a `ScrollView` turns back into 40 px, so
//! precision scrolling stays 1:1), and no consumer ever sees a fraction.
//!
//! The accumulator is per-axis and resets when the direction flips, so a
//! reversal takes effect immediately instead of first burning the
//! leftover of the previous direction.

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

/// Per-axis sub-notch accumulator (see the module docs).
///
/// One instance per input source; a shell keeps it beside its event
/// listener. Sign convention is the caller's — the normalizer only
/// changes units, so a shell that flips the browser's
/// "positive = scroll down" into agg-gui's "positive = wheel forward"
/// keeps doing that at its own call site.
#[derive(Debug, Clone, Copy, Default)]
pub struct WheelNormalizer {
    acc_x: f64,
    acc_y: f64,
}

impl WheelNormalizer {
    pub fn new() -> WheelNormalizer {
        WheelNormalizer::default()
    }

    /// Convert one event's deltas to whole notches, carrying whatever
    /// did not add up to one into the next call.
    pub fn normalize(&mut self, delta_x: f64, delta_y: f64, mode: WheelDeltaMode) -> (f64, f64) {
        let x = accumulate(&mut self.acc_x, to_notches(delta_x, mode));
        let y = accumulate(&mut self.acc_y, to_notches(delta_y, mode));
        (x, y)
    }

    /// Forget any partial travel — for a shell that lost the pointer
    /// (canvas blur, gesture cancel) and must not apply half a notch
    /// from before to whatever comes next.
    pub fn reset(&mut self) {
        self.acc_x = 0.0;
        self.acc_y = 0.0;
    }
}

/// Slack allowed when deciding whether the banked travel has reached a
/// whole notch. Ten 4-px events *are* 40 px, but ten binary `0.1`s fall a
/// few ulps short of `1.0`; without this the tenth event of a steady
/// trackpad scroll would silently bank instead of firing, and the notch
/// would arrive one event late forever after.
const NOTCH_EPSILON: f64 = 1e-9;

/// Add `notches` to `acc` and take out the whole part, resetting first
/// when the direction reverses.
fn accumulate(acc: &mut f64, notches: f64) -> f64 {
    if notches == 0.0 {
        return 0.0;
    }
    if *acc != 0.0 && acc.signum() != notches.signum() {
        *acc = 0.0;
    }
    *acc += notches;
    let whole = (acc.abs() + NOTCH_EPSILON).trunc().copysign(*acc);
    *acc -= whole;
    whole
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
        assert_eq!(n.normalize(0.0, 100.0, WheelDeltaMode::Pixel).1, 2.0);
        // Firefox: 3 lines per notch.
        assert_eq!(n.normalize(0.0, 3.0, WheelDeltaMode::Line).1, 3.0);
        // A page device.
        assert_eq!(
            n.normalize(0.0, 1.0, WheelDeltaMode::Page).1,
            NOTCHES_PER_PAGE
        );
    }

    /// The precision case: 4 px events emit nothing until they add up,
    /// and then exactly one notch — with the leftover carried, so no
    /// travel is lost and 40 px of finger always means one notch.
    #[test]
    fn sub_notch_deltas_accumulate_instead_of_jumping_or_vanishing() {
        let mut n = WheelNormalizer::new();
        for _ in 0..9 {
            assert_eq!(
                n.normalize(0.0, 4.0, WheelDeltaMode::Pixel).1,
                0.0,
                "a fraction of a notch must not reach a sign-reading consumer"
            );
        }
        assert_eq!(n.normalize(0.0, 4.0, WheelDeltaMode::Pixel).1, 1.0);

        // 400 px of travel is ten notches, however it is chopped up.
        let mut fine = WheelNormalizer::new();
        let total: f64 = (0..400)
            .map(|_| fine.normalize(0.0, 1.0, WheelDeltaMode::Pixel).1)
            .sum();
        assert_eq!(total, 10.0);
        let mut coarse = WheelNormalizer::new();
        assert_eq!(coarse.normalize(0.0, 400.0, WheelDeltaMode::Pixel).1, 10.0);
    }

    /// Reversing direction answers at once rather than first spending the
    /// notch fragment banked in the other direction.
    #[test]
    fn a_direction_change_drops_the_partial_notch() {
        let mut n = WheelNormalizer::new();
        assert_eq!(n.normalize(0.0, 30.0, WheelDeltaMode::Pixel).1, 0.0);
        assert_eq!(n.normalize(0.0, -30.0, WheelDeltaMode::Pixel).1, 0.0);
        // …and 40 more px the *new* way is a whole notch, not 10 px worth
        // of catching up.
        assert_eq!(n.normalize(0.0, -10.0, WheelDeltaMode::Pixel).1, -1.0);
    }

    /// The two axes bank separately — a diagonal trackpad swipe must not
    /// let horizontal travel pay for a vertical notch.
    #[test]
    fn the_axes_accumulate_independently() {
        let mut n = WheelNormalizer::new();
        assert_eq!(n.normalize(30.0, 20.0, WheelDeltaMode::Pixel), (0.0, 0.0));
        assert_eq!(n.normalize(10.0, 0.0, WheelDeltaMode::Pixel), (1.0, 0.0));
        assert_eq!(n.normalize(0.0, 20.0, WheelDeltaMode::Pixel), (0.0, 1.0));
    }

    /// Nonsense in, nothing out: a `NaN` delta (seen from at least one
    /// browser extension) must not poison the accumulator.
    #[test]
    fn non_finite_deltas_are_ignored() {
        let mut n = WheelNormalizer::new();
        assert_eq!(
            n.normalize(f64::NAN, f64::INFINITY, WheelDeltaMode::Pixel),
            (0.0, 0.0)
        );
        assert_eq!(n.normalize(0.0, 40.0, WheelDeltaMode::Pixel).1, 1.0);
    }

    /// `reset` throws the partial travel away.
    #[test]
    fn reset_forgets_partial_travel() {
        let mut n = WheelNormalizer::new();
        n.normalize(0.0, 30.0, WheelDeltaMode::Pixel);
        n.reset();
        assert_eq!(n.normalize(0.0, 30.0, WheelDeltaMode::Pixel).1, 0.0);
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
