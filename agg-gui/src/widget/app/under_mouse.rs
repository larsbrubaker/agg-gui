//! Where the pointer is in the widget tree: agg-sharp's `UnderMouseState`,
//! the hovered-chain queries on [`App`], and the thread's published snapshot
//! that widget code reads while it handles an event.
//!
//! The hovered chain (`hover_chain.rs`) is the root plus every widget on the
//! hover path. A widget on it is under the mouse; the deepest one is the
//! *first* under the mouse (the pointer is over its own surface, not a
//! child's) unless pointer capture holds and the pointer is off the captured
//! widget — then, as in agg-sharp's `OnMouseMoveWhenCaptured`, the captured
//! widget's ancestors stay under the mouse and nothing is first.
//!
//! [`App`] answers by path ([`App::under_mouse_state`]). Event handlers have
//! no `App` to ask, and agg-sharp's handlers read other widgets'
//! `UnderMouseState` while they run, so every chain update also publishes the
//! chain's widget identities ([`WidgetId`]) to a thread-local snapshot before
//! any enter/leave/over/out event is delivered; [`under_mouse_state_of`]
//! reads it. The snapshot is the last update made by any `App` on this
//! thread (one `App` per UI thread is the norm).

use std::cell::RefCell;

use super::path_anchor::{anchor_of, identity};
use crate::widget::{App, Widget};

/// agg-sharp's `UnderMouseState`: where a widget stands relative to the
/// pointer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UnderMouseState {
    /// The pointer is not over the widget (or it is not on the hovered chain).
    #[default]
    NotUnderMouse,
    /// The pointer is over the widget, but over one of its children (or it
    /// is an ancestor of the captured widget).
    UnderMouseNotFirst,
    /// The pointer is over the widget's own surface: it is the deepest widget
    /// on the hovered chain.
    FirstUnderMouse,
}

/// A widget's identity: the address of its data, stable while the widget
/// lives (children are boxed, so reordering them never moves the widget).
/// Take it once the widget is boxed — moving an unboxed widget changes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WidgetId(usize);

impl WidgetId {
    /// The identity of `widget`.
    pub fn of(widget: &dyn Widget) -> Self {
        Self(identity(widget))
    }
}

/// The hovered chain as last published on this thread.
#[derive(Default)]
struct Snapshot {
    /// Identities from the root down to the deepest hovered widget.
    chain: Vec<usize>,
    /// Whether the deepest entry is first under the mouse.
    has_first: bool,
}

thread_local! {
    static SNAPSHOT: RefCell<Snapshot> = RefCell::new(Snapshot::default());
}

/// The state of `chain_index` in a chain of `chain_len` entries whose
/// deepest entry is first when `has_first`.
fn state_at(chain_index: usize, chain_len: usize, has_first: bool) -> UnderMouseState {
    if chain_index + 1 == chain_len && has_first {
        UnderMouseState::FirstUnderMouse
    } else {
        UnderMouseState::UnderMouseNotFirst
    }
}

/// The under-mouse state of the widget `id` as this thread's `App` last
/// announced it. Inside an enter/leave/over/out handler it is already the
/// state after the change being announced, for every widget.
pub fn under_mouse_state_of(id: WidgetId) -> UnderMouseState {
    SNAPSHOT.with(|s| {
        let s = s.borrow();
        s.chain
            .iter()
            .position(|&w| w == id.0)
            .map_or(UnderMouseState::NotUnderMouse, |i| {
                state_at(i, s.chain.len(), s.has_first)
            })
    })
}

impl App {
    /// The hovered chain: the path (from the root; the root itself is the
    /// empty path) of the deepest widget the pointer is over, as last
    /// announced with `MouseEnter`/`MouseLeave`. `None` when the pointer is
    /// over nothing. Every prefix of the path is under the mouse too.
    pub fn hovered_chain(&self) -> Option<&[usize]> {
        self.hover_chain.as_deref()
    }

    /// The path of the widget first under the mouse (agg-sharp's
    /// `FirstWidgetUnderMouse`), if any.
    pub fn first_under_mouse(&self) -> Option<&[usize]> {
        self.hover_chain.as_deref().filter(|_| self.hover_first)
    }

    /// The under-mouse state of the widget at `path` (agg-sharp's
    /// `UnderMouseState`).
    pub fn under_mouse_state(&self, path: &[usize]) -> UnderMouseState {
        match self.hover_chain.as_deref() {
            Some(chain) if chain.starts_with(path) => {
                state_at(path.len(), chain.len() + 1, self.hover_first)
            }
            _ => UnderMouseState::NotUnderMouse,
        }
    }

    /// The path of the widget holding pointer capture (agg-sharp's
    /// `MouseCaptured`; an ancestor of it has `ChildHasMouseCaptured`).
    pub fn captured_path(&self) -> Option<&[usize]> {
        self.captured.as_deref()
    }

    /// Publish the chain `chain` (first under the mouse when `has_first`) for
    /// [`under_mouse_state_of`]. Called before its events are delivered.
    pub(super) fn publish_under_mouse(&self, chain: Option<&[usize]>, has_first: bool) {
        let ids = match chain {
            Some(path) => {
                let mut ids = vec![identity(self.root.as_ref())];
                ids.extend(anchor_of(self.root.as_ref(), Some(path)));
                ids
            }
            None => Vec::new(),
        };
        SNAPSHOT.with(|s| {
            *s.borrow_mut() = Snapshot {
                chain: ids,
                has_first: has_first && chain.is_some(),
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{state_at, UnderMouseState};

    #[test]
    fn only_the_deepest_entry_can_be_first() {
        assert_eq!(state_at(2, 3, true), UnderMouseState::FirstUnderMouse);
        assert_eq!(state_at(1, 3, true), UnderMouseState::UnderMouseNotFirst);
        assert_eq!(state_at(2, 3, false), UnderMouseState::UnderMouseNotFirst);
    }
}
