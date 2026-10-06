//! The labelled face images the tumble cube is textured with, and the
//! per-instance hover-highlight copies drawn over them.
//!
//! Port of MatterCAD's `TumbleCubeFaceTextures` (the shared, cached source
//! images) plus the `TextureData` / `DrawMouseHover` / `ResetTextures`
//! half of `TumbleCubeControl` (the per-instance `active` copies).
//!
//! * Source images are rasterized once per `(background, text, border,
//!   label)` and shared by every cube on the thread (C# shares them
//!   process wide under a lock; agg-gui's `Font` and widget tree are
//!   UI-thread objects, so a thread-local cache is the equivalent).
//! * The cache is dropped whenever the thread's typography epoch moves — the
//!   analogue of the C# `LcdRenderSettings.Epoch` check — because a label
//!   rasterized under the old font / LCD settings must not outlive them.
//! * Hover highlighting only ever draws into a per-instance *copy*
//!   ([`CubeFaces`]), so one cube's hover can never light up another's.
//!
//! Pixel format everywhere in this module: `FACE_SIZE²` straight-alpha
//! RGBA8, **top row first** — what `DrawCtx::draw_image_rgba*` and a wgpu
//! texture upload both take.  Tile rectangles are in the C#'s Y-up image
//! space and converted on write.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use crate::color::Color;
use crate::framebuffer::{unpremultiply_rgba_inplace, Framebuffer};
use crate::gfx_ctx::GfxCtx;
use crate::text::{measure_text_metrics, Font};

use super::hit_test::{HitData, FACE_NAMES};

/// Face images are square and this many pixels on a side (C# `FaceSize`).
pub const FACE_SIZE: u32 = 256;

/// Label size, as the C# `DrawString(..., 60, ...)`.
const LABEL_SIZE: f64 = 60.0;
/// Width of the outline stroke (C# `new Stroke(..., 6)`).
const BORDER_WIDTH: f64 = 6.0;

type CacheKey = ([u8; 4], [u8; 4], [u8; 4], String);
/// `(epoch the entries were drawn at, entries)`.
type Cache = (u64, HashMap<CacheKey, Arc<Vec<u8>>>);

thread_local! {
    static CACHE: RefCell<Cache> = RefCell::new((0, HashMap::new()));
    /// Bundled fallback for when the app has not installed a system font,
    /// so the faces are always labelled (the C# always has a font).
    static FALLBACK_FONT: Arc<Font> = Arc::new(crate::fonts::standard_ui_font());
}

fn color_key(c: Color) -> [u8; 4] {
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    [q(c.r), q(c.g), q(c.b), q(c.a)]
}

/// The shared, immutable source image for one labelled face — C#
/// `TumbleCubeFaceTextures.GetFaceImage`.  Repeated calls with the same
/// colours and label return the *same* `Arc` (test with `Arc::ptr_eq`);
/// never mutate it — copy it for anything that draws over it.
pub fn get_face_image(
    background: Color,
    text: Color,
    border: Color,
    face_name: &str,
) -> Arc<Vec<u8>> {
    let epoch = crate::font_settings::current_thread_typography_epoch();
    let key = (
        color_key(background),
        color_key(text),
        color_key(border),
        face_name.to_string(),
    );
    if let Some(hit) = CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if c.0 != epoch {
            c.1.clear();
            c.0 = epoch;
        }
        c.1.get(&key).cloned()
    }) {
        return hit;
    }
    // Rasterize outside the borrow: text rendering may consult other
    // thread-locals and must not re-enter this RefCell.
    let image = Arc::new(render_face_image(background, text, border, face_name));
    CACHE.with(|c| c.borrow_mut().1.insert(key, image.clone()));
    image
}

/// C# `RenderFaceImage`: clear to the background, centre the label at
/// 60 px, outline the square with a 6 px stroke.
fn render_face_image(background: Color, text: Color, border: Color, face_name: &str) -> Vec<u8> {
    let size = FACE_SIZE as f64;
    let mut fb = Framebuffer::new(FACE_SIZE, FACE_SIZE);
    {
        let mut g = GfxCtx::new(&mut fb);
        g.clear(background);

        let font = crate::font_settings::current_system_font()
            .unwrap_or_else(|| FALLBACK_FONT.with(|f| f.clone()));
        let m = measure_text_metrics(&font, face_name, LABEL_SIZE);
        g.set_font(font);
        g.set_font_size(LABEL_SIZE);
        g.set_fill_color(text);
        // C# centres the string's bounds (`Justification.Center`,
        // `Baseline.BoundsCenter`); the ascent/descent midpoint is the
        // closest agg-gui metric to the ink bounds.
        let baseline = size * 0.5 - (m.ascent - m.descent) * 0.5;
        g.fill_text(face_name, size * 0.5 - m.width * 0.5, baseline);

        // `RoundedRect(.5, .5, FaceSize - 1.5, FaceSize - 1.6, 0)` is
        // (left, bottom, right, top); agg-gui's rect takes (x, y, w, h).
        g.set_stroke_color(border);
        g.set_line_width(BORDER_WIDTH);
        g.begin_path();
        g.rect(0.5, 0.5, size - 2.0, size - 2.1);
        g.stroke();
    }
    let mut pixels = fb.pixels_flipped();
    unpremultiply_rgba_inplace(&mut pixels);
    pixels
}

/// RGBA of pixel `(x, y)` with `y` counted **up** from the bottom row, the
/// way the C# `ImageBuffer.GetPixel` addresses it.
pub fn face_pixel(image: &[u8], x: u32, y: u32) -> [u8; 4] {
    let row = FACE_SIZE - 1 - y;
    let i = ((row * FACE_SIZE + x) * 4) as usize;
    [image[i], image[i + 1], image[i + 2], image[i + 3]]
}

/// `(left, bottom, right, top)` of `tile` in Y-up pixels — the rectangles
/// `DrawMouseHover`'s `FillRectangle` switch uses (quarter-width borders,
/// half-width middles).
pub fn tile_rect(tile: i32) -> Option<(u32, u32, u32, u32)> {
    let q = FACE_SIZE / 4;
    let s = FACE_SIZE;
    Some(match tile {
        0 => (0, 0, q, q),
        1 => (q, 0, q * 3, q),
        2 => (q * 3, 0, s, q),
        3 => (0, q, q, q * 3),
        4 => (q, q, q * 3, q * 3),
        5 => (q * 3, q, s, q * 3),
        6 => (0, q * 3, q, s),
        7 => (q, q * 3, q * 3, s),
        8 => (q * 3, q * 3, s, s),
        _ => return None,
    })
}

/// Straight-alpha source-over of `color` across `tile` of `image`.
fn fill_tile(image: &mut [u8], tile: i32, color: Color) {
    let Some((x0, y0, x1, y1)) = tile_rect(tile) else {
        return;
    };
    let sa = color.a.clamp(0.0, 1.0);
    let src = [color.r, color.g, color.b];
    for y_up in y0..y1 {
        let row = FACE_SIZE - 1 - y_up;
        for x in x0..x1 {
            let i = ((row * FACE_SIZE + x) * 4) as usize;
            let da = image[i + 3] as f32 / 255.0;
            let oa = sa + da * (1.0 - sa);
            for c in 0..3 {
                let d = image[i + c] as f32 / 255.0;
                let o = if oa > 0.0 {
                    (src[c] * sa + d * da * (1.0 - sa)) / oa
                } else {
                    0.0
                };
                image[i + c] = (o.clamp(0.0, 1.0) * 255.0).round() as u8;
            }
            image[i + 3] = (oa * 255.0).round() as u8;
        }
    }
}

/// Colours the faces are drawn with.  Defaults are MatterCAD's Light-White
/// theme (`BedColor` `#f1f1f1`, `TextColor` `#333`, `BedGridColors.Line`
/// `#ccc`); `hover: None` means "the theme accent at alpha 128", which is
/// how the C# `AccentMimimalOverlay` is defined.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TumbleCubeStyle {
    pub background: Color,
    pub text: Color,
    pub border: Color,
    pub hover: Option<Color>,
}

impl Default for TumbleCubeStyle {
    fn default() -> Self {
        Self {
            background: Color::from_rgb8(0xf1, 0xf1, 0xf1),
            text: Color::from_rgb8(0x33, 0x33, 0x33),
            border: Color::from_rgb8(0xcc, 0xcc, 0xcc),
            hover: None,
        }
    }
}

/// One face's pair of images — C# `TextureData`.
pub struct FaceTexture {
    /// The shared cached label (never written).
    pub source: Arc<Vec<u8>>,
    /// This cube's copy, with any hover highlight drawn in.
    pub active: Arc<Vec<u8>>,
    /// `active` differs from `source` (C# `textureChanged`).
    pub changed: bool,
}

/// The six per-instance face textures plus the hover state that drives
/// them.  `version` bumps on every pixel change so renderers know when to
/// re-upload or re-rasterize.
pub struct CubeFaces {
    pub faces: Vec<FaceTexture>,
    last_hit: HitData,
    version: u64,
    style: TumbleCubeStyle,
    epoch: u64,
}

impl CubeFaces {
    pub fn new(style: TumbleCubeStyle) -> Self {
        let mut s = Self {
            faces: Vec::new(),
            last_hit: HitData::NONE,
            version: 0,
            style,
            epoch: 0,
        };
        s.rebuild();
        s
    }

    fn rebuild(&mut self) {
        let st = self.style;
        self.faces = FACE_NAMES
            .iter()
            .map(|name| {
                let source = get_face_image(st.background, st.text, st.border, name);
                FaceTexture {
                    active: source.clone(),
                    source,
                    changed: false,
                }
            })
            .collect();
        self.last_hit = HitData::NONE;
        self.epoch = crate::font_settings::current_thread_typography_epoch();
        self.version += 1;
    }

    /// Re-fetch the labels if the font/LCD settings or colours changed.
    pub fn refresh(&mut self, style: TumbleCubeStyle) {
        if style != self.style
            || self.epoch != crate::font_settings::current_thread_typography_epoch()
        {
            self.style = style;
            self.rebuild();
        }
    }

    pub fn style(&self) -> TumbleCubeStyle {
        self.style
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn last_hit(&self) -> HitData {
        self.last_hit
    }

    /// C# `DrawMouseHover`: when the hit changed, reset and paint the
    /// overlay over every tile the hit names.  Returns whether anything
    /// changed (the caller repaints).
    pub fn highlight(&mut self, hit: HitData, overlay: Color) -> bool {
        if hit == self.last_hit {
            return false;
        }
        self.reset();
        self.last_hit = hit;
        for (face, tile) in hit.pairs() {
            let Some(tex) = self.faces.get_mut(face) else {
                continue;
            };
            // A fresh copy, never the shared source (see module docs).
            let mut active = (*tex.source).clone();
            fill_tile(&mut active, tile, overlay);
            tex.active = Arc::new(active);
            tex.changed = true;
        }
        self.version += 1;
        true
    }

    /// C# `ResetTextures`: restore every changed face from its source.
    pub fn reset(&mut self) -> bool {
        let mut had_reset = false;
        for tex in &mut self.faces {
            if tex.changed {
                tex.active = tex.source.clone();
                tex.changed = false;
                had_reset = true;
            }
        }
        self.last_hit = HitData::NONE;
        if had_reset {
            self.version += 1;
        }
        had_reset
    }
}

#[cfg(test)]
#[path = "face_textures_tests.rs"]
mod tests;
