//! [`DrawCell`] and [`DrawRefCell`] — interior-mutable UI state that
//! automatically requests a repaint when it changes.
//!
//! # Why this exists
//!
//! The most common bug in this framework is mutating paint-affecting state
//! without calling [`crate::animation::request_draw`], so part of a widget
//! silently fails to repaint. The event dispatcher already makes a
//! `Consumed` event schedule a draw (see [`crate::event::EventResult`]), but
//! state shared between a widget and its owner via `Rc<Cell<T>>` /
//! `Rc<RefCell<T>>` is often written from a callback (an `on_change` /
//! `on_click` closure) that runs *outside* the widget's own `on_event`, so
//! the auto-invalidation doesn't cover it.
//!
//! [`DrawCell`] is the recommended replacement for `Cell<T>` in that role:
//! its [`set`](DrawCell::set) requests a draw **only when the value actually
//! changes**, so redundant writes stay free and callers no longer have to
//! remember a manual `request_draw()`.
//!
//! # Example
//!
//! ```
//! use std::rc::Rc;
//! use agg_gui::DrawCell;
//!
//! // Shared between a slider's `on_change` and the paint code.
//! let stroke_width = Rc::new(DrawCell::new(1.0_f64));
//!
//! let for_cb = Rc::clone(&stroke_width);
//! let on_change = move |v: f64| for_cb.set(v); // auto-requests a draw on change
//!
//! on_change(2.0);
//! assert_eq!(stroke_width.get(), 2.0);
//! ```

use crate::animation::request_draw;
use std::cell::{Cell, Ref, RefCell, RefMut};
use std::fmt;
use std::ops::{Deref, DerefMut};

/// A [`Cell`]-like container for `Copy + PartialEq` UI state that requests a
/// repaint whenever its value changes.
///
/// Drop-in for the `new` / `get` / `set` / `replace` surface of [`Cell`].
/// Writing the same value again is a no-op that does **not** schedule a draw,
/// so callers can `set` unconditionally without causing redundant frames.
pub struct DrawCell<T: Copy + PartialEq> {
    inner: Cell<T>,
}

impl<T: Copy + PartialEq> DrawCell<T> {
    /// Create a cell holding `value`.
    pub const fn new(value: T) -> Self {
        Self {
            inner: Cell::new(value),
        }
    }

    /// Read the current value (a copy).
    pub fn get(&self) -> T {
        self.inner.get()
    }

    /// Store `value`. Requests a repaint via
    /// [`crate::animation::request_draw`] only when `value` differs from the
    /// current contents, so no-op writes don't schedule a frame.
    pub fn set(&self, value: T) {
        if self.inner.get() != value {
            self.inner.set(value);
            request_draw();
        }
    }

    /// Store `value`, returning the previous contents. Requests a repaint
    /// only when the value actually changed.
    pub fn replace(&self, value: T) -> T {
        let old = self.inner.get();
        if old != value {
            self.inner.set(value);
            request_draw();
        }
        old
    }
}

impl<T: Copy + PartialEq + Default> Default for DrawCell<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

impl<T: Copy + PartialEq + fmt::Debug> fmt::Debug for DrawCell<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("DrawCell").field(&self.inner.get()).finish()
    }
}

impl<T: Copy + PartialEq> Clone for DrawCell<T> {
    fn clone(&self) -> Self {
        Self::new(self.inner.get())
    }
}

/// A [`RefCell`]-flavoured variant for state that is not `Copy` (strings,
/// vectors, structs). Reading is unchanged; a mutable borrow requests a
/// repaint when its guard drops.
///
/// # Caveat
///
/// Unlike [`DrawCell`], this cannot do change detection — it doesn't know
/// whether a `&mut` borrow actually modified anything — so **every**
/// [`borrow_mut`](DrawRefCell::borrow_mut) requests a draw on drop, even if
/// the caller changed nothing. Prefer [`DrawCell`] for `Copy` values where
/// redundant frames matter; reach for `DrawRefCell` when the payload can't be
/// `Copy` and you accept a repaint per mutable borrow.
pub struct DrawRefCell<T> {
    inner: RefCell<T>,
}

impl<T> DrawRefCell<T> {
    /// Create a cell holding `value`.
    pub const fn new(value: T) -> Self {
        Self {
            inner: RefCell::new(value),
        }
    }

    /// Immutably borrow the contents. Does not request a draw.
    pub fn borrow(&self) -> Ref<'_, T> {
        self.inner.borrow()
    }

    /// Mutably borrow the contents. The returned guard requests a repaint
    /// when it drops (see the type-level caveat about no change detection).
    pub fn borrow_mut(&self) -> DrawRefMut<'_, T> {
        DrawRefMut {
            inner: self.inner.borrow_mut(),
        }
    }
}

impl<T: Default> Default for DrawRefCell<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

impl<T: fmt::Debug> fmt::Debug for DrawRefCell<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("DrawRefCell").field(&self.inner).finish()
    }
}

/// Mutable-borrow guard from [`DrawRefCell::borrow_mut`] that requests a
/// repaint on drop.
pub struct DrawRefMut<'a, T> {
    inner: RefMut<'a, T>,
}

impl<T> Deref for DrawRefMut<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.inner
    }
}

impl<T> DerefMut for DrawRefMut<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.inner
    }
}

impl<T> Drop for DrawRefMut<'_, T> {
    fn drop(&mut self) {
        request_draw();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_to_new_value_requests_draw() {
        let cell = DrawCell::new(1_i32);
        crate::animation::clear_draw_request();
        cell.set(2);
        assert_eq!(cell.get(), 2);
        assert!(
            crate::animation::wants_draw(),
            "changing a DrawCell must request a draw"
        );
    }

    #[test]
    fn set_to_same_value_does_not_request_draw() {
        let cell = DrawCell::new(5_i32);
        crate::animation::clear_draw_request();
        cell.set(5);
        assert_eq!(cell.get(), 5);
        assert!(
            !crate::animation::wants_draw(),
            "a no-op write must NOT request a draw"
        );
    }

    #[test]
    fn replace_returns_old_and_requests_draw_only_on_change() {
        let cell = DrawCell::new(10_i32);

        crate::animation::clear_draw_request();
        let old = cell.replace(11);
        assert_eq!(old, 10);
        assert_eq!(cell.get(), 11);
        assert!(crate::animation::wants_draw());

        crate::animation::clear_draw_request();
        let old = cell.replace(11);
        assert_eq!(old, 11);
        assert!(!crate::animation::wants_draw(), "no-op replace stays quiet");
    }

    #[test]
    fn draw_ref_cell_requests_draw_on_mut_borrow_drop() {
        let cell = DrawRefCell::new(String::from("a"));

        // Read borrow: no draw.
        crate::animation::clear_draw_request();
        assert_eq!(&*cell.borrow(), "a");
        assert!(!crate::animation::wants_draw());

        // Mutable borrow: draw requested when the guard drops.
        crate::animation::clear_draw_request();
        {
            let mut g = cell.borrow_mut();
            g.push('b');
        }
        assert_eq!(&*cell.borrow(), "ab");
        assert!(crate::animation::wants_draw());
    }
}
