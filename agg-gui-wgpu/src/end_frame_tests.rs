//! Tests of `end_frame.rs`'s scissor decision: `clip_yields_visible_pixels`
//! is a pure shadow of `apply_clip`'s intersection maths, checked without a
//! live render pass.

use crate::WgpuGfxCtx;

/// Pure-function shadow of `end_frame::apply_clip`'s decision —
/// returns whether a clip rect would let any fragments through.  Mirrors
/// the same intersection math (Y-up → Y-down + viewport clamp + zero-area
/// reject) without needing a live `wgpu::RenderPass`.
pub(crate) fn clip_yields_visible_pixels(clip: Option<[i32; 4]>, vp: (f32, f32)) -> bool {
    let vp_w = vp.0 as u32;
    let vp_h = vp.1 as u32;
    match clip {
        None => vp_w > 0 && vp_h > 0,
        Some(scissor) => {
            let (x, y, w, h) = WgpuGfxCtx::yup_to_ydown_scissor(scissor, vp_h);
            let w = w.min(vp_w.saturating_sub(x));
            let h = h.min(vp_h.saturating_sub(y));
            w > 0 && h > 0
        }
    }
}

mod clip_tests {
    use super::clip_yields_visible_pixels;

    #[test]
    fn zero_height_clip_skips_draw() {
        // The collapsed-Window path passes `(x, y, w, 0)` as the children
        // clip rect — a zero-height area.  Without skipping the draw,
        // wgpu's sticky scissor state lets children paint over the title
        // bar.  Regression test for that bug: ensure the clip is rejected.
        assert!(!clip_yields_visible_pixels(
            Some([0, 0, 200, 0]),
            (400.0, 300.0)
        ));
    }

    #[test]
    fn zero_width_clip_skips_draw() {
        // Mirror case — a vertical zero-width clip should also be rejected.
        assert!(!clip_yields_visible_pixels(
            Some([0, 0, 0, 100]),
            (400.0, 300.0)
        ));
    }

    #[test]
    fn ordinary_clip_passes() {
        assert!(clip_yields_visible_pixels(
            Some([10, 10, 100, 50]),
            (400.0, 300.0)
        ));
    }

    #[test]
    fn no_clip_passes_when_viewport_is_non_empty() {
        assert!(clip_yields_visible_pixels(None, (400.0, 300.0)));
    }

    #[test]
    fn clip_entirely_outside_viewport_is_rejected() {
        // A scissor placed past the viewport's right edge has zero
        // intersection — should be rejected so the draw is skipped.
        assert!(!clip_yields_visible_pixels(
            Some([400, 0, 50, 50]),
            (400.0, 300.0)
        ));
    }
}
