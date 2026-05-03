//! Texture upload and image blit helpers for the wgpu backend.
//!
//! Mirrors `demo-gl/src/image_blit.rs`, providing:
//! - `draw_image_rgba_slice_impl` — uploads a `&[u8]` as a transient texture
//!   keyed by a lightweight hash, with LRU eviction at 512 entries.
//! - `draw_image_rgba_arc_impl` — Arc-pointer-keyed hot path for Label
//!   backbuffers; one GPU upload per unique raster, lifetime tied to the `Arc`.
//!
//! Phase 6 implements the full texture pipeline.

// Placeholder — Phase 6 fills this module.
