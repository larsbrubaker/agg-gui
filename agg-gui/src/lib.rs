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

pub mod app_state;
pub mod color;
pub mod cursor;
pub mod device_scale;
pub mod undo;
#[cfg(target_arch = "wasm32")]
pub mod wasm_clipboard;
pub mod draw_ctx;
pub mod event;
pub mod framebuffer;
pub mod geometry;
pub mod gfx_ctx;
pub mod gl_renderer;
pub mod layout_props;
pub mod screenshot;
pub mod text;
pub mod theme;
pub mod widget;
pub mod widgets;

// Re-export the most commonly used types at the crate root.
pub use app_state::{OsWindowHandle, OsWindowState};
pub use screenshot::ScreenshotHandle;
pub use color::Color;
pub use cursor::{CursorIcon, current_cursor_icon, set_cursor_icon, reset_cursor_icon};
pub use device_scale::{device_scale, set_device_scale};
pub use draw_ctx::{DrawCtx, GlPaint};
pub use theme::{ThemePreference, Visuals, current_visuals, set_visuals};
pub use event::{Event, EventResult, Key, Modifiers, MouseButton};
pub use framebuffer::Framebuffer;
pub use geometry::{Point, Rect, Size};
pub use gfx_ctx::GfxCtx;
pub use layout_props::{HAnchor, Insets, VAnchor, WidgetBase, resolve_fit_or_stretch};
pub use text::{Font, TextMetrics, measure_text_metrics};
pub use undo::{DoUndoActions, UndoBuffer, UndoRedoCommand};
pub use widget::{App, InspectorNode, Widget, collect_inspector_nodes};
pub use widgets::{Button, Checkbox, CollapsingHeader, ComboBox, Container, DragValue, FlexColumn, FlexRow,
                  Hyperlink, ImageView, InspectorPanel, InspectorSavedState,
                  Label, LabelAlign, MarkdownView, NodeIcon, Padding, ProgressBar, RadioGroup,
                  ScrollBarColor, ScrollBarKind, ScrollBarStyle, ScrollBarVisibility,
                  ScrollView, Separator, SizedBox, Slider, Spacer, Splitter, Stack,
                  TabView, TextField, ToggleSwitch, Tooltip, TreeView, Window,
                  current_scroll_style, current_scroll_visibility,
                  set_scroll_style, set_scroll_visibility};

// Re-export AGG types so callers don't need to import agg-rust directly.
pub use agg_rust::trans_affine::TransAffine;
pub use agg_rust::math_stroke::{LineCap, LineJoin};
pub use agg_rust::comp_op::CompOp;

#[cfg(test)]
mod tests;
