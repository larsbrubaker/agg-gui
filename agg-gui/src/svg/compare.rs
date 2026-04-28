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
    let mut max_visual_distance = 0.0_f64;
    for (a, b) in rendered.chunks_exact(4).zip(reference.chunks_exact(4)) {
        let rgba_delta = max_rgba_delta(a, b);
        let rgb_delta = max_rgb_delta(a, b);
        let alpha_delta = a[3].abs_diff(b[3]);
        let premul_delta = max_rgb_delta(&premultiply_pixel(a), &premultiply_pixel(b));
        let visual_distance = color_distance(composite_over_white(a), composite_over_white(b));

        max_delta = max_delta.max(rgba_delta);
        max_alpha_delta = max_alpha_delta.max(alpha_delta);
        max_visual_distance = max_visual_distance.max(visual_distance);

        let opaque_rgb_failed = a[3] == 255 && b[3] == 255 && rgb_delta > thresholds.opaque_rgb;
        let alpha_failed = alpha_delta > thresholds.alpha;
        let translucent_rgb_failed =
            (a[3] < 255 || b[3] < 255) && premul_delta > thresholds.translucent_rgb;
        let visual_failed = visual_distance > thresholds.visual_rgb;

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
        max_visual_distance,
    }
}

fn max_rgba_delta(a: &[u8], b: &[u8]) -> u8 {
    a.iter()
        .zip(b.iter())
        .map(|(&a, &b)| a.abs_diff(b))
        .max()
        .unwrap_or(0)
}

fn max_rgb_delta(a: &[u8], b: &[u8]) -> u8 {
    a[..3]
        .iter()
        .zip(b[..3].iter())
        .map(|(&a, &b)| a.abs_diff(b))
        .max()
        .unwrap_or(0)
}

fn premultiply_pixel(px: &[u8]) -> [u8; 4] {
    let a = px[3] as u32;
    [
        (((px[0] as u32) * a + 127) / 255) as u8,
        (((px[1] as u32) * a + 127) / 255) as u8,
        (((px[2] as u32) * a + 127) / 255) as u8,
        px[3],
    ]
}

fn composite_over_white(px: &[u8]) -> [u8; 3] {
    let a = px[3] as u32;
    [
        (((px[0] as u32) * a + 255 * (255 - a) + 127) / 255) as u8,
        (((px[1] as u32) * a + 255 * (255 - a) + 127) / 255) as u8,
        (((px[2] as u32) * a + 255 * (255 - a) + 127) / 255) as u8,
    ]
}

fn color_distance(a: [u8; 3], b: [u8; 3]) -> f64 {
    let dr = a[0] as f64 - b[0] as f64;
    let dg = a[1] as f64 - b[1] as f64;
    let db = a[2] as f64 - b[2] as f64;
    (dr * dr + dg * dg + db * db).sqrt()
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
