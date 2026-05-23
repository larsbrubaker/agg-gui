//! `agg-gui-node-editor` — a reusable, model-agnostic node-graph editor
//! widget for [`agg-gui`].
//!
//! # Overview
//!
//! Drop a [`NodeEditor`] into your widget tree and point it at an
//! implementation of [`NodeGraphModel`]. The widget handles the canvas
//! UX (pan, zoom, drag-to-connect, right-click menu, keyboard delete,
//! property drags); the host owns the underlying graph and decides how
//! mutations propagate.
//!
//! ```ignore
//! use std::sync::{Arc, Mutex};
//! use agg_gui_node_editor::{NodeEditor, NodeGraphModel};
//!
//! struct MyModel { /* ... */ }
//! impl NodeGraphModel for MyModel { /* ... */ }
//!
//! let model: Arc<Mutex<dyn NodeGraphModel + Send>> =
//!     Arc::new(Mutex::new(MyModel { /* ... */ }));
//! let editor = NodeEditor::new(model);
//! ```
//!
//! # Architecture
//!
//! - [`NodeGraphModel`] — the trait the widget reads/writes through.
//! - [`NodeView`], [`EdgeView`], [`NodeTypeView`], [`PropertyView`] —
//!   owned data the model returns from snapshot calls. The widget
//!   never holds a long-lived borrow into the host graph.
//! - [`NodeEditor`] — the widget itself. Implements `agg_gui::Widget`.
//! - [`CanvasPalette`] — theme-driven colour bundle (auto-rebuilt
//!   from `ctx.visuals()` per paint, or set manually with
//!   [`NodeEditor::set_palette`]).
//!
//! See [`crate::model`] for the trait + view types and [`crate::widget`]
//! for the widget implementation.

pub mod draw;
pub mod model;
pub mod widget;

pub use draw::CanvasPalette;
pub use model::{
    EdgeResult, EdgeView, EditorHint, NodeGraphModel, NodeId, NodeTypeView, NodeView,
    PropertyValue, PropertyView, SocketTypeId, SocketView,
};
pub use widget::{NodeEditor, SharedModel};
