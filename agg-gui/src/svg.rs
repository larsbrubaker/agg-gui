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
use crate::framebuffer::Framebuffer;
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
    render_tree::render_group(
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
    render_tree::render_group(
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
mod render_tree;
use render_tree::render_group;

#[cfg(test)]
mod core_tests;
