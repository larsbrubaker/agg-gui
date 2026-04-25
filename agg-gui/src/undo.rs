//! Shared undo / redo infrastructure.
//!
//! Mirrors the C# agg-sharp `IUndoRedoCommand` / `UndoBuffer` pattern so that
//! any subsystem — text editing, layout, graph editing — can participate in a
//! common, extensible undo stack.
//!
//! # Usage
//!
//! ```rust,ignore
//! use agg_gui::undo::{DoUndoActions, UndoBuffer};
//!
//! let mut buf = UndoBuffer::new();
//!
//! // Execute an action and make it undoable:
//! let v = std::rc::Rc::new(std::cell::Cell::new(0i32));
//! let v2 = v.clone();
//! buf.add_and_do(Box::new(DoUndoActions::new(
//!     "set value",
//!     move || v.set(42),
//!     move || v2.set(0),
//! )));
//! ```

// ---------------------------------------------------------------------------
// IUndoRedoCommand — the core trait
// ---------------------------------------------------------------------------

/// A named, reversible operation.
///
/// Implement this trait to participate in the shared undo/redo stack.
/// The `do_it` / `undo_it` methods are called by [`UndoBuffer`] on redo and
/// undo respectively.
pub trait UndoRedoCommand {
    /// Short human-readable description, e.g. `"insert text"`.
    fn name(&self) -> &str;
    /// Re-apply the operation (called on Redo).
    fn do_it(&mut self);
    /// Reverse the operation (called on Undo).
    fn undo_it(&mut self);
}

// ---------------------------------------------------------------------------
// UndoBuffer
// ---------------------------------------------------------------------------

/// Two-stack undo/redo history buffer.
///
/// Mirrors the C# `UndoBuffer` class: when a new action is added the redo
/// stack is cleared (so a new branch cannot be redone).  The undo stack is
/// size-limited; the oldest entries are dropped when the limit is exceeded.
pub struct UndoBuffer {
    undo_stack: Vec<Box<dyn UndoRedoCommand>>,
    redo_stack: Vec<Box<dyn UndoRedoCommand>>,
    max_undos: usize,
}

impl UndoBuffer {
    /// Create a new buffer with a default history limit of 200 entries.
    pub fn new() -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            max_undos: 200,
        }
    }

    /// Set the maximum number of undo steps retained.
    pub fn with_max_undos(mut self, n: usize) -> Self {
        self.max_undos = n;
        self
    }

    /// Push `cmd` without executing it.
    ///
    /// Use this when the action has **already** been applied to the state;
    /// the command only needs to know how to undo (and redo) it.
    /// Clears the redo stack.
    pub fn add(&mut self, cmd: Box<dyn UndoRedoCommand>) {
        self.redo_stack.clear();
        self.undo_stack.push(cmd);
        if self.undo_stack.len() > self.max_undos {
            self.undo_stack.remove(0);
        }
    }

    /// Execute `cmd.do_it()` and push it onto the undo stack.
    ///
    /// Use this when the action has **not** yet been applied.
    pub fn add_and_do(&mut self, mut cmd: Box<dyn UndoRedoCommand>) {
        cmd.do_it();
        self.add(cmd);
    }

    /// Undo the most recent operation.  No-op if the stack is empty.
    pub fn undo(&mut self) {
        if let Some(mut cmd) = self.undo_stack.pop() {
            cmd.undo_it();
            self.redo_stack.push(cmd);
        }
    }

    /// Redo the most recently undone operation.  No-op if the redo stack is empty.
    pub fn redo(&mut self) {
        if let Some(mut cmd) = self.redo_stack.pop() {
            cmd.do_it();
            self.undo_stack.push(cmd);
        }
    }

    /// Returns `true` if there is at least one operation that can be undone.
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    /// Returns `true` if there is at least one operation that can be redone.
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    /// Name of the operation that `undo()` would reverse, if any.
    pub fn undo_name(&self) -> Option<&str> {
        self.undo_stack.last().map(|c| c.name())
    }

    /// Name of the operation that `redo()` would re-apply, if any.
    pub fn redo_name(&self) -> Option<&str> {
        self.redo_stack.last().map(|c| c.name())
    }

    /// Discard all undo and redo history.
    pub fn clear_history(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
    }
}

impl Default for UndoBuffer {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// DoUndoActions — closure-based command
// ---------------------------------------------------------------------------

/// A command backed by two closures: one for `do_it` and one for `undo_it`.
///
/// This is the Rust equivalent of the C# `DoUndoActions` class.  Use it for
/// simple operations where capturing state in closures is natural.
///
/// For operations that share state with an owning object (e.g. text editing),
/// consider using `std::rc::Rc<std::cell::RefCell<T>>` to share mutable state
/// between the owning widget and the undo command closures.
pub struct DoUndoActions {
    name: String,
    do_fn: Box<dyn FnMut()>,
    undo_fn: Box<dyn FnMut()>,
}

impl DoUndoActions {
    /// Create a command with the given `name`, `do_action`, and `undo_action`.
    pub fn new(
        name: impl Into<String>,
        do_action: impl FnMut() + 'static,
        undo_action: impl FnMut() + 'static,
    ) -> Self {
        Self {
            name: name.into(),
            do_fn: Box::new(do_action),
            undo_fn: Box::new(undo_action),
        }
    }
}

impl UndoRedoCommand for DoUndoActions {
    fn name(&self) -> &str {
        &self.name
    }
    fn do_it(&mut self) {
        (self.do_fn)()
    }
    fn undo_it(&mut self) {
        (self.undo_fn)()
    }
}
