//! DOM → agg-gui input and sizing math: mouse-button codes, the `buttons`
//! bitmask, CSS-pixel → physical-pixel positions, canvas backing-store size,
//! and device-pixel-ratio sanitising.
//!
//! Pure functions over plain numbers so they are unit-testable natively; the
//! wasm listeners (the crate's `web::input` module) read the numbers off the
//! DOM events and call these. Crate-internal: not part of the public API.

use agg_gui::{Modifiers, MouseButton};

/// Smallest device scale the shell will hand agg-gui. A zero / negative /
/// NaN `devicePixelRatio` (seen in headless browsers and during some zoom
/// transitions) would collapse layout.
pub(crate) const MIN_DEVICE_SCALE: f64 = 0.5;

/// A usable device scale from a raw `window.devicePixelRatio`: non-finite or
/// non-positive reads as 1.0, anything else is clamped to at least
/// [`MIN_DEVICE_SCALE`].
pub(crate) fn sanitize_dpr(raw: f64) -> f64 {
    if !raw.is_finite() || raw <= 0.0 {
        1.0
    } else {
        raw.max(MIN_DEVICE_SCALE)
    }
}

/// Canvas backing-store size for a CSS client size at `dpr`: truncated
/// `client × dpr`, at least 1×1 so wgpu never sees a zero extent.
pub(crate) fn backing_size(client_w: f64, client_h: f64, dpr: f64) -> (u32, u32) {
    let px = |css: f64| {
        let v = css * dpr;
        if v.is_finite() && v > 1.0 {
            // Truncation intended: matches `Math.floor` in the JS harnesses.
            v.min(u32::MAX as f64) as u32
        } else {
            1
        }
    };
    (px(client_w), px(client_h))
}

/// Clamp a backing size to the device's `max_texture_dimension_2d` — a DPR-3
/// phone canvas can exceed a conservative WebGL2 limit, and an oversized
/// surface fails validation and leaves the canvas black.
pub(crate) fn clamp_to_max_dim(size: (u32, u32), max_dim: u32) -> (u32, u32) {
    let max_dim = max_dim.max(1);
    (size.0.clamp(1, max_dim), size.1.clamp(1, max_dim))
}

/// The canvas backing store and the scale it was built at: `client × dpr`,
/// except that when either axis would exceed the device's
/// `max_texture_dimension_2d` the **scale** is reduced uniformly so both fit.
///
/// This is the single source of truth for sizing: the returned scale is what
/// agg-gui's device scale and the pointer mapping ([`client_to_physical`])
/// must use, so layout, surface and input stay in one coordinate space. (A
/// per-axis clamp would leave the surface smaller than `client × dpr`; the
/// browser's surface `configure` then rewrites the canvas size and the next
/// tick "resizes" it back — a resize every frame.)
pub(crate) fn fit_backing(
    client_w: f64,
    client_h: f64,
    dpr: f64,
    max_dim: u32,
) -> ((u32, u32), f64) {
    let max = max_dim.max(1) as f64;
    let mut scale = dpr;
    for css in [client_w, client_h] {
        if css.is_finite() && css > 0.0 && css * scale > max {
            scale = max / css;
        }
    }
    let size = clamp_to_max_dim(backing_size(client_w, client_h, scale), max_dim);
    (size, scale)
}

/// Canvas-local pointer position in physical pixels, Y-down — the input space
/// `App` expects — from an event's `clientX/Y`, the canvas bounding rect's
/// left/top (CSS px) and the DPR.
pub(crate) fn client_to_physical(
    client_x: f64,
    client_y: f64,
    rect_left: f64,
    rect_top: f64,
    dpr: f64,
) -> (f64, f64) {
    ((client_x - rect_left) * dpr, (client_y - rect_top) * dpr)
}

/// DOM `MouseEvent.button` → agg-gui button.
pub(crate) fn mouse_button_from_dom(button: i16) -> MouseButton {
    match button {
        0 => MouseButton::Left,
        1 => MouseButton::Middle,
        2 => MouseButton::Right,
        n => MouseButton::Other(n.clamp(0, 255) as u8),
    }
}

/// Modifiers from a DOM event's `shiftKey/ctrlKey/altKey/metaKey`. The event's
/// own flags are authoritative — cached key state goes stale across an
/// Alt+Tab that never delivers the matching `keyup`.
pub(crate) fn modifiers(shift: bool, ctrl: bool, alt: bool, meta: bool) -> Modifiers {
    Modifiers {
        shift,
        ctrl,
        alt,
        meta,
    }
}

/// Number of buttons held according to a `MouseEvent.buttons` bitmask (bit 0
/// left, 1 right, 2 middle). Back/forward/eraser bits are ignored — they never
/// start a drag, and counting them would wedge the idle guard.
pub(crate) fn pressed_button_count(buttons: u16) -> u32 {
    (buttons & 0b111).count_ones()
}

/// The kinds of pointer the DOM reports in `PointerEvent.pointerType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PointerKind {
    Mouse,
    Pen,
    Touch,
}

impl PointerKind {
    /// Parse `pointerType`. Unknown / empty strings are treated as a mouse —
    /// the direct path that never synthesises gestures.
    pub(crate) fn from_dom(pointer_type: &str) -> Self {
        match pointer_type {
            "touch" => Self::Touch,
            "pen" => Self::Pen,
            _ => Self::Mouse,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dpr_sanitising() {
        assert_eq!(sanitize_dpr(2.0), 2.0);
        assert_eq!(sanitize_dpr(0.0), 1.0);
        assert_eq!(sanitize_dpr(-1.0), 1.0);
        assert_eq!(sanitize_dpr(f64::NAN), 1.0);
        assert_eq!(sanitize_dpr(0.25), MIN_DEVICE_SCALE);
    }

    #[test]
    fn backing_size_scales_and_never_hits_zero() {
        assert_eq!(backing_size(400.0, 300.0, 2.0), (800, 600));
        assert_eq!(backing_size(411.0, 731.0, 3.0), (1233, 2193));
        assert_eq!(backing_size(100.5, 100.5, 1.0), (100, 100));
        assert_eq!(backing_size(0.0, 0.0, 2.0), (1, 1));
        assert_eq!(backing_size(f64::NAN, 10.0, 1.0), (1, 10));
    }

    #[test]
    fn clamps_to_device_limit() {
        assert_eq!(clamp_to_max_dim((1233, 2193), 2048), (1233, 2048));
        assert_eq!(clamp_to_max_dim((0, 5), 0), (1, 1));
    }

    #[test]
    fn client_to_physical_offsets_then_scales() {
        assert_eq!(
            client_to_physical(110.0, 60.0, 10.0, 20.0, 2.0),
            (200.0, 80.0)
        );
    }

    #[test]
    fn button_mapping() {
        assert_eq!(mouse_button_from_dom(0), MouseButton::Left);
        assert_eq!(mouse_button_from_dom(1), MouseButton::Middle);
        assert_eq!(mouse_button_from_dom(2), MouseButton::Right);
        assert_eq!(mouse_button_from_dom(3), MouseButton::Other(3));
        assert_eq!(mouse_button_from_dom(-1), MouseButton::Other(0));
    }

    #[test]
    fn buttons_bitmask_counts_only_primary_three() {
        assert_eq!(pressed_button_count(0), 0);
        assert_eq!(pressed_button_count(0b1), 1);
        assert_eq!(pressed_button_count(0b111), 3);
        assert_eq!(pressed_button_count(0b11000), 0);
    }

    #[test]
    fn pointer_kind_parsing() {
        assert_eq!(PointerKind::from_dom("touch"), PointerKind::Touch);
        assert_eq!(PointerKind::from_dom("pen"), PointerKind::Pen);
        assert_eq!(PointerKind::from_dom("mouse"), PointerKind::Mouse);
        assert_eq!(PointerKind::from_dom(""), PointerKind::Mouse);
    }

    #[test]
    fn modifiers_pass_through() {
        let m = modifiers(true, false, true, false);
        assert!(m.shift && m.alt && !m.ctrl && !m.meta);
    }

    #[test]
    fn fit_backing_is_plain_dpr_within_the_limit() {
        assert_eq!(fit_backing(400.0, 300.0, 2.0, 8192), ((800, 600), 2.0));
    }

    #[test]
    fn fit_backing_reduces_scale_uniformly_over_the_limit() {
        // 1000x800 CSS at DPR 3 wants 3000x2400; a 2048 limit caps the scale.
        let ((w, h), scale) = fit_backing(1000.0, 800.0, 3.0, 2048);
        assert!((scale - 2.048).abs() < 1e-12);
        assert_eq!((w, h), (2048, 1638));
        assert!(w <= 2048 && h <= 2048);
        // Stable: fitting again at the same inputs gives the same size, so
        // the canvas is not resized every tick.
        assert_eq!(fit_backing(1000.0, 800.0, 3.0, 2048), ((w, h), scale));
    }

    #[test]
    fn fit_backing_pointer_mapping_matches_the_clamped_surface() {
        let ((w, h), scale) = fit_backing(1000.0, 800.0, 3.0, 2048);
        // The canvas's bottom-right CSS corner maps onto the surface corner.
        let (x, y) = client_to_physical(1010.0, 820.0, 10.0, 20.0, scale);
        assert!((x - w as f64).abs() <= 1.0, "x={x} w={w}");
        assert!((y - h as f64).abs() <= 1.0, "y={y} h={h}");
    }

    #[test]
    fn fit_backing_tall_canvas_limits_on_height() {
        let ((w, h), scale) = fit_backing(400.0, 1500.0, 2.0, 2048);
        assert!(h <= 2048 && w < 2048);
        assert!(scale < 2.0);
    }
}
