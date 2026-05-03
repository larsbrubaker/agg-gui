//! `BarGridWgpuRenderer` and `WgpuCubeWidget` — wgpu port of the 3-D bar-grid
//! animation widget.
//!
//! Mirrors the role of `bar_grid.rs` in `demo-gl`: both the renderer and the
//! widget live in this shared crate so that `demo-native` and `demo-wasm` use
//! exactly the same compiled bytes.
//!
//! Phase 9 implements the full bar-grid wgpu rendering.

use agg_gui::geometry::Rect;

/// Screen-space Y-up rect reserved for the 3-D bar-grid widget.
pub const CUBE_SCREEN_RECT: Rect = Rect {
    x: 0.0,
    y: 0.0,
    width: 0.0,
    height: 0.0,
};

/// Wgpu renderer for the instanced 3-D bar grid.  Phase 9 stub.
pub struct BarGridWgpuRenderer;

/// The widget that drives [`BarGridWgpuRenderer`] via [`agg_gui::GlPaint`].
pub struct WgpuCubeWidget;
