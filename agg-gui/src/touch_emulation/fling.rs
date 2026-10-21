//! The touch side of momentum scrolling: which lifts may fling, and catching
//! a running fling with a new finger.
//!
//! A `ScrollView` keeps scrolling after a finger flicks its content
//! (`widgets/scroll_view/fling.rs`). Two things about that need the touch
//! pipeline in [`crate::widget::App`], which this module carries between
//! them as thread-local state, like [`super::touch_pan_travel`]:
//!
//! - **Which release is a lift.** A touch pan's middle button is released
//!   when the finger lifts, but also when a second finger lands (the gesture
//!   becomes a pinch) and when the platform cancels the touch. Only the
//!   first throws the content: `App::on_touch_end` replays its commands
//!   inside [`while_lifting`], and a `ScrollView` flings only while
//!   [`lift_may_fling`] says so.
//! - **Catching.** A finger that lands on a view while it flings stops it
//!   there, as on every phone, and that touch is not a tap on whatever row
//!   happens to be under it (the user aimed at moving content). A flinging
//!   view registers its on-screen viewport ([`FlingRegistration`]);
//!   `App::on_touch_start` calls [`catch_at`] with the touch-down point, and
//!   when it lands on one, the view stops at its next layout (or at once, if
//!   the finger goes on to drag it) and the emulator turns the touch's tap
//!   and long press off (`TouchMouseEmu::catch_fling`). A finger elsewhere
//!   leaves the fling running and taps as usual.

use std::cell::{Cell, RefCell};

use crate::geometry::{Point, Rect};

thread_local! {
    static LIFTING: Cell<bool> = const { Cell::new(false) };
    static FLINGS: RefCell<Vec<Entry>> = const { RefCell::new(Vec::new()) };
    static NEXT_ID: Cell<u64> = const { Cell::new(1) };
}

/// One running fling: where its view shows on screen (logical, Y-up, the
/// space touches are hit-tested in) and whether a finger has caught it.
struct Entry {
    id: u64,
    rect: Rect,
    caught: bool,
}

/// Whether a touch pan's release being dispatched now is its finger lifting
/// (which may fling), rather than a second finger landing or a cancel.
pub fn lift_may_fling() -> bool {
    LIFTING.with(|l| l.get())
}

/// Run `f` (the replay of a finger's lift) with [`lift_may_fling`] true.
pub(crate) fn while_lifting<R>(f: impl FnOnce() -> R) -> R {
    LIFTING.with(|l| l.set(true));
    let result = f();
    LIFTING.with(|l| l.set(false));
    result
}

/// Whether any view is flinging on this thread.
pub fn any_flinging() -> bool {
    FLINGS.with(|f| !f.borrow().is_empty())
}

/// A finger landed at `pos` (logical, Y-up): every fling whose view is under
/// it is caught. Returns whether one was.
pub(crate) fn catch_at(pos: Point) -> bool {
    FLINGS.with(|f| {
        let mut caught = false;
        for entry in f.borrow_mut().iter_mut() {
            if entry.rect.contains(pos) {
                entry.caught = true;
                caught = true;
            }
        }
        caught
    })
}

/// A running fling's place in the registry; dropping it (the fling ended,
/// or its view went away) takes the fling out.
pub(crate) struct FlingRegistration {
    id: u64,
}

impl FlingRegistration {
    /// Register a fling of the view showing at `rect` on screen.
    pub(crate) fn new(rect: Rect) -> Self {
        let id = NEXT_ID.with(|n| {
            let id = n.get();
            n.set(id.wrapping_add(1));
            id
        });
        FLINGS.with(|f| {
            f.borrow_mut().push(Entry {
                id,
                rect,
                caught: false,
            })
        });
        Self { id }
    }

    /// Whether a finger has landed on the view since the fling began.
    pub(crate) fn caught(&self) -> bool {
        FLINGS.with(|f| {
            f.borrow()
                .iter()
                .any(|entry| entry.id == self.id && entry.caught)
        })
    }
}

impl Drop for FlingRegistration {
    fn drop(&mut self) {
        // `try_with`: a view dropped during thread teardown has nothing left
        // to take itself out of.
        let _ = FLINGS.try_with(|f| f.borrow_mut().retain(|entry| entry.id != self.id));
    }
}
