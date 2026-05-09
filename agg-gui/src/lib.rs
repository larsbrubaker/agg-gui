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
pub mod clipboard;
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
pub mod paints;
pub mod persistence;
pub mod pixel_bounds;
pub mod platform;
pub mod screenshot;
pub mod svg;
pub mod text;
pub mod theme;
pub mod timestep;
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
pub use platform::{current_platform, platform_from_name, set_platform, Platform};
pub use screenshot::ScreenshotHandle;
pub use svg::{
    compare_svg_rgba, parse_svg, render_svg, render_svg_at_size, render_svg_at_size_with_options,
    render_svg_at_size_with_resources, render_svg_to_framebuffer,
    render_svg_to_framebuffer_at_size, render_svg_to_framebuffer_at_size_with_options,
    render_svg_to_framebuffer_at_size_with_resources, render_svg_to_framebuffer_with_options,
    render_svg_to_lcd_buffer, render_svg_to_lcd_buffer_at_size,
    render_svg_to_lcd_buffer_at_size_with_options, render_svg_to_lcd_buffer_at_size_with_resources,
    render_svg_to_lcd_buffer_with_options, render_svg_tree, render_svg_tree_at_size,
    render_svg_tree_to_framebuffer, render_svg_tree_to_framebuffer_at_size,
    render_svg_tree_to_lcd_buffer, render_svg_tree_to_lcd_buffer_at_size, render_svg_with_options,
    set_default_svg_parse_options, svg_fontdb_from_font_data, SvgCompareResult,
    SvgCompareThresholds, SvgParseOptions, SvgRenderError, DEFAULT_ALPHA_TOLERANCE,
    DEFAULT_MISMATCH_RATIO, DEFAULT_OPAQUE_RGB_TOLERANCE, DEFAULT_TRANSLUCENT_RGB_TOLERANCE,
    DEFAULT_VISUAL_RGB_TOLERANCE,
};
pub use text::{measure_text_metrics, Font, TextMetrics};
pub use theme::{
    current_visuals, current_visuals_epoch, set_visuals, AccentColor, ThemePreference, Visuals,
};
pub use timestep::{FixedTimestep, StepBatch, FIXED_DT, MAX_STEPS_PER_DRAW, SIMULATION_HZ};
pub use touch_state::{current_multi_touch, MultiTouchInfo, TouchDeviceId, TouchId, TouchPhase};
pub use undo::{DoUndoActions, UndoBuffer, UndoRedoCommand};
pub use widget::{
    apply_widget_base_edit, collect_inspector_nodes, current_mouse_world, current_viewport,
    find_widget_by_id, find_widget_by_id_mut, find_widget_by_type, App, BackbufferKind,
    BackbufferSpec, BackbufferState, InspectorNode, InspectorOverlay, Widget, WidgetBaseEdit,
    WidgetBaseField,
};
#[cfg(feature = "reflect")]
pub use widget::{
    apply_inspector_edit, reflect_fields, InspectorEdit,
};
pub use widgets::{
    current_scroll_style, current_scroll_visibility, set_scroll_style, set_scroll_visibility,
    Button, Checkbox, CollapsingHeader, ColorPicker, ComboBox, Conditional, Container, DragValue,
    FlexColumn, FlexRow, Hyperlink, ImageView, InspectorPanel, InspectorSavedState, Label, LabelAlign,
    MarkdownView, MenuBar, MenuEntry, MenuItem, MenuResponse, MenuSelection, MenuShortcut,
    NodeIcon, Padding, PopupMenu, ProgressBar, RadioGroup, Resize, ScrollBarColor, ScrollBarKind,
    ScrollBarStyle, ScrollBarVisibility, ScrollView, Separator, ShortcutKey, SizedBox, Slider,
    CellInfo, HeaderInfo, Spacer, Splitter, Stack, TabView, Table, TableBuilder, TableColumn,
    TableRows, TextArea, TextField, ToggleSwitch, Tooltip, TopMenu, TreeView, Window,
};

// Re-export AGG types so callers don't need to import agg-rust directly.
pub use agg_rust::comp_op::CompOp;
pub use agg_rust::math_stroke::{LineCap, LineJoin};
pub use agg_rust::trans_affine::TransAffine;

#[cfg(test)]
mod tests;
