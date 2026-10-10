//! Zooming the SVG Test view: changing the zoom level while keeping a content
//! point (or the viewport centre) fixed on screen, and recomputing the scroll
//! ranges for the new content size.
//!
//! Split out of `svg.rs` (the SVG Test widgets) for the 800-line cap; the
//! header's zoom buttons (`controls.rs`) and the body's Ctrl+wheel handler
//! call these.

use super::*;

pub(super) fn zoom_svg_around_viewport_center(
    samples: &[SvgSampleRender],
    zoom: &Rc<Cell<f64>>,
    v_offset: &Rc<Cell<f64>>,
    v_max: &Rc<Cell<f64>>,
    h_offset: &Rc<Cell<f64>>,
    h_max: &Rc<Cell<f64>>,
    new_zoom: f64,
) {
    let old_zoom = zoom.get();
    let old_w = svg_content_width(samples, old_zoom);
    let old_h = svg_content_height(samples, old_zoom);
    let viewport_w = (old_w - h_max.get()).max(1.0);
    let viewport_h = (old_h - v_max.get()).max(1.0);
    zoom_svg_around_content_point(
        samples,
        zoom,
        v_offset,
        v_max,
        h_offset,
        h_max,
        h_offset.get() + viewport_w * 0.5,
        v_offset.get() + viewport_h * 0.5,
        new_zoom,
    );
}

// Each argument is a distinct drawing/geometry input; a struct would only rename them.
#[allow(clippy::too_many_arguments)]
pub(super) fn zoom_svg_around_content_point(
    samples: &[SvgSampleRender],
    zoom: &Rc<Cell<f64>>,
    v_offset: &Rc<Cell<f64>>,
    v_max: &Rc<Cell<f64>>,
    h_offset: &Rc<Cell<f64>>,
    h_max: &Rc<Cell<f64>>,
    anchor_x: f64,
    anchor_top_y: f64,
    new_zoom: f64,
) {
    let old_zoom = zoom.get();
    if (new_zoom - old_zoom).abs() < 0.001 {
        return;
    }

    let old_w = svg_content_width(samples, old_zoom);
    let old_h = svg_content_height(samples, old_zoom);
    let new_w = svg_content_width(samples, new_zoom);
    let new_h = svg_content_height(samples, new_zoom);
    let viewport_w = (old_w - h_max.get()).max(1.0);
    let viewport_h = (old_h - v_max.get()).max(1.0);
    let screen_x = anchor_x - h_offset.get();
    let screen_top_y = anchor_top_y - v_offset.get();
    let new_h_max = (new_w - viewport_w).max(0.0);
    let new_v_max = (new_h - viewport_h).max(0.0);
    let scaled_anchor_x = anchor_x * (new_w / old_w.max(1.0));
    let scaled_anchor_top_y = anchor_top_y * (new_h / old_h.max(1.0));

    zoom.set(new_zoom);
    h_max.set(new_h_max);
    v_max.set(new_v_max);
    h_offset.set((scaled_anchor_x - screen_x).clamp(0.0, new_h_max));
    v_offset.set((scaled_anchor_top_y - screen_top_y).clamp(0.0, new_v_max));
    agg_gui::animation::request_draw();
}
