//! How finely the software rasterisers flatten curves.
//!
//! `GfxCtx` and `LcdGfxCtx` flatten a path's curves (`ConvCurve`) in user
//! space and transform the straight segments to pixels afterwards, so AGG's
//! curve tolerance (half a unit at approximation scale 1) is measured in user
//! units.  [`curve_approximation_scale`] makes it half a *pixel* whatever the
//! transform, as agg-sharp's `SvgRenderer.DrawShape` does
//! (`FlattenCurves { ResolutionScale = sqrt|det| }`): an SVG icon drawn larger
//! than its viewBox no longer shows facets, and one drawn smaller flattens as
//! coarsely as agg-sharp's, so its coverage matches agg-sharp's pixel for
//! pixel (`internet.svg` at 16x16 draws its opaque ring pixels at alpha 255,
//! not 254).  Identity and translation-only transforms keep scale 1.

use agg_rust::trans_affine::TransAffine;

/// The `ConvCurve` approximation scale for curves drawn through `transform`:
/// the square root of its absolute determinant (its area scale), or 1 for a
/// degenerate or non-finite transform.
pub(crate) fn curve_approximation_scale(transform: &TransAffine) -> f64 {
    let scale = transform.determinant().abs().sqrt();
    if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_and_translation_keep_scale_one() {
        assert_eq!(curve_approximation_scale(&TransAffine::new()), 1.0);
        let t = TransAffine::new_translation(5.0, -3.0);
        assert_eq!(curve_approximation_scale(&t), 1.0);
    }

    #[test]
    fn scale_is_the_area_scale() {
        assert_eq!(
            curve_approximation_scale(&TransAffine::new_scaling(0.5, 0.5)),
            0.5
        );
        assert_eq!(
            curve_approximation_scale(&TransAffine::new_scaling(2.0, 8.0)),
            4.0
        );
        // SVG's y flip keeps the magnitude.
        assert_eq!(
            curve_approximation_scale(&TransAffine::new_scaling(3.0, -3.0)),
            3.0
        );
    }

    #[test]
    fn degenerate_transform_falls_back_to_one() {
        assert_eq!(
            curve_approximation_scale(&TransAffine::new_scaling(0.0, 2.0)),
            1.0
        );
    }
}
