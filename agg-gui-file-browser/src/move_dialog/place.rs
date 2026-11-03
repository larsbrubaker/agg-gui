//! [`DialogPlace`]: where a picking dialog's controller (`SaveController`, `MoveController`)
//! opens its browser and what it remembers of where the user goes.
//!
//! A dialog opens in a starting folder the app gives (the website's Save As opens in the
//! design's own folder), else where it was last left under its location key (the config's
//! `BrowserMemory`), else at the browser's root. The first [`DialogPlace::update`] starts that
//! opening, so a controller's builder methods may come in any order. From then on each folder
//! the user moves to is remembered under the key, until the dialog is accepted (its folder then
//! handed over, never remembered as the dialog's place).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::memory::LocationFuture;
use crate::state::BrowserState;

/// A dialog's place (see the module header).
#[derive(Default)]
pub(crate) struct DialogPlace {
    key: RefCell<Option<String>>,
    start: RefCell<Option<LocationFuture>>,
    /// The browser's navigation number last seen; `None` until the first update.
    seen: Cell<Option<u64>>,
    /// Set once the dialog is accepted: from then on its moves are not remembered.
    pub accepted: Cell<bool>,
}

impl DialogPlace {
    /// Remembers the dialog's place under `key`, and opens there when no start is given.
    pub fn remember_under(&self, key: impl Into<String>) {
        *self.key.borrow_mut() = Some(key.into());
    }

    /// Opens the dialog in the folders `start` finds (below the root, in order).
    pub fn start_in(&self, start: LocationFuture) {
        *self.start.borrow_mut() = Some(start);
    }

    /// The first call opens the dialog's place; later ones remember a move by the user.
    pub fn update(&self, state: &Rc<RefCell<BrowserState>>) {
        let navigation = state.borrow().navigation();
        match self.seen.get() {
            None => {
                let start = self.start.borrow_mut().take();
                // The key is copied out: a restore's listeners may set it again.
                let key = self.key.borrow().clone();
                match (start, key) {
                    (Some(start), _) => open_found(state, start),
                    (None, Some(key)) => BrowserState::restore_location(state, &key),
                    (None, None) => {}
                }
            }
            Some(seen) if seen != navigation && !self.accepted.get() => {
                let key = self.key.borrow().clone();
                if let Some(key) = key {
                    BrowserState::remember_location(state, &key);
                }
            }
            Some(_) => {}
        }
        self.seen.set(Some(navigation));
    }
}

/// Opens the folders `found` finds below the root, once found (on the config's spawner), unless
/// the user has moved the browser meanwhile (their choice wins).
fn open_found(state: &Rc<RefCell<BrowserState>>, found: LocationFuture) {
    let (spawner, navigation) = {
        let s = state.borrow();
        (Rc::clone(&s.config().spawner), s.navigation())
    };
    let weak = Rc::downgrade(state);
    spawner.spawn_task(Box::pin(async move {
        let Some(folders) = found.await else {
            return;
        };
        let Some(state) = weak.upgrade() else {
            return;
        };
        let mut s = state.borrow_mut();
        if s.navigation() != navigation || s.depth() != 1 {
            return;
        }
        for folder in folders {
            s.push_container(folder);
        }
    }));
}
