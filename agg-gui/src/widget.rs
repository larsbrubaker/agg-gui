//! Widget trait, tree traversal, and the top-level [`App`] struct.
//!
//! # Coordinate system
//!
//! Widget bounds are expressed in **parent-local** first-quadrant (Y-up)
//! coordinates. A widget at `bounds.x = 10, bounds.y = 20` is drawn 10 units
//! right and 20 units up from its parent's bottom-left corner.
//!
//! OS/browser mouse events arrive in Y-down screen coordinates. The single
//! conversion `y_up = viewport_height - y_down` happens inside
//! [`App::on_mouse_move`] / [`App::on_mouse_down`] / [`App::on_mouse_up`].
//! All widget code sees Y-up coordinates only.
//!
//! # Tree traversal
//!
//! Paint: root → leaves (children painted on top of parents).
//! Hit test: root → leaves (deepest child under cursor wins).
//! Event dispatch: leaf → root (events bubble up; any widget can consume).

// Submodules (`app`, `tree`, `paint`, ...) glob-import these via `super::*`.
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult, Key, Modifiers};
use crate::geometry::{Point, Rect, Size};

mod widget_trait;

pub use widget_trait::Widget;

mod app;
mod backbuffer;
mod event_observer;
mod event_root;
pub(crate) mod keyboard_scroll;
mod paint;
pub(crate) mod paint_timing;
mod tree;
mod tree_inspector;

pub use app::{under_mouse_state_of, App, UnderMouseState, WidgetAnchor, WidgetId};
pub(crate) use backbuffer::next_content_version;
pub use backbuffer::{
    BackbufferBand, BackbufferCache, BackbufferKind, BackbufferMode, BackbufferSpec,
    BackbufferState, CompositingLayer,
};
pub use event_observer::{observe_events, EventObserver};
pub use event_root::{event_rect_to_root, event_root_transform};
pub(crate) use paint::paint_subtree_forced;
pub use paint::{
    current_paint_clip, is_local_rect_in_paint_clip, paint_global_overlays, paint_subtree,
};
pub use tree::{
    activate_action_at, active_modal_path, cancel_action_path, default_action_path, dispatch_event,
    dispatch_event_broadcast, dispatch_event_dyn, dispatch_unconsumed_key, global_overlay_hit_path,
    hit_test_subtree, mark_subtree_dirty,
};
#[cfg(feature = "reflect")]
pub use tree_inspector::{apply_inspector_edit, reflect_fields, InspectorEdit};
pub use tree_inspector::{
    apply_widget_base_edit, collect_inspector_nodes, current_mouse_world, current_viewport,
    debug_draw_report, find_widget_by_id, find_widget_by_id_mut, find_widget_by_type,
    find_widget_screen_rect, set_current_mouse_world, set_current_viewport, walk_path,
    walk_path_mut, InspectorNode, InspectorOverlay, WidgetBaseEdit, WidgetBaseField,
};
