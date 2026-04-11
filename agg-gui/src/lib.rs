//! # agg-gui
//!
//! A Rust GUI framework built on [AGG](https://github.com/larsbrubaker/agg-rust)
//! (Anti-Grain Geometry).
//!
//! ## Architecture
//!
//! ```text
//! Application / Widgets
//!   │
//! GfxCtx (Cairo-style stateful 2D drawing API)
//!   │
//! AGG (rasterization) + Clipper2 (boolean geometry)
//!   │
//! Framebuffer (RGBA8, bottom-up Y-up row order)
//!   │
//! Platform (WGL native / WebGL WASM)
//! ```
//!
//! ## Coordinate system
//!
//! The entire framework uses **first-quadrant (Y-up)** coordinates throughout.
//! Origin is the bottom-left corner of the window. Positive Y goes upward.
//! This is a non-negotiable architectural invariant — see the dev plan for
//! the rationale.

pub mod color;
pub mod framebuffer;
pub mod geometry;
pub mod gfx_ctx;

// Re-export the most commonly used types at the crate root.
pub use color::Color;
pub use framebuffer::Framebuffer;
pub use geometry::{Point, Rect, Size};
pub use gfx_ctx::GfxCtx;

// Re-export AGG transform so callers can use set_transform without
// importing agg-rust directly.
pub use agg_rust::trans_affine::TransAffine;
pub use agg_rust::math_stroke::{LineCap, LineJoin};

#[cfg(test)]
mod tests;
