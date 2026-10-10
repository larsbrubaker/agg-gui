//! `IconImage` — an image icon for widgets whose icon slot otherwise holds a
//! Font Awesome glyph (`MenuItem`, `Button`, `TreeView` nodes).
//!
//! Font Awesome glyphs remain the default icon in agg-gui.  Applications that
//! port artwork-based UIs (MatterCAD's SVG/PNG icon set) need the same slots
//! to show images, so each of those widgets carries an optional `IconImage`
//! that takes precedence over its glyph.
//!
//! An `IconImage` has a **logical** size (layout units, like widget bounds)
//! and one of two sources:
//!
//! - **SVG** ([`IconImage::from_svg_tree`] / [`IconImage::from_svg_data`]):
//!   rasterised on demand at `logical size × device_scale()` physical pixels,
//!   so the icon is crisp at any device scale.  Rasters are cached per
//!   physical size (a few sizes at once, so one icon shown at two sizes does
//!   not thrash) and re-made only when the scale or draw size changes.
//! - **RGBA** ([`IconImage::from_rgba`]): pre-rendered pixels the caller
//!   supplies, drawn scaled into the logical rect.  Supply pixels at the
//!   physical size you expect for crisp output.
//!
//! Pixel convention: like every `DrawCtx::draw_image_rgba*` input, the RGBA
//! is **straight (non-premultiplied) alpha, top row first**.  An optional
//! pixel filter ([`IconImage::with_pixel_filter`]) runs over each fresh
//! raster in that convention — the place for MatterCAD-style tinting or
//! lightness inversion, which operate on straight alpha.
//!
//! Drawing goes through [`DrawCtx::draw_image_rgba_arc`], so GPU backends key
//! their texture cache on the cached raster's `Arc` and re-upload only when
//! the raster changes.

use std::fmt;
use std::sync::{Arc, Mutex};

use crate::draw_ctx::DrawCtx;
use crate::framebuffer::unpremultiply_rgba_inplace;
use crate::geometry::{Rect, Size};
use crate::svg::{render_svg_tree_to_framebuffer_at_size, SvgParseOptions, SvgRenderError};

/// Post-rasterisation filter over straight-alpha, top-row-first RGBA8.
pub type IconPixelFilter = Arc<dyn Fn(&mut [u8]) + Send + Sync>;

#[derive(Clone)]
enum IconSource {
    Rgba {
        data: Arc<Vec<u8>>,
        width: u32,
        height: u32,
    },
    Svg(Arc<usvg::Tree>),
}

/// One rasterised copy of the icon, straight alpha, top row first.
#[derive(Clone)]
struct IconRaster {
    data: Arc<Vec<u8>>,
    width: u32,
    height: u32,
}

/// An image icon with a logical size; see the module docs.
///
/// Cloning is cheap (shared source and shared raster cache).  Two
/// `IconImage`s compare equal when they are clones of the same image.
#[derive(Clone)]
pub struct IconImage {
    // One pointer, so `Option<IconImage>` keeps `MenuItem` and friends small.
    inner: Arc<IconImageInner>,
}

struct IconImageInner {
    source: IconSource,
    size: Size,
    filter: Option<IconPixelFilter>,
    cache: Mutex<Vec<IconRaster>>,
}

/// Physical sizes kept per icon.  Most icons are drawn at one size; a few
/// appear at two (menu + toolbar) or across a scale change.
const MAX_CACHED_RASTERS: usize = 4;

impl IconImage {
    /// An icon from straight-alpha RGBA8 pixels (`width × height × 4` bytes,
    /// top row first) drawn at `logical_size`.  Returns `None` when the
    /// buffer length does not match the dimensions or a dimension is zero.
    pub fn from_rgba(
        data: Arc<Vec<u8>>,
        width: u32,
        height: u32,
        logical_size: Size,
    ) -> Option<Self> {
        let expected = (width as usize)
            .checked_mul(height as usize)?
            .checked_mul(4)?;
        if width == 0 || height == 0 || data.len() != expected {
            return None;
        }
        Some(Self::new(
            IconSource::Rgba {
                data,
                width,
                height,
            },
            logical_size,
        ))
    }

    /// An icon from a parsed SVG tree, rasterised at physical resolution
    /// whenever it is drawn.
    pub fn from_svg_tree(tree: Arc<usvg::Tree>, logical_size: Size) -> Self {
        Self::new(IconSource::Svg(tree), logical_size)
    }

    /// Parse SVG source (default parse options) into an icon.
    pub fn from_svg_data(data: &[u8], logical_size: Size) -> Result<Self, SvgRenderError> {
        let tree = crate::svg::parse_svg(data, &SvgParseOptions::new())?;
        Ok(Self::from_svg_tree(Arc::new(tree), logical_size))
    }

    fn new(source: IconSource, size: Size) -> Self {
        Self {
            inner: Arc::new(IconImageInner {
                source,
                size,
                filter: None,
                cache: Mutex::new(Vec::new()),
            }),
        }
    }

    /// Run `filter` over every raster this icon produces (straight alpha,
    /// top row first).  Returns a distinct icon with its own cache.
    pub fn with_pixel_filter(self, filter: IconPixelFilter) -> Self {
        Self {
            inner: Arc::new(IconImageInner {
                source: self.inner.source.clone(),
                size: self.inner.size,
                filter: Some(filter),
                cache: Mutex::new(Vec::new()),
            }),
        }
    }

    /// Logical (layout-unit) size the icon is drawn at.
    pub fn size(&self) -> Size {
        self.inner.size
    }

    /// The pixels drawn at device scale `scale` at the icon's logical size:
    /// `(rgba, width, height)`, straight alpha, top row first.  SVG icons are
    /// rasterised at `ceil(logical size × scale)`; RGBA icons return their own
    /// pixels.
    pub fn raster_at_scale(&self, scale: f64) -> Option<(Arc<Vec<u8>>, u32, u32)> {
        self.raster_for(self.inner.size, scale)
    }

    fn raster_for(&self, logical: Size, scale: f64) -> Option<(Arc<Vec<u8>>, u32, u32)> {
        let (want_w, want_h) = match &self.inner.source {
            IconSource::Rgba { width, height, .. } => (*width, *height),
            IconSource::Svg(_) => {
                let scale = if scale.is_finite() && scale > 0.0 {
                    scale
                } else {
                    1.0
                };
                (
                    (logical.width * scale).ceil().max(1.0) as u32,
                    (logical.height * scale).ceil().max(1.0) as u32,
                )
            }
        };
        let mut cache = self.inner.cache.lock().ok()?;
        if let Some(pos) = cache
            .iter()
            .position(|r| r.width == want_w && r.height == want_h)
        {
            // Most recently used goes last.
            let r = cache.remove(pos);
            let out = (Arc::clone(&r.data), r.width, r.height);
            cache.push(r);
            return Some(out);
        }
        let raster = self.rasterise(want_w, want_h)?;
        let out = (Arc::clone(&raster.data), raster.width, raster.height);
        if cache.len() >= MAX_CACHED_RASTERS {
            cache.remove(0);
        }
        cache.push(raster);
        Some(out)
    }

    fn rasterise(&self, width: u32, height: u32) -> Option<IconRaster> {
        let mut pixels = match &self.inner.source {
            IconSource::Rgba { data, .. } => {
                if self.inner.filter.is_none() {
                    return Some(IconRaster {
                        data: Arc::clone(data),
                        width,
                        height,
                    });
                }
                data.as_ref().clone()
            }
            IconSource::Svg(tree) => {
                let fb = render_svg_tree_to_framebuffer_at_size(tree, width, height).ok()?;
                let mut pixels = fb.pixels_flipped();
                unpremultiply_rgba_inplace(&mut pixels);
                pixels
            }
        };
        if let Some(filter) = &self.inner.filter {
            filter(&mut pixels);
        }
        Some(IconRaster {
            data: Arc::new(pixels),
            width,
            height,
        })
    }

    /// Draw the icon at its logical size with its bottom-left corner at
    /// `(x, y)` (Y-up local coordinates), rasterised for the current
    /// `device_scale()`.
    pub fn draw(&self, ctx: &mut dyn DrawCtx, x: f64, y: f64) {
        self.draw_in(
            ctx,
            Rect::new(x, y, self.inner.size.width, self.inner.size.height),
        );
    }

    /// Draw the icon scaled into `rect` (Y-up local coordinates).  SVG icons
    /// are rasterised for `rect`'s size at the current `device_scale()`.
    pub fn draw_in(&self, ctx: &mut dyn DrawCtx, rect: Rect) {
        if rect.width <= 0.0 || rect.height <= 0.0 {
            return;
        }
        let scale = crate::device_scale::device_scale();
        if let Some((data, w, h)) = self.raster_for(Size::new(rect.width, rect.height), scale) {
            ctx.draw_image_rgba_arc(&data, w, h, rect.x, rect.y, rect.width, rect.height);
        }
    }
}

impl IconImage {
    /// Identity shared by clones of this image (and by nothing else).  Lets
    /// widgets fold "which image" into a content signature without hashing
    /// pixels.
    pub fn identity(&self) -> usize {
        Arc::as_ptr(&self.inner) as *const () as usize
    }
}

#[cfg(test)]
impl IconImage {
    /// Physical sizes currently cached, oldest first.
    pub(crate) fn cached_raster_sizes(&self) -> Vec<(u32, u32)> {
        self.inner
            .cache
            .lock()
            .map(|c| c.iter().map(|r| (r.width, r.height)).collect())
            .unwrap_or_default()
    }
}

impl PartialEq for IconImage {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl fmt::Debug for IconImage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let source = match &self.inner.source {
            IconSource::Rgba { width, height, .. } => format!("Rgba({width}x{height})"),
            IconSource::Svg(_) => "Svg".to_string(),
        };
        f.debug_struct("IconImage")
            .field("source", &source)
            .field("size", &(self.inner.size.width, self.inner.size.height))
            .field("filtered", &self.inner.filter.is_some())
            .finish()
    }
}

#[cfg(test)]
#[path = "icon_image_tests.rs"]
mod tests;
