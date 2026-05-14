//! SVG rendering support for `agg-gui`.
//!
//! This module is the library-owned SVG renderer used by tests, demos, and
//! applications.  It parses SVG with `usvg`, then emits drawing commands only
//! through [`crate::draw_ctx::DrawCtx`] so RGBA software, LCD coverage, and
//! hardware targets all share one render path.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, RwLock};

use agg_rust::math_stroke::{LineCap, LineJoin};
use agg_rust::trans_affine::TransAffine;
use usvg::tiny_skia_path::PathSegment;

use crate::draw_ctx::{DrawCtx, FillRule};
use crate::framebuffer::{unpremultiply_rgba_inplace, Framebuffer};
use crate::gfx_ctx::GfxCtx;
use crate::lcd_coverage::LcdBuffer;
use crate::lcd_gfx_ctx::LcdGfxCtx;

pub use compare::{
    compare_svg_rgba, SvgCompareResult, SvgCompareThresholds, DEFAULT_ALPHA_TOLERANCE,
    DEFAULT_MISMATCH_RATIO, DEFAULT_OPAQUE_RGB_TOLERANCE, DEFAULT_TRANSLUCENT_RGB_TOLERANCE,
    DEFAULT_VISUAL_RGB_TOLERANCE,
};

pub type SvgTree = usvg::Tree;

#[derive(Clone, Copy, Debug)]
struct SvgRenderState {
    opacity: f32,
    layer_width: f64,
    layer_height: f64,
    source_cull: Option<SvgSourceRect>,
}

impl Default for SvgRenderState {
    fn default() -> Self {
        Self {
            opacity: 1.0,
            layer_width: 1.0,
            layer_height: 1.0,
            source_cull: None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct SvgSourceRect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

impl SvgSourceRect {
    fn intersects(self, rect: SvgSourceRect) -> bool {
        let ax0 = self.x;
        let ay0 = self.y;
        let ax1 = self.x + self.w;
        let ay1 = self.y + self.h;
        let bx0 = rect.x;
        let by0 = rect.y;
        let bx1 = rect.x + rect.w;
        let by1 = rect.y + rect.h;
        ax0 < bx1 && ax1 > bx0 && ay0 < by1 && ay1 > by0
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

/// Options used while parsing SVG documents.
///
/// `agg-gui` keeps SVG rendering in the core library, but font selection is
/// intentionally application-owned. Callers that need SVG text should provide a
/// `fontdb` built from their own assets.
#[derive(Clone, Default)]
pub struct SvgParseOptions {
    resources_dir: Option<PathBuf>,
    font_family: Option<String>,
    fontdb: Option<Arc<usvg::fontdb::Database>>,
}

impl SvgParseOptions {
    pub fn new() -> Self {
        Self::default()
    }

    /// Resolve relative image references from `resources_dir`.
    pub fn with_resources_dir(mut self, resources_dir: impl Into<PathBuf>) -> Self {
        self.resources_dir = Some(resources_dir.into());
        self
    }

    /// Set the preferred SVG text family for documents that omit one.
    pub fn with_font_family(mut self, family: impl Into<String>) -> Self {
        self.font_family = Some(family.into());
        self
    }

    /// Provide a prepared font database for SVG text parsing.
    pub fn with_fontdb(mut self, fontdb: Arc<usvg::fontdb::Database>) -> Self {
        self.fontdb = Some(fontdb);
        self
    }
}

static DEFAULT_SVG_PARSE_OPTIONS: OnceLock<RwLock<SvgParseOptions>> = OnceLock::new();

fn default_svg_parse_options_cell() -> &'static RwLock<SvgParseOptions> {
    DEFAULT_SVG_PARSE_OPTIONS.get_or_init(|| RwLock::new(system_svg_parse_options()))
}

fn system_svg_parse_options() -> SvgParseOptions {
    let mut fontdb = usvg::fontdb::Database::new();
    fontdb.load_system_fonts();
    font_defaults::configure_generic_font_families(&mut fontdb, None);
    SvgParseOptions::new().with_fontdb(Arc::new(fontdb))
}

/// Replace the default SVG parse options used by convenience render helpers.
///
/// This keeps SVG rendering in the core library while letting applications own
/// the font database used for SVG text.
pub fn set_default_svg_parse_options(options: SvgParseOptions) {
    *default_svg_parse_options_cell()
        .write()
        .expect("default SVG parse options lock poisoned") = options;
}

/// Build a `usvg` font database from caller-owned font bytes.
pub fn svg_fontdb_from_font_data<I>(
    fonts: I,
    generic_family: Option<&str>,
) -> Arc<usvg::fontdb::Database>
where
    I: IntoIterator<Item = Vec<u8>>,
{
    let mut fontdb = usvg::fontdb::Database::new();
    for bytes in fonts {
        fontdb.load_font_data(bytes);
    }
    font_defaults::configure_generic_font_families(&mut fontdb, generic_family);
    Arc::new(fontdb)
}

fn parse_svg_tree(data: &[u8], resources_dir: Option<&Path>) -> Result<usvg::Tree, SvgRenderError> {
    let mut options = default_svg_parse_options_cell()
        .read()
        .expect("default SVG parse options lock poisoned")
        .clone();
    if let Some(dir) = resources_dir {
        options = options.with_resources_dir(dir);
    }
    parse_svg(data, &options)
}

/// Parse an SVG document using caller-supplied parse options.
pub fn parse_svg(data: &[u8], svg_options: &SvgParseOptions) -> Result<usvg::Tree, SvgRenderError> {
    let mut options = usvg::Options::default();
    options.resources_dir = svg_options.resources_dir.clone();
    if let Some(font_family) = &svg_options.font_family {
        options.font_family = font_family.clone();
    }
    if let Some(fontdb) = &svg_options.fontdb {
        options.fontdb = Arc::clone(fontdb);
    }
    Ok(usvg::Tree::from_data(data, &options)?)
}

/// Parse an SVG document and render it into `ctx`.
///
/// This is a convenience wrapper around [`render_svg_tree`].  Callers that
/// already cache a `usvg::Tree` should use [`render_svg_tree`] directly.
pub fn render_svg(data: &[u8], ctx: &mut dyn DrawCtx) -> Result<(), SvgRenderError> {
    let tree = parse_svg_tree(data, None)?;
    render_svg_tree(&tree, ctx)
}

/// Parse an SVG document with explicit options and render it into `ctx`.
pub fn render_svg_with_options(
    data: &[u8],
    ctx: &mut dyn DrawCtx,
    options: &SvgParseOptions,
) -> Result<(), SvgRenderError> {
    let tree = parse_svg(data, options)?;
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
    let tree = parse_svg_tree(data, None)?;
    render_svg_tree_at_size(&tree, ctx, width, height)
}

pub fn render_svg_at_size_with_options(
    data: &[u8],
    ctx: &mut dyn DrawCtx,
    width: u32,
    height: u32,
    options: &SvgParseOptions,
) -> Result<(), SvgRenderError> {
    let tree = parse_svg(data, options)?;
    render_svg_tree_at_size(&tree, ctx, width, height)
}

pub fn render_svg_at_size_with_resources(
    data: &[u8],
    ctx: &mut dyn DrawCtx,
    width: u32,
    height: u32,
    resources_dir: &Path,
) -> Result<(), SvgRenderError> {
    let tree = parse_svg_tree(data, Some(resources_dir))?;
    render_svg_tree_at_size(&tree, ctx, width, height)
}

/// Parse an SVG document and render it into a newly allocated RGBA framebuffer.
///
/// This is the library API the SVG regression tests and demo viewer should use
/// for the `agg-rgba-bitmap render` column.
pub fn render_svg_to_framebuffer(data: &[u8]) -> Result<Framebuffer, SvgRenderError> {
    let tree = parse_svg_tree(data, None)?;
    render_svg_tree_to_framebuffer(&tree)
}

pub fn render_svg_to_framebuffer_with_options(
    data: &[u8],
    options: &SvgParseOptions,
) -> Result<Framebuffer, SvgRenderError> {
    let tree = parse_svg(data, options)?;
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
    let tree = parse_svg_tree(data, None)?;
    render_svg_tree_to_framebuffer_at_size(&tree, width, height)
}

pub fn render_svg_to_framebuffer_at_size_with_options(
    data: &[u8],
    width: u32,
    height: u32,
    options: &SvgParseOptions,
) -> Result<Framebuffer, SvgRenderError> {
    let tree = parse_svg(data, options)?;
    render_svg_tree_to_framebuffer_at_size(&tree, width, height)
}

pub fn render_svg_to_framebuffer_at_size_with_resources(
    data: &[u8],
    width: u32,
    height: u32,
    resources_dir: &Path,
) -> Result<Framebuffer, SvgRenderError> {
    let tree = parse_svg_tree(data, Some(resources_dir))?;
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

/// Render a rectangular region of a parsed SVG tree into a newly allocated RGBA framebuffer.
pub fn render_svg_tree_region_to_framebuffer_at_size(
    tree: &usvg::Tree,
    src_x: f64,
    src_y: f64,
    src_w: f64,
    src_h: f64,
    width: u32,
    height: u32,
) -> Result<Framebuffer, SvgRenderError> {
    let width = width.max(1);
    let height = height.max(1);
    let mut fb = Framebuffer::new(width, height);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        render_svg_tree_region_at_size(tree, &mut ctx, src_x, src_y, src_w, src_h, width, height)?;
    }
    Ok(fb)
}

/// Parse an SVG document and render it into a newly allocated LCD coverage buffer.
///
/// This is the library API the SVG regression tests and demo viewer should use
/// for the `agg-lcd-bitmap render` column.
pub fn render_svg_to_lcd_buffer(data: &[u8]) -> Result<LcdBuffer, SvgRenderError> {
    let tree = parse_svg_tree(data, None)?;
    render_svg_tree_to_lcd_buffer(&tree)
}

pub fn render_svg_to_lcd_buffer_with_options(
    data: &[u8],
    options: &SvgParseOptions,
) -> Result<LcdBuffer, SvgRenderError> {
    let tree = parse_svg(data, options)?;
    render_svg_tree_to_lcd_buffer(&tree)
}

/// Parse an SVG document and render it into an LCD coverage buffer with an
/// explicit pixel size.
pub fn render_svg_to_lcd_buffer_at_size(
    data: &[u8],
    width: u32,
    height: u32,
) -> Result<LcdBuffer, SvgRenderError> {
    let tree = parse_svg_tree(data, None)?;
    render_svg_tree_to_lcd_buffer_at_size(&tree, width, height)
}

pub fn render_svg_to_lcd_buffer_at_size_with_options(
    data: &[u8],
    width: u32,
    height: u32,
    options: &SvgParseOptions,
) -> Result<LcdBuffer, SvgRenderError> {
    let tree = parse_svg(data, options)?;
    render_svg_tree_to_lcd_buffer_at_size(&tree, width, height)
}

pub fn render_svg_to_lcd_buffer_at_size_with_resources(
    data: &[u8],
    width: u32,
    height: u32,
    resources_dir: &Path,
) -> Result<LcdBuffer, SvgRenderError> {
    let tree = parse_svg_tree(data, Some(resources_dir))?;
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
    render_group(
        tree.root(),
        ctx,
        SvgRenderState {
            layer_width: width.max(1) as f64,
            layer_height: height.max(1) as f64,
            ..SvgRenderState::default()
        },
    )?;
    ctx.restore();
    Ok(())
}

/// Render a parsed `usvg::Tree` region into `ctx`, mapping the SVG source
/// rectangle `(src_x, src_y, src_w, src_h)` to the output viewport.
#[allow(clippy::too_many_arguments)]
pub fn render_svg_tree_region_at_size(
    tree: &usvg::Tree,
    ctx: &mut dyn DrawCtx,
    src_x: f64,
    src_y: f64,
    src_w: f64,
    src_h: f64,
    width: u32,
    height: u32,
) -> Result<(), SvgRenderError> {
    let width = width.max(1);
    let height = height.max(1);
    let src_w = src_w.max(1.0);
    let src_h = src_h.max(1.0);
    let saved_transform = ctx.transform();
    let mut svg_to_ctx = saved_transform;
    svg_to_ctx.premultiply(&svg_y_down_region_to_ctx_y_up(
        src_x, src_y, src_w, src_h, width, height,
    ));

    ctx.save();
    ctx.set_transform(svg_to_ctx);
    render_group(
        tree.root(),
        ctx,
        SvgRenderState {
            layer_width: width as f64,
            layer_height: height as f64,
            source_cull: Some(SvgSourceRect {
                x: src_x,
                y: src_y,
                w: src_w,
                h: src_h,
            }),
            ..SvgRenderState::default()
        },
    )?;
    ctx.restore();
    Ok(())
}

fn svg_y_down_to_ctx_y_up(tree: &usvg::Tree, width: u32, height: u32) -> TransAffine {
    let sx = width.max(1) as f64 / tree.size().width().max(1.0) as f64;
    let sy = height.max(1) as f64 / tree.size().height().max(1.0) as f64;
    TransAffine::new_custom(sx, 0.0, 0.0, -sy, 0.0, height.max(1) as f64)
}

fn svg_y_down_region_to_ctx_y_up(
    src_x: f64,
    src_y: f64,
    src_w: f64,
    src_h: f64,
    width: u32,
    height: u32,
) -> TransAffine {
    let sx = width as f64 / src_w;
    let sy = height as f64 / src_h;
    TransAffine::new_custom(sx, 0.0, 0.0, -sy, -src_x * sx, height as f64 + src_y * sy)
}

fn render_group(
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
        if let Some(pattern) = pattern::render_pattern_paint(pattern, opacity, object_bbox) {
            ctx.set_fill_pattern(pattern);
            return true;
        }
        return false;
    }

    paint::apply_fill_paint(ctx, paint, opacity)
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
        if let Some(pattern) = pattern::render_pattern_paint(pattern, opacity, object_bbox) {
            ctx.set_stroke_pattern(pattern);
            return true;
        }
        return false;
    }

    paint::apply_stroke_paint(ctx, paint, opacity)
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

fn apply_transform(ctx: &mut dyn DrawCtx, transform: usvg::Transform) {
    let mut current = ctx.transform();
    let node_transform = to_trans_affine(transform);
    current.premultiply(&node_transform);
    ctx.set_transform(current);
}

pub(super) fn to_trans_affine(transform: usvg::Transform) -> TransAffine {
    TransAffine::new_custom(
        transform.sx as f64,
        transform.ky as f64,
        transform.kx as f64,
        transform.sy as f64,
        transform.tx as f64,
        transform.ty as f64,
    )
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

fn transformed_svg_rect(rect: usvg::Rect, transform: usvg::Transform) -> SvgSourceRect {
    let transform = to_trans_affine(transform);
    let x = rect.x() as f64;
    let y = rect.y() as f64;
    let w = rect.width() as f64;
    let h = rect.height() as f64;
    let corners = [(x, y), (x + w, y), (x + w, y + h), (x, y + h)];
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
    SvgSourceRect {
        x: min_x,
        y: min_y,
        w: (max_x - min_x).abs(),
        h: (max_y - min_y).abs(),
    }
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
mod clip_tests;
pub mod compare;
#[cfg(test)]
mod gradient_tests;
#[cfg(test)]
mod image_tests;
#[cfg(test)]
mod opacity_tests;
#[cfg(test)]
mod text_tests;

mod font_defaults;
mod paint;
mod pattern;

#[cfg(test)]
mod core_tests;
