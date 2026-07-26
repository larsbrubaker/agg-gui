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
//!
//! ## Module guide
//!
//! - [`widget`] / [`widgets`] — the [`App`] event/layout/paint driver and the
//!   widget set (buttons, text editing, windows, flex layout, menus, …).
//! - [`draw_ctx`] — the [`DrawCtx`] drawing trait every widget paints
//!   through; [`gfx_ctx`] is the software AGG implementation.
//! - [`theme`] — dark / light / system visuals read via `ctx.visuals()`.
//! - [`overlay_insets`] + [`widgets::ReserveInset`] + [`card`] — safe-area
//!   overlay placement: reserved screen edges (the on-screen keyboard
//!   reserves itself), and anchored info cards that measure, word-wrap,
//!   flip, and clamp so floating content never hides under sibling chrome.
//! - [`input_profile`] / [`ux_scale`] — touch-device detection and the
//!   mobile UX zoom; [`widgets::on_screen_keyboard`] is the in-canvas
//!   keyboard with keyboard-avoidance handled by [`App::layout`].
//! - `winit_adapter` (feature `winit-adapter`) and, on WASM,
//!   `web_adapter` (`install_keyboard_listeners` = typing + clipboard
//!   for any browser shell) — platform glue. The repo's `demo-wgpu` crate
//!   adds turn-key `native_shell` / `web_shell` runners on top.

pub mod animation;
pub mod app_state;
pub mod card;
pub mod clipboard;
pub mod color;
pub mod confetti;
pub mod cursor;
pub mod device_scale;
pub mod draw_cell;
pub mod draw_ctx;
pub mod event;
pub mod focus;
pub mod font_settings;
pub mod framebuffer;
pub mod fullscreen;
pub mod geometry;
pub mod gfx_ctx;
pub mod gl_renderer;
pub mod input_profile;
pub mod layout_props;
pub mod lcd_coverage;
pub mod lcd_gfx_ctx;
pub mod overlay_insets;
pub mod paints;
pub mod persistence;
pub mod pixel_bounds;
pub mod platform;
pub mod screenshot;
pub mod snap;
pub mod svg;
pub mod text;
pub mod theme;
pub mod tilt;
pub mod timestep;
pub mod touch_emulation;
pub mod touch_points;
pub mod touch_state;
pub mod undo;
pub mod ux_scale;
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
pub use draw_cell::{DrawCell, DrawRefCell, DrawRefMut};
pub use draw_ctx::{DrawCtx, FillRule, GlPaint};
pub use event::{Event, EventResult, Key, Modifiers, MouseButton};
pub use font_settings::current_typography_epoch;
pub use framebuffer::Framebuffer;
pub use geometry::{Point, Rect, Size};
pub use gfx_ctx::GfxCtx;
pub use layout_props::{resolve_fit_or_stretch, HAnchor, Insets, VAnchor, WidgetBase};
pub use platform::{current_platform, platform_from_name, set_platform, Platform};
pub use screenshot::ScreenshotHandle;
pub use snap::{
    compute_snap, next_snap_id, ResizeEdge as SnapResizeEdge, SnapGuide, SnapId, SnapMode,
    SnapOverlay, SnapResult, Snappable, DEFAULT_THRESHOLD as SNAP_DEFAULT_THRESHOLD,
};
pub use svg::{
    compare_svg_rgba, parse_svg, render_svg, render_svg_at_size, render_svg_at_size_with_options,
    render_svg_at_size_with_resources, render_svg_to_framebuffer,
    render_svg_to_framebuffer_at_size, render_svg_to_framebuffer_at_size_with_options,
    render_svg_to_framebuffer_at_size_with_resources, render_svg_to_framebuffer_with_options,
    render_svg_to_lcd_buffer, render_svg_to_lcd_buffer_at_size,
    render_svg_to_lcd_buffer_at_size_with_options, render_svg_to_lcd_buffer_at_size_with_resources,
    render_svg_to_lcd_buffer_with_options, render_svg_tree, render_svg_tree_at_size,
    render_svg_tree_region_at_size, render_svg_tree_region_to_framebuffer_at_size,
    render_svg_tree_to_framebuffer, render_svg_tree_to_framebuffer_at_size,
    render_svg_tree_to_lcd_buffer, render_svg_tree_to_lcd_buffer_at_size, render_svg_with_options,
    set_default_svg_parse_options, svg_fontdb_from_font_data, SvgCompareResult,
    SvgCompareThresholds, SvgParseOptions, SvgRenderError, SvgTree, DEFAULT_ALPHA_TOLERANCE,
    DEFAULT_MISMATCH_RATIO, DEFAULT_OPAQUE_RGB_TOLERANCE, DEFAULT_TRANSLUCENT_RGB_TOLERANCE,
    DEFAULT_VISUAL_RGB_TOLERANCE,
};
pub use text::{measure_text_metrics, Font, TextMetrics};
pub use theme::{
    current_visuals, current_visuals_epoch, set_visuals, AccentColor, ThemePreference, Visuals,
};
pub use timestep::{FixedTimestep, StepBatch, FIXED_DT, MAX_STEPS_PER_DRAW, SIMULATION_HZ};
pub use touch_emulation::{EmuCmd, TouchMouseEmu, TOUCH_SCROLL_THRESHOLD};
pub use touch_state::{current_multi_touch, MultiTouchInfo, TouchDeviceId, TouchId, TouchPhase};
pub use undo::{DoUndoActions, Settings as UndoerSettings, UndoBuffer, UndoRedoCommand, Undoer};
#[cfg(feature = "reflect")]
pub use widget::{apply_inspector_edit, reflect_fields, InspectorEdit};
pub use widget::{
    apply_widget_base_edit, collect_inspector_nodes, current_mouse_world, current_viewport,
    debug_draw_report, find_widget_by_id, find_widget_by_id_mut, find_widget_by_type, App,
    BackbufferKind, BackbufferSpec, BackbufferState, InspectorNode, InspectorOverlay, Widget,
    WidgetBaseEdit, WidgetBaseField,
};
pub use widgets::{
    color_wheel_picker_dialog, color_wheel_picker_dialog_with_on_close, current_scroll_style,
    current_scroll_visibility, paint_sparkline, set_scroll_style, set_scroll_visibility,
    set_tooltip_timings, shared_frame_history, shared_run_mode, single_font_resolver,
    tooltip_timings, Align, Align2, Button, CellInfo, Checkbox, ClickAwayAction, CloseReason,
    CollapsingHeader, ColorPicker, ColorWheelPicker, ComboBox, Conditional, Container, DragValue,
    FlexColumn, FlexRow, FrameHistory, HandleShape, HeaderInfo, Hyperlink, ImageView,
    InspectorPanel, InspectorSavedState, Label, LabelAlign, MarkdownView, MenuBar, MenuBarStrip,
    MenuEntry, MenuItem, MenuResponse, MenuSelection, MenuShortcut, ModalSheet, NodeIcon, Padding,
    PerformanceView, Popup, PopupClickOutcome, PopupCloseBehavior, PopupMenu, ProgressBar, QrView,
    RadioGroup, Rebuilder, RectAlign, Resize, RichCommand, RichDoc, RichEditHandle, RichTextEdit,
    RichTextToolbar, RichTextView, RunMode, RunModeDesc, RunModeRow, Scene, SceneTransform,
    ScrollBarColor, ScrollBarKind, ScrollBarStyle, ScrollBarVisibility, ScrollView, Separator,
    SharedFrameHistory, SharedResolver, ShortcutKey, SizedBox, Slider, SliderClamping,
    SliderOrientation, Spacer, Splitter, Stack, TabView, Table, TableBuilder, TableColumn,
    TableRows, TextArea, TextAreaScrollInfo, TextEditState, TextField, TextHAlign, TextVAlign,
    ToggleSwitch, Tooltip, TooltipTimings, TopMenu, TreeView, Window, DEFAULT_COLUMN_GAP,
    DEFAULT_ROW_GAP,
};

// Re-export AGG types so callers don't need to import agg-rust directly.
pub use agg_rust::comp_op::CompOp;
pub use agg_rust::math_stroke::{LineCap, LineJoin};
pub use agg_rust::trans_affine::TransAffine;

#[cfg(test)]
mod tests;
