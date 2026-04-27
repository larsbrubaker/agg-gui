//! Stroke outline helpers for `GfxCtx`.
//!
//! Gradient strokes are rendered by first converting the stroked curve into a
//! fillable outline, then reusing the same sampled-gradient fill path.

use super::*;

pub(super) fn materialize_stroke_outline(
    path: &mut PathStorage,
    width: f64,
    join: LineJoin,
    cap: LineCap,
    miter_limit: f64,
    dashes: &[f64],
    dash_offset: f64,
) -> PathStorage {
    let mut materialized = PathStorage::new();
    let mut curves = ConvCurve::new(path);
    if dashes.is_empty() {
        let mut stroke = ConvStroke::new(&mut curves);
        configure_stroke(&mut stroke, width, join, cap, miter_limit);
        materialized.concat_path(&mut stroke, 0);
    } else {
        let mut dash = ConvDash::new(&mut curves);
        configure_dashes(&mut dash, dashes, dash_offset);
        let mut stroke = ConvStroke::new(dash);
        configure_stroke(&mut stroke, width, join, cap, miter_limit);
        materialized.concat_path(&mut stroke, 0);
    }
    materialized
}

fn configure_stroke<VS: VertexSource>(
    stroke: &mut ConvStroke<VS>,
    width: f64,
    join: LineJoin,
    cap: LineCap,
    miter_limit: f64,
) {
    stroke.set_width(width);
    stroke.set_line_join(join);
    stroke.set_line_cap(cap);
    stroke.set_miter_limit(miter_limit);
}

fn configure_dashes<VS: VertexSource>(dash: &mut ConvDash<VS>, dashes: &[f64], dash_offset: f64) {
    let mut chunks = dashes.chunks_exact(2);
    for pair in &mut chunks {
        dash.add_dash(pair[0], pair[1]);
    }
    if let Some(&last) = chunks.remainder().first() {
        dash.add_dash(last, last);
    }
    dash.dash_start(dash_offset);
}
