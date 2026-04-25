//! SVG rendering support for `agg-gui`.
//!
//! This module is the library-owned SVG renderer used by tests, demos, and
//! applications.  It parses SVG with `usvg`, then emits drawing commands only
//! through [`crate::draw_ctx::DrawCtx`] so RGBA software, LCD coverage, and
//! hardware targets all share one render path.

use std::fmt;

use agg_rust::math_stroke::{LineCap, LineJoin};
use agg_rust::trans_affine::TransAffine;
use usvg::tiny_skia_path::PathSegment;

use crate::color::Color;
use crate::draw_ctx::{DrawCtx, FillRule};
use crate::framebuffer::Framebuffer;
use crate::gfx_ctx::GfxCtx;
use crate::lcd_coverage::LcdBuffer;
use crate::lcd_gfx_ctx::LcdGfxCtx;

#[derive(Clone, Copy, Debug)]
struct SvgRenderState {
    opacity: f32,
}

impl Default for SvgRenderState {
    fn default() -> Self {
        Self { opacity: 1.0 }
    }
}

/// Errors returned by the SVG renderer.
#[derive(Debug)]
pub enum SvgRenderError {
    /// The SVG data could not be parsed by `usvg`.
    Parse(usvg::Error),
    /// A raster image referenced by the SVG could not be decoded.
    DecodeImage(image::ImageError),
}

impl fmt::Display for SvgRenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SvgRenderError::Parse(err) => write!(f, "failed to parse SVG: {err}"),
            SvgRenderError::DecodeImage(err) => write!(f, "failed to decode SVG image: {err}"),
        }
    }
}

impl std::error::Error for SvgRenderError {}

impl From<usvg::Error> for SvgRenderError {
    fn from(err: usvg::Error) -> Self {
        SvgRenderError::Parse(err)
    }
}

impl From<image::ImageError> for SvgRenderError {
    fn from(err: image::ImageError) -> Self {
        SvgRenderError::DecodeImage(err)
    }
}

/// Parse an SVG document and render it into `ctx`.
///
/// This is a convenience wrapper around [`render_svg_tree`].  Callers that
/// already cache a `usvg::Tree` should use [`render_svg_tree`] directly.
pub fn render_svg(data: &[u8], ctx: &mut dyn DrawCtx) -> Result<(), SvgRenderError> {
    let options = usvg::Options::default();
    let tree = usvg::Tree::from_data(data, &options)?;
    render_svg_tree(&tree, ctx)
}

/// Parse an SVG document and render it into `ctx` using an explicit output
/// pixel size for the document viewport.
pub fn render_svg_at_size(
    data: &[u8],
    ctx: &mut dyn DrawCtx,
    width: u32,
    height: u32,
) -> Result<(), SvgRenderError> {
    let options = usvg::Options::default();
    let tree = usvg::Tree::from_data(data, &options)?;
    render_svg_tree_at_size(&tree, ctx, width, height)
}

/// Parse an SVG document and render it into a newly allocated RGBA framebuffer.
///
/// This is the library API the SVG regression tests and demo viewer should use
/// for the `agg-rgba-bitmap render` column.
pub fn render_svg_to_framebuffer(data: &[u8]) -> Result<Framebuffer, SvgRenderError> {
    let options = usvg::Options::default();
    let tree = usvg::Tree::from_data(data, &options)?;
    render_svg_tree_to_framebuffer(&tree)
}

/// Parse an SVG document and render it into an RGBA framebuffer with an
/// explicit pixel size.
///
/// The resvg test suite reference PNGs are not always the SVG document's
/// intrinsic size, so regression tests and viewers should use this helper when
/// they need render output to match a reference image one-to-one.
pub fn render_svg_to_framebuffer_at_size(
    data: &[u8],
    width: u32,
    height: u32,
) -> Result<Framebuffer, SvgRenderError> {
    let options = usvg::Options::default();
    let tree = usvg::Tree::from_data(data, &options)?;
    render_svg_tree_to_framebuffer_at_size(&tree, width, height)
}

/// Render a parsed SVG tree into a newly allocated RGBA framebuffer.
pub fn render_svg_tree_to_framebuffer(tree: &usvg::Tree) -> Result<Framebuffer, SvgRenderError> {
    let width = tree.size().width().ceil().max(1.0) as u32;
    let height = tree.size().height().ceil().max(1.0) as u32;
    render_svg_tree_to_framebuffer_at_size(tree, width, height)
}

/// Render a parsed SVG tree into an RGBA framebuffer with an explicit pixel size.
pub fn render_svg_tree_to_framebuffer_at_size(
    tree: &usvg::Tree,
    width: u32,
    height: u32,
) -> Result<Framebuffer, SvgRenderError> {
    let width = width.max(1);
    let height = height.max(1);
    let mut fb = Framebuffer::new(width, height);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        render_svg_tree_at_size(tree, &mut ctx, width, height)?;
    }
    Ok(fb)
}

/// Parse an SVG document and render it into a newly allocated LCD coverage buffer.
///
/// This is the library API the SVG regression tests and demo viewer should use
/// for the `agg-lcd-bitmap render` column.
pub fn render_svg_to_lcd_buffer(data: &[u8]) -> Result<LcdBuffer, SvgRenderError> {
    let options = usvg::Options::default();
    let tree = usvg::Tree::from_data(data, &options)?;
    render_svg_tree_to_lcd_buffer(&tree)
}

/// Parse an SVG document and render it into an LCD coverage buffer with an
/// explicit pixel size.
pub fn render_svg_to_lcd_buffer_at_size(
    data: &[u8],
    width: u32,
    height: u32,
) -> Result<LcdBuffer, SvgRenderError> {
    let options = usvg::Options::default();
    let tree = usvg::Tree::from_data(data, &options)?;
    render_svg_tree_to_lcd_buffer_at_size(&tree, width, height)
}

/// Render a parsed SVG tree into a newly allocated LCD coverage buffer.
pub fn render_svg_tree_to_lcd_buffer(tree: &usvg::Tree) -> Result<LcdBuffer, SvgRenderError> {
    let width = tree.size().width().ceil().max(1.0) as u32;
    let height = tree.size().height().ceil().max(1.0) as u32;
    render_svg_tree_to_lcd_buffer_at_size(tree, width, height)
}

/// Render a parsed SVG tree into an LCD coverage buffer with an explicit pixel size.
pub fn render_svg_tree_to_lcd_buffer_at_size(
    tree: &usvg::Tree,
    width: u32,
    height: u32,
) -> Result<LcdBuffer, SvgRenderError> {
    let width = width.max(1);
    let height = height.max(1);
    let mut buffer = LcdBuffer::new(width, height);
    {
        let mut ctx = LcdGfxCtx::new(&mut buffer);
        render_svg_tree_at_size(tree, &mut ctx, width, height)?;
    }
    Ok(buffer)
}

/// Render a parsed `usvg::Tree` into `ctx`.
///
/// The tree's native SVG coordinate system is Y-down.  This function installs
/// a root transform that maps it into `agg-gui`'s Y-up convention before any
/// node commands are emitted.
pub fn render_svg_tree(tree: &usvg::Tree, ctx: &mut dyn DrawCtx) -> Result<(), SvgRenderError> {
    let width = tree.size().width().ceil().max(1.0) as u32;
    let height = tree.size().height().ceil().max(1.0) as u32;
    render_svg_tree_at_size(tree, ctx, width, height)
}

/// Render a parsed `usvg::Tree` into `ctx`, fitting its document viewport into
/// an explicit output pixel size.
pub fn render_svg_tree_at_size(
    tree: &usvg::Tree,
    ctx: &mut dyn DrawCtx,
    width: u32,
    height: u32,
) -> Result<(), SvgRenderError> {
    let saved_transform = ctx.transform();
    let mut svg_to_ctx = saved_transform;
    svg_to_ctx.premultiply(&svg_y_down_to_ctx_y_up(tree, width, height));

    ctx.save();
    ctx.set_transform(svg_to_ctx);
    render_group(tree.root(), ctx, SvgRenderState::default())?;
    ctx.restore();
    Ok(())
}

fn svg_y_down_to_ctx_y_up(tree: &usvg::Tree, width: u32, height: u32) -> TransAffine {
    let sx = width.max(1) as f64 / tree.size().width().max(1.0) as f64;
    let sy = height.max(1) as f64 / tree.size().height().max(1.0) as f64;
    TransAffine::new_custom(sx, 0.0, 0.0, -sy, 0.0, height.max(1) as f64)
}

fn render_group(
    group: &usvg::Group,
    ctx: &mut dyn DrawCtx,
    parent_state: SvgRenderState,
) -> Result<(), SvgRenderError> {
    let state = SvgRenderState {
        opacity: parent_state.opacity * group.opacity().get(),
    };

    for node in group.children() {
        match node {
            usvg::Node::Group(group) => render_group(group, ctx, state)?,
            usvg::Node::Path(path) => render_path(path, ctx, state),
            usvg::Node::Image(image) => render_image(image, ctx, state)?,
            // Text lands in a later phase.  `usvg` can convert text to paths,
            // but the first library renderer slices keep it explicit.
            usvg::Node::Text(_) => {}
        }
    }
    Ok(())
}

fn render_path(path: &usvg::Path, ctx: &mut dyn DrawCtx, state: SvgRenderState) {
    if !path.is_visible() {
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
    let Some(color) = solid_paint(fill.paint(), state.opacity * fill.opacity().get()) else {
        return;
    };

    emit_path(path, ctx);
    ctx.set_fill_color(color);
    ctx.set_fill_rule(map_fill_rule(fill.rule()));
    ctx.fill();
}

fn stroke_path(path: &usvg::Path, ctx: &mut dyn DrawCtx, state: SvgRenderState) {
    let Some(stroke) = path.stroke() else {
        return;
    };
    let Some(color) = solid_paint(stroke.paint(), state.opacity * stroke.opacity().get()) else {
        return;
    };

    emit_path(path, ctx);
    ctx.set_stroke_color(color);
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
            ctx.save();
            apply_transform(ctx, image.abs_transform());
            render_svg_tree(tree, ctx)?;
            ctx.restore();
        }
    }

    Ok(())
}

fn emit_path(path: &usvg::Path, ctx: &mut dyn DrawCtx) {
    ctx.begin_path();
    for segment in path.data().segments() {
        match segment {
            PathSegment::MoveTo(p) => ctx.move_to(p.x as f64, p.y as f64),
            PathSegment::LineTo(p) => ctx.line_to(p.x as f64, p.y as f64),
            PathSegment::QuadTo(p1, p2) => {
                ctx.quad_to(p1.x as f64, p1.y as f64, p2.x as f64, p2.y as f64)
            }
            PathSegment::CubicTo(p1, p2, p3) => ctx.cubic_to(
                p1.x as f64,
                p1.y as f64,
                p2.x as f64,
                p2.y as f64,
                p3.x as f64,
                p3.y as f64,
            ),
            PathSegment::Close => ctx.close_path(),
        }
    }
}

fn solid_paint(paint: &usvg::Paint, opacity: f32) -> Option<Color> {
    match paint {
        usvg::Paint::Color(color) => Some(Color::rgba(
            color.red as f32 / 255.0,
            color.green as f32 / 255.0,
            color.blue as f32 / 255.0,
            opacity,
        )),
        usvg::Paint::LinearGradient(_)
        | usvg::Paint::RadialGradient(_)
        | usvg::Paint::Pattern(_) => None,
    }
}

fn apply_transform(ctx: &mut dyn DrawCtx, transform: usvg::Transform) {
    let mut current = ctx.transform();
    let node_transform = TransAffine::new_custom(
        transform.sx as f64,
        transform.ky as f64,
        transform.kx as f64,
        transform.sy as f64,
        transform.tx as f64,
        transform.ty as f64,
    );
    current.premultiply(&node_transform);
    ctx.set_transform(current);
}

fn transformed_rect(transform: &TransAffine, width: f64, height: f64) -> (f64, f64, f64, f64) {
    let corners = [(0.0, 0.0), (width, 0.0), (width, height), (0.0, height)];
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for (mut x, mut y) in corners {
        transform.transform(&mut x, &mut y);
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    }

    (min_x, min_y, (max_x - min_x).abs(), (max_y - min_y).abs())
}

fn map_line_cap(cap: usvg::LineCap) -> LineCap {
    match cap {
        usvg::LineCap::Butt => LineCap::Butt,
        usvg::LineCap::Round => LineCap::Round,
        usvg::LineCap::Square => LineCap::Square,
    }
}

fn map_line_join(join: usvg::LineJoin) -> LineJoin {
    match join {
        usvg::LineJoin::Miter | usvg::LineJoin::MiterClip => LineJoin::Miter,
        usvg::LineJoin::Round => LineJoin::Round,
        usvg::LineJoin::Bevel => LineJoin::Bevel,
    }
}

fn map_fill_rule(rule: usvg::FillRule) -> FillRule {
    match rule {
        usvg::FillRule::NonZero => FillRule::NonZero,
        usvg::FillRule::EvenOdd => FillRule::EvenOdd,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framebuffer::Framebuffer;
    use crate::gfx_ctx::GfxCtx;
    use base64::Engine;

    #[test]
    fn renders_solid_path_via_library_api() {
        let svg = br##"
            <svg xmlns="http://www.w3.org/2000/svg" width="4" height="4">
                <rect x="1" y="1" width="2" height="2" fill="#ff0000"/>
            </svg>
        "##;

        let mut fb = Framebuffer::new(4, 4);
        {
            let mut ctx = GfxCtx::new(&mut fb);
            render_svg(svg, &mut ctx).expect("SVG should render");
        }

        let center = ((2 * fb.width() + 2) * 4) as usize;
        assert_eq!(&fb.pixels()[center..center + 4], &[255, 0, 0, 255]);
    }

    #[test]
    fn renders_to_framebuffer_via_library_target_helper() {
        let svg = br##"
            <svg xmlns="http://www.w3.org/2000/svg" width="2" height="2">
                <rect width="2" height="2" fill="#ff0000"/>
            </svg>
        "##;

        let fb = render_svg_to_framebuffer(svg).expect("SVG should render");
        assert_eq!(fb.width(), 2);
        assert_eq!(fb.height(), 2);
        assert_eq!(&fb.pixels()[0..4], &[255, 0, 0, 255]);
    }

    #[test]
    fn renders_resvg_suite_case_at_reference_png_size() {
        let svg = include_bytes!("../../tests/resvg-test-suite/tests/shapes/rect/simple-case.svg");
        let png = include_bytes!("../../tests/resvg-test-suite/tests/shapes/rect/simple-case.png");
        let reference = image::load_from_memory(png)
            .expect("reference PNG should decode")
            .to_rgba8();

        let fb = render_svg_to_framebuffer_at_size(svg, reference.width(), reference.height())
            .expect("SVG should render at reference size");

        assert_eq!(fb.width(), reference.width());
        assert_eq!(fb.height(), reference.height());

        let center = ((250 * fb.width() + 250) * 4) as usize;
        assert_eq!(
            &fb.pixels()[center..center + 4],
            &reference.as_raw()[center..center + 4],
            "simple resvg-suite center pixel should match the reference PNG"
        );
    }

    #[test]
    fn renders_to_lcd_buffer_via_library_target_helper() {
        let svg = br##"
            <svg xmlns="http://www.w3.org/2000/svg" width="2" height="2">
                <rect width="2" height="2" fill="#ff0000"/>
            </svg>
        "##;

        let buffer = render_svg_to_lcd_buffer(svg).expect("SVG should render");
        assert_eq!(buffer.width(), 2);
        assert_eq!(buffer.height(), 2);
        assert!(buffer
            .color_plane()
            .chunks_exact(3)
            .any(|px| px == [255, 0, 0]));
        assert!(buffer.alpha_plane().iter().any(|&alpha| alpha > 0));
    }

    #[test]
    fn honors_even_odd_fill_rule() {
        let svg = br##"
            <svg xmlns="http://www.w3.org/2000/svg" width="5" height="5">
                <path fill="#ff0000" fill-rule="evenodd"
                      d="M0 0 H5 V5 H0 Z M1 1 H4 V4 H1 Z"/>
            </svg>
        "##;

        let mut fb = Framebuffer::new(5, 5);
        {
            let mut ctx = GfxCtx::new(&mut fb);
            render_svg(svg, &mut ctx).expect("SVG should render");
        }

        let center = ((2 * fb.width() + 2) * 4) as usize;
        assert_eq!(&fb.pixels()[center..center + 4], &[0, 0, 0, 0]);
    }

    #[test]
    fn multiplies_group_and_paint_opacity() {
        let svg = br##"
            <svg xmlns="http://www.w3.org/2000/svg" width="3" height="3">
                <g opacity="0.5">
                    <rect x="0" y="0" width="3" height="3" fill="#ff0000" fill-opacity="0.5"/>
                </g>
            </svg>
        "##;

        let mut fb = Framebuffer::new(3, 3);
        {
            let mut ctx = GfxCtx::new(&mut fb);
            render_svg(svg, &mut ctx).expect("SVG should render");
        }

        let center = ((1 * fb.width() + 1) * 4) as usize;
        assert_eq!(&fb.pixels()[center..center + 4], &[63, 0, 0, 63]);
    }

    #[test]
    fn renders_embedded_png_image_via_draw_ctx() {
        let mut png = Vec::new();
        let img = image::RgbaImage::from_pixel(1, 1, image::Rgba([0, 255, 0, 255]));
        image::DynamicImage::ImageRgba8(img)
            .write_to(
                &mut std::io::Cursor::new(&mut png),
                image::ImageOutputFormat::Png,
            )
            .expect("test PNG should encode");
        let encoded = base64::engine::general_purpose::STANDARD.encode(png);
        let svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg"
                    xmlns:xlink="http://www.w3.org/1999/xlink"
                    width="3" height="3">
                   <image width="3" height="3" xlink:href="data:image/png;base64,{encoded}"/>
               </svg>"#
        );

        let mut fb = Framebuffer::new(3, 3);
        {
            let mut ctx = GfxCtx::new(&mut fb);
            render_svg(svg.as_bytes(), &mut ctx).expect("SVG should render");
        }

        assert!(
            fb.pixels().chunks_exact(4).any(|px| px == [0, 255, 0, 255]),
            "embedded image should paint at least one green pixel"
        );
    }

    #[test]
    fn applies_svg_node_transforms() {
        let svg = br##"
            <svg xmlns="http://www.w3.org/2000/svg" width="4" height="4">
                <g transform="translate(1 1)">
                    <rect x="0" y="0" width="1" height="1" fill="#0000ff"/>
                </g>
            </svg>
        "##;

        let mut fb = Framebuffer::new(4, 4);
        {
            let mut ctx = GfxCtx::new(&mut fb);
            render_svg(svg, &mut ctx).expect("SVG should render");
        }

        let translated = ((2 * fb.width() + 1) * 4) as usize;
        assert_eq!(&fb.pixels()[translated..translated + 4], &[0, 0, 255, 255]);
    }

    #[test]
    fn svg_coordinates_are_y_down_in_visual_space() {
        let svg = br##"
            <svg xmlns="http://www.w3.org/2000/svg" width="6" height="6">
                <rect x="1" y="1" width="1" height="1" fill="#ff0000"/>
                <rect x="4" y="4" width="1" height="1" fill="#0000ff"/>
            </svg>
        "##;

        let fb = render_svg_to_framebuffer(svg).expect("SVG should render");

        let top_left_svg_pixel = ((4 * fb.width() + 1) * 4) as usize;
        assert_eq!(
            &fb.pixels()[top_left_svg_pixel..top_left_svg_pixel + 4],
            &[255, 0, 0, 255],
            "SVG y=1 should land one pixel below the visual top"
        );

        let bottom_right_svg_pixel = ((1 * fb.width() + 4) * 4) as usize;
        assert_eq!(
            &fb.pixels()[bottom_right_svg_pixel..bottom_right_svg_pixel + 4],
            &[0, 0, 255, 255],
            "SVG y=4 should land one pixel above the visual bottom"
        );
    }

    #[test]
    fn applies_stroke_dash_array() {
        let solid = br##"
            <svg xmlns="http://www.w3.org/2000/svg" width="12" height="3">
                <path d="M0 1.5 H12" stroke="#000000" stroke-width="1" fill="none"/>
            </svg>
        "##;
        let dashed = br##"
            <svg xmlns="http://www.w3.org/2000/svg" width="12" height="3">
                <path d="M0 1.5 H12" stroke="#000000" stroke-width="1"
                      stroke-dasharray="2 2" fill="none"/>
            </svg>
        "##;

        let painted_pixels = |svg: &[u8]| -> usize {
            let mut fb = Framebuffer::new(12, 3);
            {
                let mut ctx = GfxCtx::new(&mut fb);
                render_svg(svg, &mut ctx).expect("SVG should render");
            }
            fb.pixels().chunks_exact(4).filter(|px| px[3] > 0).count()
        };

        let solid_count = painted_pixels(solid);
        let dashed_count = painted_pixels(dashed);
        assert!(dashed_count > 0, "dashed stroke should paint some pixels");
        assert!(
            dashed_count < solid_count,
            "dashed stroke should paint fewer pixels than a solid stroke"
        );
    }
}
