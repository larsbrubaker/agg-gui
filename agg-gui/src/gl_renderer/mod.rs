//! GL renderer infrastructure — tess2 bridge + GL command buffer.
//!
//! This module provides the building blocks for hardware-accelerated rendering:
//!
//! - [`tess2_bridge`] — converts AGG-style polygon contours into GL triangle
//!   meshes using tess2-rust.
//!
//! The higher-level [`GlGfxCtx`] (a drop-in parallel to [`crate::GfxCtx`] for
//! GL targets) and the full [`RenderTarget`] abstraction are planned extensions.
//!
//! # Reference
//!
//! Modelled after the MatterCAD agg-sharp `Graphics2DGpu` / `AARenderTesselator`
//! pipeline: shapes are tessellated to triangle meshes, then uploaded as VBOs
//! and rendered with a simple colour-fill shader.  Anti-aliased edge expansion
//! uses a **bidirectional** 1-pixel feather centred on the polygon edge
//! (inner endpoint half-width inside at α=1, outer endpoint half-width
//! outside at α=0) — same shape as epaint's tessellator — so adjacent
//! polygons' feather strips overlap with complementary alpha and tile
//! without bleeding into each other.

pub mod glyph_cache;
pub mod tess2_bridge;

#[cfg(test)]
mod aa_feather_tests;

pub use glyph_cache::GlyphCache;
pub use tess2_bridge::{
    agg_path_to_contours, expand_aa_halo, install_tess_panic_logger, tessellate_circle,
    tessellate_fill, tessellate_interior, tessellate_path, tessellate_path_aa, tessellate_rect,
    tessellate_rounded_rect, CachedTess,
};
