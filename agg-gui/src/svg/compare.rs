//! SVG render comparison helpers shared by tests and diagnostics.
//!
//! The renderer aims for exact opaque color while allowing tiny coverage
//! differences at antialiased alpha edges.  Keeping this public gives demos,
//! regression tests, and downstream users one source of truth for whether a
//! rendered SVG is close enough to a reference image.

pub const DEFAULT_OPAQUE_RGB_TOLERANCE: u8 = 0;
pub const DEFAULT_ALPHA_TOLERANCE: u8 = 1;
pub const DEFAULT_TRANSLUCENT_RGB_TOLERANCE: u8 = 2;
pub const DEFAULT_VISUAL_RGB_TOLERANCE: f64 = 5.0;
pub const DEFAULT_MISMATCH_RATIO: f64 = 0.001;

#[derive(Clone, Copy, Debug)]
pub struct SvgCompareThresholds {
    pub opaque_rgb: u8,
    pub alpha: u8,
    pub translucent_rgb: u8,
    pub visual_rgb: f64,
    pub mismatch_ratio: f64,
}

impl Default for SvgCompareThresholds {
    fn default() -> Self {
        Self {
            opaque_rgb: DEFAULT_OPAQUE_RGB_TOLERANCE,
            alpha: DEFAULT_ALPHA_TOLERANCE,
            translucent_rgb: DEFAULT_TRANSLUCENT_RGB_TOLERANCE,
            visual_rgb: DEFAULT_VISUAL_RGB_TOLERANCE,
            mismatch_ratio: DEFAULT_MISMATCH_RATIO,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SvgCompareResult {
    pub pass: bool,
    pub ratio: f64,
    pub max_delta: u8,
    pub opaque_rgb_failures: usize,
    pub alpha_failures: usize,
    pub translucent_rgb_failures: usize,
    pub visual_failures: usize,
    pub max_alpha_delta: u8,
    pub max_visual_distance: f64,
}

impl SvgCompareResult {
    pub fn summary(&self) -> String {
        format!(
            "ratio={:.6}, max_delta={}, opaque_rgb_failures={}, alpha_failures={}, translucent_rgb_failures={}, visual_failures={}, max_alpha_delta={}, max_visual_distance={:.2}",
            self.ratio,
            self.max_delta,
            self.opaque_rgb_failures,
            self.alpha_failures,
            self.translucent_rgb_failures,
            self.visual_failures,
            self.max_alpha_delta,
            self.max_visual_distance
        )
    }
}

pub fn compare_svg_rgba(
    rendered: &[u8],
    reference: &[u8],
    thresholds: SvgCompareThresholds,
) -> SvgCompareResult {
    if rendered.len() != reference.len() || rendered.len() % 4 != 0 {
        return SvgCompareResult {
            pass: false,
            ratio: 1.0,
            max_delta: u8::MAX,
            opaque_rgb_failures: 0,
            alpha_failures: 0,
            translucent_rgb_failures: 0,
            visual_failures: 0,
            max_alpha_delta: u8::MAX,
            max_visual_distance: f64::INFINITY,
        };
    }

    let mut mismatched = 0usize;
    let mut max_delta = 0u8;
    let mut opaque_rgb_failures = 0usize;
    let mut alpha_failures = 0usize;
    let mut translucent_rgb_failures = 0usize;
    let mut visual_failures = 0usize;
    let mut max_alpha_delta = 0u8;
    let mut max_visual_distance_sq = 0u32;
    let visual_threshold_sq = (thresholds.visual_rgb * thresholds.visual_rgb).ceil() as u32;
    for (a, b) in rendered.chunks_exact(4).zip(reference.chunks_exact(4)) {
        let dr = a[0].abs_diff(b[0]);
        let dg = a[1].abs_diff(b[1]);
        let db = a[2].abs_diff(b[2]);
        let alpha_delta = a[3].abs_diff(b[3]);
        let rgb_delta = dr.max(dg).max(db);
        let rgba_delta = rgb_delta.max(alpha_delta);
        let premul_delta = premul_rgb_delta(a, b);
        let visual_distance_sq = visual_rgb_distance_sq_over_white(a, b);

        max_delta = max_delta.max(rgba_delta);
        max_alpha_delta = max_alpha_delta.max(alpha_delta);
        max_visual_distance_sq = max_visual_distance_sq.max(visual_distance_sq);

        let opaque_rgb_failed = a[3] == 255 && b[3] == 255 && rgb_delta > thresholds.opaque_rgb;
        let alpha_failed = alpha_delta > thresholds.alpha;
        let translucent_rgb_failed =
            (a[3] < 255 || b[3] < 255) && premul_delta > thresholds.translucent_rgb;
        let visual_failed = visual_distance_sq > visual_threshold_sq;

        opaque_rgb_failures += usize::from(opaque_rgb_failed);
        alpha_failures += usize::from(alpha_failed);
        translucent_rgb_failures += usize::from(translucent_rgb_failed);
        visual_failures += usize::from(visual_failed);

        if opaque_rgb_failed || alpha_failed || translucent_rgb_failed || visual_failed {
            mismatched += 1;
        }
    }

    let pixels = rendered.len() / 4;
    let ratio = mismatched as f64 / pixels.max(1) as f64;
    SvgCompareResult {
        pass: ratio <= thresholds.mismatch_ratio,
        ratio,
        max_delta,
        opaque_rgb_failures,
        alpha_failures,
        translucent_rgb_failures,
        visual_failures,
        max_alpha_delta,
        max_visual_distance: (max_visual_distance_sq as f64).sqrt(),
    }
}

fn premul_rgb_delta(a: &[u8], b: &[u8]) -> u8 {
    let aa = a[3] as u32;
    let ba = b[3] as u32;
    let pr = premul_channel(a[0], aa).abs_diff(premul_channel(b[0], ba));
    let pg = premul_channel(a[1], aa).abs_diff(premul_channel(b[1], ba));
    let pb = premul_channel(a[2], aa).abs_diff(premul_channel(b[2], ba));
    pr.max(pg).max(pb) as u8
}

fn visual_rgb_distance_sq_over_white(a: &[u8], b: &[u8]) -> u32 {
    let aa = a[3] as u32;
    let ba = b[3] as u32;
    let ar = composite_channel_over_white(a[0], aa);
    let ag = composite_channel_over_white(a[1], aa);
    let ab = composite_channel_over_white(a[2], aa);
    let br = composite_channel_over_white(b[0], ba);
    let bg = composite_channel_over_white(b[1], ba);
    let bb = composite_channel_over_white(b[2], ba);
    let dr = ar as i32 - br as i32;
    let dg = ag as i32 - bg as i32;
    let db = ab as i32 - bb as i32;
    (dr * dr + dg * dg + db * db) as u32
}

fn premul_channel(channel: u8, alpha: u32) -> u32 {
    ((channel as u32) * alpha + 127) / 255
}

fn composite_channel_over_white(channel: u8, alpha: u32) -> u32 {
    ((channel as u32) * alpha + 255 * (255 - alpha) + 127) / 255
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requires_opaque_rgb_to_match_exactly_by_default() {
        let rendered = [11, 20, 30, 255];
        let reference = [10, 20, 30, 255];

        let diff = compare_svg_rgba(&rendered, &reference, exact_thresholds());

        assert!(!diff.pass);
        assert_eq!(diff.opaque_rgb_failures, 1);
    }

    #[test]
    fn allows_tiny_alpha_coverage_error() {
        let rendered = [20, 40, 60, 128];
        let reference = [20, 40, 60, 129];

        let diff = compare_svg_rgba(&rendered, &reference, exact_thresholds());

        assert!(diff.pass, "{}", diff.summary());
    }

    #[test]
    fn rejects_large_alpha_coverage_error() {
        let rendered = [20, 40, 60, 128];
        let reference = [20, 40, 60, 134];

        let diff = compare_svg_rgba(&rendered, &reference, exact_thresholds());

        assert!(!diff.pass);
        assert_eq!(diff.alpha_failures, 1);
    }

    #[test]
    fn ignores_rgb_payload_under_zero_alpha() {
        let rendered = [255, 0, 0, 0];
        let reference = [0, 255, 0, 0];

        let diff = compare_svg_rgba(&rendered, &reference, exact_thresholds());

        assert!(diff.pass, "{}", diff.summary());
    }

    fn exact_thresholds() -> SvgCompareThresholds {
        SvgCompareThresholds {
            mismatch_ratio: 0.0,
            ..SvgCompareThresholds::default()
        }
    }
}
