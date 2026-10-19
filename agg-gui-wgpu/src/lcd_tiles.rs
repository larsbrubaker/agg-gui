//! GPU textures for LCD coverage planes, split into tiles no larger than the
//! device allows.
//!
//! A coverage plane is as wide as the text run it holds, and a long single
//! line (a few thousand characters) is wider than any GPU's
//! `max_texture_dimension_2d`; asking for such a texture is a validation
//! error, which wgpu's default handler turns into a panic. So a plane is
//! uploaded as a row of tiles (a grid, if it is also too tall), each at most
//! the device's limit on a side, and drawn as one quad per tile. A plane that
//! fits is one tile, so ordinary text costs nothing extra.
//!
//! [`crate::text_render`] owns the cache and the draw commands; this file
//! owns the textures: [`upload_lcd_tiles`] makes them and
//! [`write_lcd_tiles`] refreshes them in place.

use std::sync::Arc;

use crate::texture_life::TextureLedger;

/// One tile of an uploaded LCD plane: its texture, and which texels of the
/// plane it holds (`x`, `y` from the plane's first column and first row).
#[derive(Clone)]
pub(crate) struct LcdTile {
    pub(crate) texture: Arc<wgpu::Texture>,
    pub(crate) view: wgpu::TextureView,
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) w: u32,
    pub(crate) h: u32,
}

/// `len` split into consecutive `(start, length)` spans of at most `max`
/// each (at least one span; `max` below 1 counts as 1).
pub(crate) fn tile_spans(len: u32, max: u32) -> Vec<(u32, u32)> {
    let max = max.max(1);
    let mut spans = Vec::with_capacity(len.div_ceil(max).max(1) as usize);
    let mut start = 0;
    loop {
        let span = (len - start).min(max);
        spans.push((start, span));
        start += span;
        if start >= len {
            return spans;
        }
    }
}

/// Upload a tightly packed `w` x `h` RGB8 plane as tiles no larger than
/// `max_dim` on a side (row-major: every tile of the first rows, then the
/// next).
pub(crate) fn upload_lcd_tiles(
    textures: &mut TextureLedger,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    rgb: &[u8],
    w: u32,
    h: u32,
    max_dim: u32,
) -> Arc<[LcdTile]> {
    let mut tiles = Vec::new();
    for &(y, th) in &tile_spans(h, max_dim) {
        for &(x, tw) in &tile_spans(w, max_dim) {
            let texture = textures.create(
                device,
                &wgpu::TextureDescriptor {
                    label: None,
                    size: wgpu::Extent3d {
                        width: tw,
                        height: th,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                },
            );
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            tiles.push(LcdTile {
                texture: Arc::new(texture),
                view,
                x,
                y,
                w: tw,
                h: th,
            });
        }
    }
    write_lcd_tiles(queue, &tiles, rgb, w);
    tiles.into()
}

/// Write each tile's part of a `plane_w`-wide RGB8 plane into its texture
/// (RGB expanded to RGBA). The caller guarantees the tiles were made for a
/// plane of this size.
pub(crate) fn write_lcd_tiles(queue: &wgpu::Queue, tiles: &[LcdTile], rgb: &[u8], plane_w: u32) {
    for tile in tiles {
        let rgba = rgb_rect_to_rgba(rgb, plane_w, tile);
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &tile.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(tile.w * 4),
                rows_per_image: Some(tile.h),
            },
            wgpu::Extent3d {
                width: tile.w,
                height: tile.h,
                depth_or_array_layers: 1,
            },
        );
    }
}

/// The tile's rectangle of a tightly packed RGB8 plane as RGBA8 (alpha 255).
/// GPUs have no 3-byte texture formats; the LCD shaders sample `.rgb`, so the
/// padded alpha byte is harmless.
fn rgb_rect_to_rgba(rgb: &[u8], plane_w: u32, tile: &LcdTile) -> Vec<u8> {
    let mut out = Vec::with_capacity(tile.w as usize * tile.h as usize * 4);
    for row in tile.y..tile.y + tile.h {
        let start = (row as usize * plane_w as usize + tile.x as usize) * 3;
        let end = start + tile.w as usize * 3;
        for px in rgb[start..end].chunks_exact(3) {
            out.extend_from_slice(&[px[0], px[1], px[2], 255]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::tile_spans;

    #[test]
    fn a_length_within_the_limit_is_one_span() {
        assert_eq!(tile_spans(100, 16384), vec![(0, 100)]);
        assert_eq!(tile_spans(16384, 16384), vec![(0, 16384)]);
    }

    #[test]
    fn a_longer_length_splits_into_spans_of_at_most_the_limit() {
        assert_eq!(tile_spans(28795, 16384), vec![(0, 16384), (16384, 12411)]);
        assert_eq!(tile_spans(10, 4), vec![(0, 4), (4, 4), (8, 2)]);
    }
}
