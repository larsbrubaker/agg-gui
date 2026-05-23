//! Tree visitor for SVG rendering — extracted from `svg.rs` to keep the
//! parent module under the 800-line guardrail.
//!
//! Walks the usvg tree, dispatching to fill/stroke/image/text drawing
//! against the [`DrawCtx`] bridge. Coordinate math, render-state, and
//! the usvg→agg-gui mapping helpers still live in the parent module and
//! are pulled in through `super`.
//!
//! Re-entry points exposed back to the parent: [`render_group`] is the
//! top-level entry the public `render_svg_tree_*` functions call.

use crate::draw_ctx::DrawCtx;
use crate::framebuffer::unpremultiply_rgba_inplace;

use super::{
    apply_transform, emit_path, map_fill_rule, map_line_cap, map_line_join,
    render_svg_tree_to_framebuffer, transformed_rect, transformed_svg_rect, SvgRenderError,
    SvgRenderState,
};

pub(super) fn render_group(
    group: &usvg::Group,
    ctx: &mut dyn DrawCtx,
    parent_state: SvgRenderState,
) -> Result<(), SvgRenderError> {
    if parent_state.source_cull.is_some_and(|cull| {
        !cull.intersects(transformed_svg_rect(
            group.bounding_box(),
            group.abs_transform(),
        ))
    }) {
        return Ok(());
    }

    let group_opacity = group.opacity().get();
    if group_opacity < 1.0 && parent_state.opacity > 0.0 && ctx.supports_compositing_layers() {
        return render_isolated_group_with_opacity(group, ctx, parent_state, group_opacity);
    }

    let state = SvgRenderState {
        opacity: parent_state.opacity * group_opacity,
        ..parent_state
    };

    ctx.save();
    apply_group_clip(ctx, group);
    for node in group.children() {
        match node {
            usvg::Node::Group(group) => render_group(group, ctx, state)?,
            usvg::Node::Path(path) => render_path(path, ctx, state),
            usvg::Node::Image(image) => render_image(image, ctx, state)?,
            usvg::Node::Text(text) => render_text(text, ctx, state)?,
        }
    }
    ctx.restore();
    Ok(())
}

fn render_isolated_group_with_opacity(
    group: &usvg::Group,
    ctx: &mut dyn DrawCtx,
    parent_state: SvgRenderState,
    group_opacity: f32,
) -> Result<(), SvgRenderError> {
    let saved_transform = ctx.transform();

    ctx.save();
    ctx.reset_transform();
    ctx.push_layer_with_alpha(
        parent_state.layer_width,
        parent_state.layer_height,
        (parent_state.opacity * group_opacity) as f64,
    );
    ctx.set_transform(saved_transform);

    let state = SvgRenderState {
        opacity: 1.0,
        ..parent_state
    };
    ctx.save();
    apply_group_clip(ctx, group);
    for node in group.children() {
        match node {
            usvg::Node::Group(group) => render_group(group, ctx, state)?,
            usvg::Node::Path(path) => render_path(path, ctx, state),
            usvg::Node::Image(image) => render_image(image, ctx, state)?,
            usvg::Node::Text(text) => render_text(text, ctx, state)?,
        }
    }
    ctx.restore();
    ctx.pop_layer();
    ctx.restore();
    Ok(())
}

fn apply_group_clip(ctx: &mut dyn DrawCtx, group: &usvg::Group) {
    if let Some(clip) = group.clip_path() {
        apply_clip_path(ctx, clip);
    }
}

fn apply_clip_path(ctx: &mut dyn DrawCtx, clip: &usvg::ClipPath) {
    // The bridge currently exposes rectangular clipping only.  This still
    // covers the common SVG badge pattern and provides a conservative fallback
    // until arbitrary path masks are wired through the draw backends.
    let bbox = clip.root().bounding_box();
    ctx.clip_rect(
        bbox.x() as f64,
        bbox.y() as f64,
        bbox.width() as f64,
        bbox.height() as f64,
    );

    if let Some(clip) = clip.clip_path() {
        apply_clip_path(ctx, clip);
    }
}

fn render_text(
    text: &usvg::Text,
    ctx: &mut dyn DrawCtx,
    state: SvgRenderState,
) -> Result<(), SvgRenderError> {
    if state.opacity <= 0.0 {
        return Ok(());
    }

    ctx.save();
    apply_transform(ctx, text.abs_transform());
    let result = render_group(text.flattened(), ctx, state);
    ctx.restore();
    result
}

fn render_path(path: &usvg::Path, ctx: &mut dyn DrawCtx, state: SvgRenderState) {
    if !path.is_visible() {
        return;
    }
    if state.source_cull.is_some_and(|cull| {
        let fill_intersects = path.fill().is_some_and(|_| {
            cull.intersects(transformed_svg_rect(
                path.bounding_box(),
                path.abs_transform(),
            ))
        });
        let stroke_intersects = path.stroke().is_some_and(|_| {
            cull.intersects(transformed_svg_rect(
                path.stroke_bounding_box(),
                path.abs_transform(),
            ))
        });
        !fill_intersects && !stroke_intersects
    }) {
        return;
    }

    ctx.save();
    apply_transform(ctx, path.abs_transform());

    match path.paint_order() {
        usvg::PaintOrder::FillAndStroke => {
            fill_path(path, ctx, state);
            stroke_path(path, ctx, state);
        }
        usvg::PaintOrder::StrokeAndFill => {
            stroke_path(path, ctx, state);
            fill_path(path, ctx, state);
        }
    }

    ctx.restore();
}

fn fill_path(path: &usvg::Path, ctx: &mut dyn DrawCtx, state: SvgRenderState) {
    let Some(fill) = path.fill() else {
        return;
    };

    emit_path(path, ctx);
    if !apply_fill_paint(
        ctx,
        fill.paint(),
        state.opacity * fill.opacity().get(),
        Some(path.bounding_box()),
    ) {
        return;
    }
    ctx.set_fill_rule(map_fill_rule(fill.rule()));
    ctx.fill();
}

fn stroke_path(path: &usvg::Path, ctx: &mut dyn DrawCtx, state: SvgRenderState) {
    let Some(stroke) = path.stroke() else {
        return;
    };
    if !apply_stroke_paint(
        ctx,
        stroke.paint(),
        state.opacity * stroke.opacity().get(),
        Some(path.stroke_bounding_box()),
    ) {
        return;
    }

    emit_path(path, ctx);
    ctx.set_line_width(stroke.width().get() as f64);
    ctx.set_line_cap(map_line_cap(stroke.linecap()));
    ctx.set_line_join(map_line_join(stroke.linejoin()));
    ctx.set_miter_limit(stroke.miterlimit().get() as f64);
    let dashes: Vec<f64> = stroke
        .dasharray()
        .map(|items| items.iter().map(|v| *v as f64).collect())
        .unwrap_or_default();
    ctx.set_line_dash(&dashes, stroke.dashoffset() as f64);
    ctx.stroke();
}

fn apply_fill_paint(
    ctx: &mut dyn DrawCtx,
    paint: &usvg::Paint,
    opacity: f32,
    object_bbox: Option<usvg::Rect>,
) -> bool {
    if let usvg::Paint::Pattern(pattern) = paint {
        if !ctx.supports_fill_pattern() {
            return false;
        }
        if let Some(pattern) = super::pattern::render_pattern_paint(pattern, opacity, object_bbox) {
            ctx.set_fill_pattern(pattern);
            return true;
        }
        return false;
    }

    super::paint::apply_fill_paint(ctx, paint, opacity)
}

fn apply_stroke_paint(
    ctx: &mut dyn DrawCtx,
    paint: &usvg::Paint,
    opacity: f32,
    object_bbox: Option<usvg::Rect>,
) -> bool {
    if let usvg::Paint::Pattern(pattern) = paint {
        if !ctx.supports_stroke_pattern() {
            return false;
        }
        if let Some(pattern) = super::pattern::render_pattern_paint(pattern, opacity, object_bbox) {
            ctx.set_stroke_pattern(pattern);
            return true;
        }
        return false;
    }

    super::paint::apply_stroke_paint(ctx, paint, opacity)
}

fn render_image(
    image: &usvg::Image,
    ctx: &mut dyn DrawCtx,
    state: SvgRenderState,
) -> Result<(), SvgRenderError> {
    if !image.is_visible() || state.opacity <= 0.0 {
        return Ok(());
    }

    match image.kind() {
        usvg::ImageKind::JPEG(data)
        | usvg::ImageKind::PNG(data)
        | usvg::ImageKind::GIF(data)
        | usvg::ImageKind::WEBP(data) => {
            let decoded = image::load_from_memory(data)?;
            let rgba = decoded.to_rgba8();
            let (img_w, img_h) = (rgba.width(), rgba.height());
            if img_w == 0 || img_h == 0 {
                return Ok(());
            }
            let mut pixels = rgba.into_raw();
            if state.opacity < 1.0 {
                for px in pixels.chunks_exact_mut(4) {
                    px[3] = ((px[3] as f32 * state.opacity).clamp(0.0, 255.0)) as u8;
                }
            }

            let size = image.size();
            ctx.save();
            apply_transform(ctx, image.abs_transform());
            let t = ctx.transform();
            let (dst_x, dst_y, dst_w, dst_h) =
                transformed_rect(&t, size.width() as f64, size.height() as f64);
            ctx.reset_transform();
            ctx.draw_image_rgba(&pixels, img_w, img_h, dst_x, dst_y, dst_w, dst_h);
            ctx.restore();
        }
        usvg::ImageKind::SVG(tree) => {
            let fb = render_svg_tree_to_framebuffer(tree)?;
            let mut pixels = fb.pixels_flipped();
            unpremultiply_rgba_inplace(&mut pixels);
            let size = image.size();
            ctx.save();
            apply_transform(ctx, image.abs_transform());
            let t = ctx.transform();
            let (dst_x, dst_y, dst_w, dst_h) =
                transformed_rect(&t, size.width() as f64, size.height() as f64);
            ctx.reset_transform();
            ctx.draw_image_rgba(&pixels, fb.width(), fb.height(), dst_x, dst_y, dst_w, dst_h);
            ctx.restore();
        }
    }

    Ok(())
}
