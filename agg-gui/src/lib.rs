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

pub mod animation;
pub mod app_state;
pub mod color;
pub mod cursor;
pub mod device_scale;
pub mod draw_ctx;
pub mod event;
pub mod font_settings;
pub mod framebuffer;
pub mod geometry;
pub mod gfx_ctx;
pub mod gl_renderer;
pub mod layout_props;
pub mod lcd_coverage;
pub mod lcd_gfx_ctx;
pub mod persistence;
pub mod pixel_bounds;
pub mod screenshot;
pub mod svg;
pub mod text;
pub mod theme;
pub mod touch_state;
pub mod undo;
#[cfg(target_arch = "wasm32")]
pub mod wasm_clipboard;
pub mod widget;
pub mod widgets;

/// Adapter helpers bridging `winit` types (keyboard, mouse, modifiers,
/// cursor) to this crate's input/cursor types.  Enabled with the
/// `winit-adapter` feature so consumers that don't use winit don't pull
/// the dep.
#[cfg(feature = "winit-adapter")]
pub mod winit_adapter;

/// Adapter helpers for web/JS targets — DOM KeyboardEvent key-string →
/// [`Key`] parser and CSS cursor-name for [`CursorIcon`].  Compiled only
/// for `wasm32` targets.
#[cfg(target_arch = "wasm32")]
pub mod web_adapter;

// Re-export the most commonly used types at the crate root.
pub use app_state::{OsWindowHandle, OsWindowState};
pub use color::Color;
pub use cursor::{current_cursor_icon, reset_cursor_icon, set_cursor_icon, CursorIcon};
pub use device_scale::{device_scale, set_device_scale};
pub use draw_ctx::{DrawCtx, FillRule, GlPaint};
pub use event::{Event, EventResult, Key, Modifiers, MouseButton};
pub use font_settings::current_typography_epoch;
pub use framebuffer::Framebuffer;
pub use geometry::{Point, Rect, Size};
pub use gfx_ctx::GfxCtx;
pub use layout_props::{resolve_fit_or_stretch, HAnchor, Insets, VAnchor, WidgetBase};
pub use screenshot::ScreenshotHandle;
pub use svg::{
    render_svg, render_svg_at_size, render_svg_to_framebuffer, render_svg_to_framebuffer_at_size,
    render_svg_to_lcd_buffer, render_svg_to_lcd_buffer_at_size, render_svg_tree,
    render_svg_tree_at_size, render_svg_tree_to_framebuffer,
    render_svg_tree_to_framebuffer_at_size, render_svg_tree_to_lcd_buffer,
    render_svg_tree_to_lcd_buffer_at_size, SvgRenderError,
};
pub use text::{measure_text_metrics, Font, TextMetrics};
pub use theme::{current_visuals, current_visuals_epoch, set_visuals, ThemePreference, Visuals};
pub use touch_state::{current_multi_touch, MultiTouchInfo, TouchDeviceId, TouchId, TouchPhase};
pub use undo::{DoUndoActions, UndoBuffer, UndoRedoCommand};
pub use widget::{
    collect_inspector_nodes, current_mouse_world, current_viewport, find_widget_by_id,
    find_widget_by_id_mut, find_widget_by_type, App, InspectorNode, Widget,
};
pub use widgets::{
    current_scroll_style, current_scroll_visibility, set_scroll_style, set_scroll_visibility,
    Button, Checkbox, CollapsingHeader, ColorPicker, ComboBox, Container, DragValue, FlexColumn,
    FlexRow, Hyperlink, ImageView, InspectorPanel, InspectorSavedState, Label, LabelAlign,
    MarkdownView, NodeIcon, Padding, ProgressBar, RadioGroup, Resize, ScrollBarColor,
    ScrollBarKind, ScrollBarStyle, ScrollBarVisibility, ScrollView, Separator, SizedBox, Slider,
    Spacer, Splitter, Stack, TabView, TextArea, TextField, ToggleSwitch, Tooltip, TreeView, Window,
};

// Re-export AGG types so callers don't need to import agg-rust directly.
pub use agg_rust::comp_op::CompOp;
pub use agg_rust::math_stroke::{LineCap, LineJoin};
pub use agg_rust::trans_affine::TransAffine;

#[cfg(test)]
mod tests;
