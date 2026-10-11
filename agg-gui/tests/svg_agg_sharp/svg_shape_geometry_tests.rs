//! agg-sharp's `SvgShapeGeometryTests` (the outlines SvgShapes builds for
//! curved shapes, which match usvg's: kurbo's arc-to-cubic at a tolerance of
//! 0.1 user units), as a render behaviour test.
//!
//! C#'s three tests (`AnArcIsOneCubicPerQuarterAtAnOrdinaryRadius`,
//! `ALargeArcIsCutIntoMoreCubicsAsKurboCutsIt`, `AnArcToItsOwnStartIsALine`)
//! count the cubics agg-sharp's own arc conversion emits; here usvg and kurbo
//! do that conversion, so there is no agg-gui outline to count. What they
//! guard, an arc drawing as its circle, is checked on the drawn pixels.

use super::common::render;

#[test]
fn rust_only_an_arc_draws_as_its_circle() {
    // Two half arcs of radius 40 about (50, 50).
    let image = render("<path d=\"M 90 50 A 40 40 0 1 1 10 50 A 40 40 0 1 1 90 50 Z\"/>");
    for step in 0..32 {
        let radians = f64::from(step) * std::f64::consts::PI / 16.0;
        let sample = |radius: f64| {
            let x = 50.0 + radius * radians.cos();
            let y = 50.0 + radius * radians.sin();
            // The pixel holding (x, y); its far corner is at most 0.71 further out.
            image.alpha_at(x.floor() as u32, y.floor() as u32)
        };
        assert_eq!(sample(38.5), 255, "inside at step {step}");
        assert_eq!(sample(41.5), 0, "outside at step {step}");
    }
}
