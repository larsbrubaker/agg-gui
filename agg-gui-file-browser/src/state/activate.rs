//! Activating a row (a double click, or one tap where taps activate), the browser's config and
//! report, and its app slot. A folder opens. A browser whose double clicks go to its host
//! (`DoubleClickTarget::Host`: a save or open dialog) keeps a part's activation as an
//! [`ItemEvent`] for the host to take (`take_item_events`); otherwise it is queued as a snapshot
//! (`BrowserRef`, `Entry`) and handed to the config's `BrowserActions::activate` by `run_hooks`.
//! Every public way to activate a row (`activate`, `click`, `press`, `finish_tap`) takes the
//! shared state, queues under its borrow and runs the hooks once the borrow has ended, so a hook
//! may borrow the browser and no activation is left waiting; the queue-only forms are private.
//!
//! A finger has no double click, so a list can act on one tap instead, on a plain press's release
//! that did not drag (a finger drag that scrolls the list lets go of its press without a release,
//! and so acts on nothing). A list that activates on one tap (`tap_activates`, a phone's popup
//! browser) does it for any row. A list whose finger taps open folders
//! (`finger_tap_opens_folders`, dialogs on touch screens) does it only for a folder and only when
//! a finger made the press (`agg_gui::last_press_was_touch`), so a mouse keeps its click to select
//! and double click to open, and a part's tap, which in a dialog would set off its Save, only
//! selects it. Part of `state/mod.rs`, whose fields it reads.

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use crate::actions::Activation;
use crate::config::{BrowserConfig, ErrorReport};
use crate::entry::{BrowserRef, Entry};

use super::{BrowserEntry, BrowserState, ItemEvent};

impl BrowserState {
    /// The browser's config: its hooks and options.
    pub fn config(&self) -> &BrowserConfig {
        &self.config
    }

    /// Where the browser reports an error the user should see: the config's report, or without
    /// one the browser's status line (`status`). Called with the message alone, so it may be
    /// called while the state is borrowed.
    pub fn report(&self) -> ErrorReport {
        if let Some(report) = &self.config.report {
            return Rc::clone(report);
        }
        let status = Rc::clone(&self.status);
        Rc::new(move |message: &str| {
            *status.borrow_mut() = message.to_string();
            agg_gui::animation::request_draw();
        })
    }

    /// The app's own data for this browser, when it is a `T` (the value `with_config` was given
    /// or `set_app_slot` last put there).
    pub fn app_slot<T: 'static>(&self) -> Option<&T> {
        self.app.downcast_ref::<T>()
    }

    /// The app's own data for this browser, to change it, when it is a `T`.
    pub fn app_slot_mut<T: 'static>(&mut self) -> Option<&mut T> {
        self.app.downcast_mut::<T>()
    }

    /// The app's own data for this browser, to change it: what is there when it is a `T`, else
    /// what `make` gives, put in the slot in place of what was there. An app whose views all set
    /// their data still gets a value from a browser made without it.
    pub fn app_slot_or_insert_with<T: 'static>(&mut self, make: impl FnOnce() -> T) -> &mut T {
        if !self.app.is::<T>() {
            self.app = Box::new(make());
        }
        match self.app.downcast_mut::<T>() {
            Some(data) => data,
            // The slot holds a `T`: it was one, or was given one just above.
            None => unreachable!("the app slot holds the type it was just given"),
        }
    }

    /// Puts `data` in the app slot in place of what was there.
    pub fn set_app_slot<T: 'static>(&mut self, data: T) {
        self.app = Box::new(data) as Box<dyn Any>;
    }

    /// Makes one tap on a row activate it, as a double click does.
    pub fn set_tap_activates(&mut self, tap: bool) {
        self.tap_activates = tap;
        self.tap_pending = None;
    }

    /// Whether one tap on a row activates it.
    pub fn tap_activates(&self) -> bool {
        self.tap_activates
    }

    /// Whether the press on row `index` now being handled is a finger's on a folder that one tap
    /// opens.
    fn finger_tap_opens(&self, index: usize) -> bool {
        self.finger_tap_opens_folders
            && agg_gui::last_press_was_touch()
            && matches!(self.listed_entry(index), Some(BrowserEntry::Container(_)))
    }

    /// A plain left press on row `index` (no keys held): with taps activating (or this a finger's
    /// press on a folder whose tap opens it), its release activates the row unless it dragged.
    /// Any other press forgets an earlier one's row.
    pub fn begin_tap(&mut self, index: Option<usize>) {
        self.tap_pending = index.filter(|&i| self.tap_activates || self.finger_tap_opens(i));
    }

    /// Whether a press is waiting for its release to activate its row as a tap.
    pub fn tap_pending(&self) -> bool {
        self.tap_pending.is_some()
    }

    /// The press on row `index` ended; `activate` when it was a tap (no drag, not the item menu's
    /// hot spot), which then activates the row its press began on, the hooks running once
    /// `state`'s borrow has ended.
    pub fn finish_tap(state: &Rc<RefCell<Self>>, index: usize, activate: bool) {
        state.borrow_mut().finish_tap_in(index, activate);
        Self::run_hooks(state);
    }

    /// `finish_tap`'s queueing half.
    fn finish_tap_in(&mut self, index: usize, activate: bool) {
        if self.tap_pending.take() == Some(index) && activate {
            self.activate_in(index, Activation::Tap);
        }
    }

    /// A double click (or an activating tap) on the row at `index` (see the module header), the
    /// hooks running once `state`'s borrow has ended.
    pub fn activate(state: &Rc<RefCell<Self>>, index: usize, how: Activation) {
        state.borrow_mut().activate_in(index, how);
        Self::run_hooks(state);
    }

    /// `activate`'s queueing half: an item's activation waits for `run_hooks`. Kept private so
    /// no caller can queue one and leave it waiting.
    pub(super) fn activate_in(&mut self, index: usize, how: Activation) {
        match self.listed_entry(index) {
            Some(BrowserEntry::Container(link)) => self.open(link.as_ref()),
            Some(entry @ BrowserEntry::Item(_)) if self.item_events.is_some() => {
                if let Some(events) = self.item_events.as_mut() {
                    events.push(ItemEvent::Activated(entry));
                }
            }
            Some(BrowserEntry::Item(item)) => {
                let entry = Entry::new(item, self.path_names());
                let browser = self.snapshot();
                self.pending_activations.push((browser, entry, how));
            }
            None => {}
        }
    }

    /// The clicks and double clicks since the last ask, for a host that hears them
    /// (`DoubleClickTarget::Host`); always empty otherwise.
    pub fn take_item_events(&mut self) -> Vec<ItemEvent> {
        self.item_events
            .as_mut()
            .map(std::mem::take)
            .unwrap_or_default()
    }

    /// The browser as hooks are handed it: the open folder's path, place and writability, the
    /// selection, and a live handle back to this state.
    pub fn snapshot(&self) -> BrowserRef {
        let path: Rc<[String]> = self.path_names().into();
        let selection: Vec<Entry> = self
            .selected_entries()
            .iter()
            .map(|entry| entry.to_entry(Rc::clone(&path)))
            .collect();
        let active = self.active_container();
        let place = active.as_ref().and_then(|c| c.place());
        let writable = active.as_ref().is_some_and(|c| c.as_writable().is_some());
        BrowserRef::new(path, selection)
            .with_folder_place(place)
            .with_folder_writable(writable)
            .with_live(self.me.clone())
    }

    /// Hands the queued activations to the config's actions, with `state` not borrowed while
    /// they run. Every public way to activate a row ends here, so none waits.
    pub fn run_hooks(state: &Rc<RefCell<Self>>) {
        let (pending, actions) = {
            let mut state = state.borrow_mut();
            let pending = std::mem::take(&mut state.pending_activations);
            (pending, Rc::clone(&state.config.actions))
        };
        for (browser, entry, how) in pending {
            actions.activate(&browser, &entry, how);
        }
    }
}
