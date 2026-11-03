//! The browser's open path and moving along it: the folders from the root down to the showing
//! one, each shared ([`OpenFolder`]); opening a folder below (`open`, `push_container`), going
//! back up (`go_to`), showing where an item lives (`show_location`), handing the showing folder
//! over to a dialog's work (`take_active_container`) or replacing it (`replace_active_container`),
//! and remembering and restoring a dialog's place through the config's `BrowserMemory`
//! (`remember_location`, `restore_location`; the memory is handed a copy of the path's handles
//! with the state let go). Every change of the showing folder leaves the old one and listens to
//! the new (`state/active_watch.rs`), restores the folder's view (`state/view.rs`), bumps the
//! navigation counter and tells the container-changed listeners (how a search learns the user
//! walked away from its results). Part of `state/mod.rs`, whose fields it reads.

use std::cell::{Ref, RefCell, RefMut};
use std::rc::Rc;

use agg_gui_files::{tr, Folder, FolderLink, Item};

use crate::memory::remember_open_location;

use super::{BrowserState, ContainerChangedListener, OpenFolder};

impl BrowserState {
    /// The names of the folders from the root down (the breadcrumbs).
    pub fn path_names(&self) -> Vec<String> {
        self.path
            .iter()
            .map(|c| c.borrow().name().to_string())
            .collect()
    }

    /// Opens the folder. A link with nothing to give that acts instead (a sign-in) leaves the
    /// list where it is; any other refusal is said on the status line.
    pub fn open(&mut self, link: &dyn FolderLink) {
        match link.get_container() {
            Ok(folder) => self.push_container(folder),
            Err(_) if link.open_unavailable() => {}
            Err(e) => {
                self.set_status(e);
                self.changed();
            }
        }
    }

    /// Shows `folder` below the one showing, as opening its row would.
    pub fn set_active_container(&mut self, folder: Box<dyn Folder>) {
        self.push_container(folder);
    }

    /// Shows `folder` below the one showing.
    pub fn push_container(&mut self, folder: Box<dyn Folder>) {
        self.watch.leave(&self.path);
        self.show_below(folder);
    }

    /// Shows `folder` at the end of the path, the one showing already left: loaded here,
    /// activated and listened to for content changes.
    fn show_below(&mut self, mut folder: Box<dyn Folder>) {
        self.watch.listen(folder.as_ref());
        folder.load();
        folder.activate();
        self.path.push(Rc::new(RefCell::new(folder)));
        self.selected.clear();
        self.restore_user_view();
        self.changed();
        self.navigated();
    }

    /// Shows the folders `located` below the path's folder at `base` (a search's root, or the
    /// showing folder), the last of them showing: where "Open File Location" takes the user once
    /// the app has found where `item` lives. `None` (nowhere to go: a folder since renamed or
    /// deleted) is said on the status line, so the user learns why nothing happened.
    pub fn show_location(
        &mut self,
        item: &dyn Item,
        base: usize,
        located: Option<Vec<Box<dyn Folder>>>,
    ) {
        let Some(located) = located else {
            self.set_status(crate::move_dialog::plan::fill(
                &tr("Couldn't find the location of \"{0}\""),
                &[item.name()],
            ));
            self.changed();
            return;
        };
        if located.is_empty() && base + 1 == self.path.len() {
            return;
        }
        self.watch.leave(&self.path);
        self.path.truncate(base + 1);
        let count = located.len();
        for (i, mut folder) in located.into_iter().enumerate() {
            if i + 1 == count {
                self.watch.listen(folder.as_ref());
            }
            folder.load();
            if i + 1 == count {
                folder.activate();
            }
            self.path.push(Rc::new(RefCell::new(folder)));
        }
        if count == 0 {
            self.watch.enter(&self.path);
        }
        self.selected.clear();
        self.restore_user_view();
        self.changed();
        self.navigated();
    }

    /// `listener` hears each change of the showing folder, with the new one's id, until it
    /// answers false.
    pub fn subscribe_container_changed(&mut self, listener: ContainerChangedListener) {
        self.container_changed.push(listener);
    }

    /// How many times the showing folder has changed.
    pub fn navigation(&self) -> u64 {
        self.navigation
    }

    /// The showing folder changed: count it and tell the listeners.
    fn navigated(&mut self) {
        // The user went somewhere of their own: a restore under way is over.
        self.end_restore();
        self.navigation += 1;
        self.list_resets += 1;
        self.parked_selection.clear();
        self.select_when_listed.clear();
        if self
            .results
            .as_ref()
            .is_some_and(|(_, depth)| *depth >= self.path.len())
        {
            self.results = None;
        }
        self.keep_own_listing();
        let id = self.active_container_id().unwrap_or_default();
        let listeners = self.container_changed.clone();
        let gone: Vec<bool> = listeners.iter().map(|l| !l(&id)).collect();
        let mut index = 0;
        self.container_changed.retain(|_| {
            index += 1;
            !gone.get(index - 1).copied().unwrap_or(false)
        });
    }

    /// The showing folder's id.
    pub fn active_container_id(&self) -> Option<String> {
        self.active_container().map(|c| c.id().to_string())
    }

    /// The ids of the folders from the root down.
    pub fn path_ids(&self) -> Vec<String> {
        self.path
            .iter()
            .map(|c| c.borrow().id().to_string())
            .collect()
    }

    /// How many folders deep the browser is (1 at the root).
    pub fn depth(&self) -> usize {
        self.path.len()
    }

    /// The showing folder, borrowed for as long as the answer is held.
    pub fn active_container(&self) -> Option<Ref<'_, dyn Folder>> {
        self.path
            .last()
            .map(|c| Ref::map(c.borrow(), |b| b.as_ref()))
    }

    /// The root's folders, what a folder tree lists.
    pub fn root_child_containers(&self) -> Vec<Rc<dyn FolderLink>> {
        self.path
            .first()
            .map(|root| root.borrow().child_containers())
            .unwrap_or_default()
    }

    /// The breadcrumb for path entry `depth` (0 = the root): shows that folder again.
    pub fn go_to(&mut self, depth: usize) {
        if depth + 1 < self.path.len() {
            self.watch.leave(&self.path);
            self.path.truncate(depth + 1);
            self.watch.enter(&self.path);
            self.selected.clear();
            self.restore_user_view();
            self.changed();
            self.navigated();
        }
    }

    /// Takes the showing folder out of the path, the one above it showing again: a dialog that
    /// hands its folder to the work it closes on gives it away rather than opening a second copy
    /// of it. `None` at the root.
    pub fn take_active_container(&mut self) -> Option<OpenFolder> {
        if self.path.len() < 2 {
            return None;
        }
        self.watch.leave(&self.path);
        let taken = self.path.pop();
        self.watch.enter(&self.path);
        self.selected.clear();
        self.changed();
        self.navigated();
        taken
    }

    /// Shows `folder`, an open folder [`take_active_container`](Self::take_active_container)
    /// took, below the one showing again, as it was: a hand-over that couldn't complete puts the
    /// folder back rather than dropping it, so the browser stays in it.
    pub fn put_back_active_container(&mut self, folder: OpenFolder) {
        self.watch.leave(&self.path);
        self.path.push(folder);
        self.watch.enter(&self.path);
        self.selected.clear();
        self.restore_user_view();
        self.changed();
        self.navigated();
    }

    /// The folders from the root down to the showing one, each borrowed while it is held.
    pub fn path_containers(&self) -> impl Iterator<Item = Ref<'_, dyn Folder>> {
        self.path
            .iter()
            .map(|c| Ref::map(c.borrow(), |b| b.as_ref()))
    }

    /// The open path's shared folders, from the root down: what a hook is handed.
    pub fn open_path(&self) -> Vec<OpenFolder> {
        self.path.clone()
    }

    /// The folder at `depth` (0 = the root), to change it.
    pub fn container_at_mut(&mut self, depth: usize) -> Option<RefMut<'_, dyn Folder + 'static>> {
        self.path
            .get(depth)
            .map(|c| RefMut::map(c.borrow_mut(), |b| b.as_mut()))
    }

    /// The showing folder is replaced by what `sync` makes of it (a folder renamed from the
    /// tree). Nothing at the root. While a hook still holds the showing folder it can't be
    /// replaced: it stays, and the user is told through the report (the status line without the
    /// app's) that it shows as it was and how to see the change.
    pub fn replace_active_container(
        &mut self,
        sync: impl FnOnce(Box<dyn Folder>) -> Box<dyn Folder>,
    ) {
        if self.path.len() < 2 {
            return;
        }
        self.watch.leave(&self.path);
        let Some(old) = self.path.pop() else {
            return;
        };
        match Rc::try_unwrap(old) {
            Ok(old) => self.show_below(sync(old.into_inner())),
            Err(still_held) => {
                self.path.push(still_held);
                self.watch.enter(&self.path);
                let message = tr("This folder is busy. Open it again to see the change.");
                (self.report())(&message);
                self.changed();
            }
        }
    }

    /// Lends the config's memory the open path, the root first, to remember under `key`, with
    /// `state` not borrowed while it runs.
    pub fn remember_location(state: &Rc<RefCell<Self>>, key: &str) {
        let (memory, path) = {
            let s = state.borrow();
            (Rc::clone(&s.config.memory), s.open_path())
        };
        remember_open_location(memory.as_ref(), key, &path);
    }

    /// Opens the folder the config's memory remembers under `key`, once found (the search runs
    /// on the config's spawner): the folders below the root are opened down to it. Nothing
    /// happens when none is remembered, it can't be reached, or the user has moved the browser
    /// meanwhile (their choice wins). While it is found, a trail the memory names
    /// (`BrowserMemory::last_location_trail`) shows as the breadcrumbs over a loading line
    /// (`state/restore.rs`).
    pub fn restore_location(state: &Rc<RefCell<Self>>, key: &str) {
        let (memory, spawner) = {
            let s = state.borrow();
            let c = &s.config;
            (Rc::clone(&c.memory), Rc::clone(&c.spawner))
        };
        state.borrow_mut().restore_key = Some(key.to_string());
        let Some(finding) = memory.last_location(key) else {
            state.borrow_mut().end_restore();
            return;
        };
        let trail = memory.last_location_trail(key);
        let (serial, navigation) = {
            let mut s = state.borrow_mut();
            (s.begin_restore(trail), s.navigation)
        };
        let weak = Rc::downgrade(state);
        spawner.spawn_task(Box::pin(async move {
            let found = finding.await;
            let Some(state) = weak.upgrade() else {
                return;
            };
            let mut s = state.borrow_mut();
            let Some(keep) = s.finish_restore(serial) else {
                return;
            };
            let Some(folders) = found else {
                return;
            };
            if s.navigation != navigation || s.depth() != 1 {
                return;
            }
            let keep = keep.unwrap_or(folders.len());
            for folder in folders.into_iter().take(keep) {
                s.push_container(folder);
            }
        }));
    }
}
