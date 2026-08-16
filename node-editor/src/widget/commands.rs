//! Host-driven editor commands — the "Edit menu" seam for
//! [`super::NodeEditor`].
//!
//! Selection lives *inside* the widget (`NodeEditor::selected`), but the
//! UI that wants to act on it — an application menu bar, a toolbar
//! button, a keyboard-shortcut table — is usually built outside the
//! widget tree and has no way to reach the editor. This module closes
//! that gap the same way [`super::host_hooks`] closes the callback gap:
//! with a small clonable handle the host constructs first, hands to the
//! editor via [`NodeEditor::with_command_handle`], and keeps a copy of.
//!
//! The host pushes a [`NodeEditorCommand`]; the editor drains the queue
//! at the start of every `layout()` (see `widget/mod.rs`), which is the
//! one point guaranteed to run on the frame a `request_draw` schedules.
//! Queueing rather than calling directly is what keeps the handle
//! `Send + Sync` and free of any borrow on the widget.
//!
//! The operations themselves ([`NodeEditor::delete_selection`],
//! [`NodeEditor::select_all`]) are public in their own right, so a host
//! that *does* have `&mut NodeEditor` can call them without the queue —
//! and so the editor's own key handler and right-click menu share one
//! implementation instead of three copies.
//!
//! A handle does not need an editor to exist yet: commands pushed while
//! none is attached (during startup, before the widget tree is built, or
//! after the editor is dropped) simply accumulate in the queue and are
//! all applied, in order, on the first `layout()` that runs once an
//! editor holding this handle is attached.
//!
//! **Poisoning of the command-queue mutex is swallowed everywhere in
//! this module.** A poisoned queue mutex would mean a panic inside one
//! of these tiny critical sections, which contain nothing but a `Vec`
//! push / take; degrading to "no commands" keeps a menu click from
//! cascading a second panic into the UI thread. The cost is a silently
//! dropped command, never corrupted graph state. The *model* lock is a
//! different matter: the operations below `unwrap` it, as the rest of
//! the widget does, because a poisoned model is corrupted graph state
//! and there is nothing safe to degrade to.

use std::sync::{Arc, Mutex};

use crate::model::NodeId;

use super::NodeEditor;

/// One queued editor operation. Additive by design — new variants can be
/// appended without breaking hosts, which only ever construct them.
///
/// `Eq` is deliberately absent: [`Self::SetView`] carries `f64`s, and a
/// command queue has no use for equality beyond the `assert_eq!` in a
/// test, which `PartialEq` already serves.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub enum NodeEditorCommand {
    /// Remove every currently-selected node from the model, exactly as
    /// the Delete key does.
    DeleteSelection,
    /// Select every node the model currently exposes.
    SelectAll,
    /// Frame every node in the view, animated over
    /// [`FIT_ANIM_MS`](super::view_nav::FIT_ANIM_MS). No-op on an empty
    /// graph. See [`NodeEditor::fit_to_content`].
    FitToContent,
    /// Adopt an exact pan / zoom — the restore half of a host that
    /// persists the canvas view with its document. Applied instantly
    /// (no animation) and clamped to the editor's zoom limits.
    ///
    /// Queued rather than called directly because the host restoring a
    /// view (a file-open continuation) has no handle on the widget, and
    /// because `layout()` is the first moment the pane's size is known.
    SetView { scale: f64, offset: [f64; 2] },
    /// Switch what a left-drag on the canvas does. See
    /// [`InteractionMode`](super::InteractionMode).
    SetInteractionMode(super::InteractionMode),
}

/// Clonable command channel between a host's chrome and a
/// [`NodeEditor`].
///
/// Construct one before building the widget tree, install it with
/// [`NodeEditor::with_command_handle`], and keep a clone wherever the
/// menu / toolbar callback can reach it. `Send + Sync`, so it can live
/// in application state shared with worker threads.
#[derive(Clone, Default)]
pub struct NodeEditorHandle {
    queue: Arc<Mutex<Vec<NodeEditorCommand>>>,
}

impl NodeEditorHandle {
    /// Create an empty command channel. Clone it to share the same queue
    /// between the host's chrome and the editor.
    pub fn new() -> Self {
        Self::default()
    }

    /// Queue `command` for the editor's next layout pass and ask for a
    /// frame, so the work happens promptly even when nothing else on
    /// screen is animating.
    pub fn push(&self, command: NodeEditorCommand) {
        // A poisoned lock drops the command rather than panicking — see
        // the module header. `request_draw` still fires in that case,
        // costing one no-op frame; keeping it unconditional avoids a
        // branch whose only payoff is skipping a repaint that is already
        // cheap when nothing changed.
        if let Ok(mut queue) = self.queue.lock() {
            queue.push(command);
        }
        agg_gui::animation::request_draw();
    }

    /// True while at least one command is still waiting to be drained.
    /// Mostly useful to tests asserting a click enqueued something.
    /// Reports `false` on a poisoned lock (module header).
    pub fn is_pending(&self) -> bool {
        self.queue
            .lock()
            .map(|queue| !queue.is_empty())
            .unwrap_or(false)
    }

    /// Take everything queued so far, leaving the channel empty. Yields
    /// an empty `Vec` on a poisoned lock (module header).
    pub fn take(&self) -> Vec<NodeEditorCommand> {
        self.queue
            .lock()
            .map(|mut queue| std::mem::take(&mut *queue))
            .unwrap_or_default()
    }
}

impl std::fmt::Debug for NodeEditorHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NodeEditorHandle")
            .field("pending", &self.is_pending())
            .finish()
    }
}

impl NodeEditor {
    /// Install a host command channel. See [`NodeEditorHandle`].
    pub fn with_command_handle(mut self, handle: NodeEditorHandle) -> Self {
        self.command_handle = Some(handle);
        self
    }

    /// Remove every selected node from the model and clear the
    /// selection. Returns `true` when something was actually removed —
    /// the key handler uses that to decide whether it consumed the
    /// event.
    ///
    /// Shared by the Delete / Backspace key, the right-click menu's
    /// "Delete", and the [`NodeEditorCommand::DeleteSelection`] queue
    /// entry, so all three behave identically.
    ///
    /// The whole selection reaches the model through a single
    /// `NodeGraphModel::remove_nodes` call rather than one
    /// `remove_node` per id — that grouping is the seam a host with an
    /// undo stack uses to record a multi-node delete as one undo step.
    ///
    /// Fires the primary-selection hook
    /// (`NodeGraphModel::on_primary_selection_changed`) with `None` when
    /// the model's primary selection was among the removed nodes, so a
    /// host mirroring the primary selection in its own state is never
    /// left pointing at a node that no longer exists. A primary outside
    /// the deleted set is still valid and is left alone.
    pub fn delete_selection(&mut self) -> bool {
        if self.selected.is_empty() {
            return false;
        }
        let to_remove: Vec<NodeId> = self.selected.drain().collect();
        let primary = {
            let mut model = self.model.lock().unwrap();
            let primary = model.primary_selection();
            model.remove_nodes(&to_remove);
            primary
        };
        // Outside the model lock — `notify_primary_selection` takes it.
        if primary.is_some_and(|id| to_remove.contains(&id)) {
            self.notify_primary_selection(None);
        }
        // Removing a node invalidates the cached child widget tree and
        // the GL backbuffer — neither will update without an explicit
        // request.
        self.backbuffer.invalidate();
        agg_gui::animation::request_draw();
        true
    }

    /// Select every node the model currently exposes. Returns `true`
    /// when the selection changed.
    ///
    /// Does **not** fire the primary-selection hook
    /// (`NodeGraphModel::on_primary_selection_changed`): widening a
    /// selection leaves any existing primary node still selected and
    /// therefore still valid, and there is no non-arbitrary choice of
    /// primary among "all nodes" when nothing was selected before. A
    /// host mirroring the primary selection will thus see it stay empty
    /// after Select All even though every node is highlighted; hosts
    /// wanting a different rule must set their mirror themselves.
    pub fn select_all(&mut self) -> bool {
        let ids: Vec<NodeId> = {
            let model = self.model.lock().unwrap();
            model.nodes().iter().map(|n| n.id).collect()
        };
        if ids.len() == self.selected.len() && ids.iter().all(|id| self.selected.contains(id)) {
            return false;
        }
        self.selected = ids.into_iter().collect();
        self.backbuffer.invalidate();
        agg_gui::animation::request_draw();
        true
    }

    /// Apply everything the host queued since the last frame. Called
    /// from `layout()` *before* the node snapshot is taken, so a
    /// selection or deletion applied here is visible to this frame's
    /// fingerprint and children rebuild.
    pub(super) fn drain_commands(&mut self) {
        let Some(handle) = self.command_handle.clone() else {
            return;
        };
        for command in handle.take() {
            match command {
                NodeEditorCommand::DeleteSelection => {
                    self.delete_selection();
                }
                NodeEditorCommand::SelectAll => {
                    self.select_all();
                }
                NodeEditorCommand::FitToContent => {
                    self.fit_to_content();
                }
                NodeEditorCommand::SetView { scale, offset } => {
                    // A non-finite view is refused (see `set_view`);
                    // there is nothing useful a queue drain can do about
                    // it beyond not applying it.
                    let _ = self.set_view(scale, offset);
                }
                NodeEditorCommand::SetInteractionMode(mode) => {
                    self.set_interaction_mode(mode);
                }
            }
        }
    }
}
