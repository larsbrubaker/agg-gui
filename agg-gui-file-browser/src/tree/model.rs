//! The folder tree's rows as data ([`TreeModel`]): which folders are listed, in display order,
//! which are open, and which have an expand arrow. The tree widget (`mod.rs`, `row.rs`) draws
//! them and turns clicks into calls here; an app keeps one `TreeModel` across its rebuilds of the
//! tree so folders stay open.
//!
//! A row has an arrow only while its folder may have folders of its own: an arrow promises
//! something to open, and on a folder with none it leads a first-time user to an empty expansion.
//! Asking only where that is cheap keeps the tree from waking drives, opening archives or reaching
//! the network as rows are drawn. A folder that lists locally (an app's built-in folders, folders
//! on disk or in memory, zip files) is asked when its row is made (the quick look,
//! `FolderLink::has_child_containers`), so a folder with none never shows one. A folder listed
//! over the network or behind a sign-in keeps its arrow until it is first opened; when that
//! opening finds no folders the arrow goes, and stays gone for the session (`childless`, by the
//! folder's keys from the root, as `relist` knows folders).
//!
//! Opening a folder whose listing is still on its way keeps the folder and listens for it to land
//! (`PendingListing`), with one "Loading..." row under it meanwhile; the listing's rows then
//! replace it, or the row folds and loses its arrow, so an open arrow never stays over nothing. A
//! listing that failed (offline, refused) says nothing about the folder: the row folds and keeps
//! its arrow, and nothing is remembered. A folder already waiting for its listing never starts a
//! second one: reopening it after a re-list puts its "Loading..." row back and keeps waiting.
//!
//! Rows are told apart by where their folders are (`place.rs`): by location key where the folder
//! answers one, any other folder by its name. So the row the list is showing, the folders reopened
//! after a re-list and the folders known to be empty are each that folder, never a same-named
//! sibling.
//!
//! The model follows the browser ([`TreeModel::sync_to`], at each layout of the tree): after a
//! write (`BrowserState::writes`) it lists itself again from the root, keeping open what was open;
//! when the browser moves it opens down to the showing folder; when the showing folder's content
//! changes its row takes the folder's name and, when open, lists its folders again.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use agg_gui_files::{ContentChangedListener, Folder, FolderLink, Item, MessageItem};

use super::place::{path_keys, PathLevel, TreeKey, TreePlace};
use crate::state::BrowserState;

/// One folder in the tree.
pub struct TreeNode {
    name: String,
    micro_icon: Option<String>,
    depth: usize,
    /// The links from the root down to this folder, this one last.
    path: Vec<Rc<dyn FolderLink>>,
    expanded: bool,
    /// Whether the folder has visible folders of its own; `None` until known (a folder listed
    /// over the network before it is opened).
    has_children: Option<bool>,
    /// Its folders could not be known before opening it, so finding none is remembered.
    lazy: bool,
    /// The "Loading..." row under a folder whose listing is on its way: it stands for no
    /// folder, so it cannot be opened, shown, dragged onto or given a menu. Its `path` is the
    /// loading folder's.
    placeholder: bool,
}

impl TreeNode {
    /// The row draws an expand arrow: always while open, and while folded unless the folder is
    /// known to have no folders.
    pub fn has_arrow(&self) -> bool {
        !self.placeholder && (self.expanded || self.has_children != Some(false))
    }

    /// The folder's name as the row shows it.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The icon key the folder's link names for its row (`FolderLink::micro_icon`).
    pub fn micro_icon(&self) -> Option<&str> {
        self.micro_icon.as_deref()
    }

    /// How many levels below the root's rows the row is (0 for a root row).
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// The links from the root down to this folder, this one last (a "Loading..." row's are its
    /// folder's).
    pub fn path(&self) -> &[Rc<dyn FolderLink>] {
        &self.path
    }

    /// Whether the folder is open, its folders listed under it.
    pub fn is_expanded(&self) -> bool {
        self.expanded
    }

    /// Whether this is the "Loading..." row under a folder whose listing is on its way.
    pub fn is_placeholder(&self) -> bool {
        self.placeholder
    }
}

/// An opened folder whose listing was still on its way: the container is kept so the listing can
/// land, and listened to so the tree wakes when it does.
struct PendingListing {
    keys: Vec<TreeKey>,
    container: Box<dyn Folder>,
    listener: ContentChangedListener,
}

impl Drop for PendingListing {
    fn drop(&mut self) {
        self.container.unsubscribe_content_changed(&self.listener);
    }
}

/// The tree's rows in display order, kept by the browser across its rebuilds so folders stay
/// open.
#[derive(Default)]
pub struct TreeModel {
    nodes: Vec<TreeNode>,
    generation: u64,
    /// The browser navigation the tree last opened down to (`expand_to`).
    synced_navigation: Option<u64>,
    /// The browser's writes the tree last listed its folders after.
    synced_writes: u64,
    /// The showing container's content changes the tree last answered (`content_changed`).
    synced_content: u64,
    /// The row a right press asked the folder menu for.
    menu_request: Option<usize>,
    /// The folders, by their keys from the root, that only opening could tell about and that
    /// turned out to have no folders. Only looked up, never walked, so its order reaches nothing.
    childless: HashSet<Vec<TreeKey>>,
    /// Opened folders whose listings are still on their way.
    pending: Vec<PendingListing>,
}

impl TreeModel {
    /// The root's folders `state` started at, ordered by name (ordinal, stable, so folders with
    /// equal names keep the order the root gave them).
    pub fn for_state(state: &Rc<RefCell<BrowserState>>) -> Rc<RefCell<Self>> {
        let state = state.borrow();
        let mut model = Self {
            synced_writes: state.writes(),
            synced_content: state.content_changes(),
            ..Self::default()
        };
        model.nodes = model.root_nodes(&state);
        Rc::new(RefCell::new(model))
    }

    /// The rows in display order.
    pub fn rows(&self) -> &[TreeNode] {
        &self.nodes
    }

    /// Each row's name indented two spaces per level, as the tree's `rows` property shows them.
    pub fn indented_names(&self) -> Vec<String> {
        self.nodes
            .iter()
            .map(|n| format!("{}{}", "  ".repeat(n.depth), n.name))
            .collect()
    }

    /// Changes whenever the rows do: the tree builds its row widgets again.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// How many opened folders are still waiting for their listings.
    pub fn waiting_listings(&self) -> usize {
        self.pending.len()
    }

    /// The links from the root down to row `index`'s folder, that folder's last.
    pub fn node_path(&self, index: usize) -> Option<Vec<Rc<dyn FolderLink>>> {
        self.nodes.get(index).map(|node| node.path.clone())
    }

    /// A right press on row `index` asks for its folder menu.
    pub(crate) fn request_menu(&mut self, index: usize) {
        self.menu_request = Some(index);
    }

    /// The row whose folder menu was asked for since the last ask, clearing it.
    pub fn take_menu_request(&mut self) -> Option<usize> {
        self.menu_request.take()
    }

    /// Lists the tree again from the root, opening the folders that were open (by their keys
    /// from the root down), then down to the browser's folder again.
    pub fn relist(&mut self, state: &BrowserState) {
        let open: Vec<Vec<TreeKey>> = self
            .nodes
            .iter()
            .filter(|node| node.expanded && !node.placeholder)
            .map(node_keys)
            .collect();
        self.pending.clear();
        self.nodes = self.root_nodes(state);
        let mut index = 0;
        while index < self.nodes.len() {
            if open.contains(&node_keys(&self.nodes[index])) {
                self.toggle(index, false);
            }
            index += 1;
        }
        self.synced_navigation = None;
        self.generation += 1;
    }

    /// Opens the folder once to list its folders under it, or folds them away again. `by_user` is
    /// a click on the node's arrow: a folder with nothing to give that acts instead (a cloud
    /// folder's sign-in while signed out, `FolderLink::open_unavailable`) acts then and stays
    /// folded, since it has no folders to show yet; a restore after a rebuild never prompts. A
    /// folder that turns out to have no folders stays folded and loses its arrow. A folder whose
    /// listing is already on its way opens to its "Loading..." row again without a second one.
    pub fn toggle(&mut self, index: usize, by_user: bool) {
        // A "Loading..." row stands for no folder: it shares its folder's keys, so a re-list's
        // reopening walk would otherwise open it as that folder, row after row.
        let Some(node) = self.nodes.get_mut(index).filter(|node| !node.placeholder) else {
            return;
        };
        if node.expanded {
            node.expanded = false;
            let keys = node_keys(node);
            let end = self.children_end(index);
            self.nodes.drain(index + 1..end);
            self.pending.retain(|p| !p.keys.starts_with(&keys));
            self.generation += 1;
            return;
        }
        let Some(link) = node.path.last().cloned() else {
            return;
        };
        let keys = node_keys(node);
        if self.pending.iter().any(|p| p.keys == keys) {
            node.expanded = true;
            let loading = loading_row(node);
            self.nodes.insert(index + 1, loading);
            self.generation += 1;
            return;
        }
        let mut container = match link.get_container() {
            Ok(container) => container,
            // A folder that cannot be opened now (signed out) is not known to have no folders:
            // it stays folded and keeps its arrow.
            Err(_) => {
                if by_user {
                    link.open_unavailable();
                }
                return;
            }
        };
        // Listened to before the load starts, so a listing that lands during `load` is not missed.
        let listener: ContentChangedListener = Rc::new(agg_gui::animation::request_draw);
        container.subscribe_content_changed(Rc::clone(&listener));
        container.load();
        if container.is_load_pending() {
            node.expanded = true;
            let loading = loading_row(node);
            self.nodes.insert(index + 1, loading);
            self.pending.push(PendingListing {
                keys,
                container,
                listener,
            });
            self.generation += 1;
            return;
        }
        container.unsubscribe_content_changed(&listener);
        self.land(index, container.as_ref());
    }

    /// Lists under row `index` what `container` listed, or, when that listing failed, folds the
    /// row and leaves its arrow (a failure is not an empty folder, so nothing is remembered).
    fn land(&mut self, index: usize, container: &dyn Folder) {
        if !container.last_load_failed() {
            self.fill(index, container.child_containers());
            return;
        }
        let end = self.children_end(index);
        self.nodes.drain(index + 1..end);
        let node = &mut self.nodes[index];
        node.expanded = false;
        node.has_children = None;
        let keys = node_keys(node);
        self.pending.retain(|p| p.keys != keys);
        self.generation += 1;
    }

    /// Lists `links`' visible folders under row `index`, open; or, with none, leaves it folded
    /// without its arrow (remembered when only opening could tell). A folder under it that was
    /// open stays open, listing what it holds now, so a change in the folder (a drop into one of
    /// its folders) doesn't fold what the user opened.
    fn fill(&mut self, index: usize, links: Vec<Rc<dyn FolderLink>>) {
        let end = self.children_end(index);
        let node = &self.nodes[index];
        let (path, depth) = (node.path.clone(), node.depth);
        let links: Vec<_> = links.into_iter().filter(|link| link.is_visible()).collect();
        // The folders under this row that were open, by their keys from the root (the folder
        // itself, not a same-named sibling), to open again (listed fresh) once the new rows are
        // in.
        let open: Vec<Vec<TreeKey>> = self.nodes[index + 1..end]
            .iter()
            .filter(|node| node.expanded && !node.placeholder)
            .map(node_keys)
            .collect();
        let rows: Vec<TreeNode> = links
            .into_iter()
            .map(|link| self.node_for(link, &path, depth + 1))
            .collect();
        let node = &mut self.nodes[index];
        node.has_children = Some(!rows.is_empty());
        node.expanded = !rows.is_empty();
        let keys = node_keys(node);
        if !rows.is_empty() {
            self.childless.remove(&keys);
        } else if node.lazy {
            self.childless.insert(keys.clone());
        }
        // A listing that landed another way (the browser's) answers this folder's own wait too.
        self.pending.retain(|p| p.keys != keys);
        self.nodes.splice(index + 1..end, rows);
        self.generation += 1;
        // Each folder that was open opens again, listing what it holds now; opening one inserts
        // its rows right after it, which the walk then reaches. One still waiting for its listing
        // shows its "Loading..." row again and keeps waiting (`toggle`).
        let mut at = index + 1;
        while at < self.children_end(index) {
            if !self.nodes[at].expanded && open.contains(&node_keys(&self.nodes[at])) {
                self.toggle(at, false);
            }
            at += 1;
        }
        // A folder that was waiting under this row and is no longer listed waits for nothing.
        let nodes = &self.nodes;
        self.pending.retain(|p| {
            !p.keys.starts_with(&keys)
                || nodes
                    .iter()
                    .any(|node| !node.placeholder && node.expanded && node_keys(node) == p.keys)
        });
    }

    /// The end of row `index`'s rows below it (the first row not deeper).
    fn children_end(&self, index: usize) -> usize {
        let depth = self.nodes[index].depth;
        (index + 1..self.nodes.len())
            .find(|&i| self.nodes[i].depth <= depth)
            .unwrap_or(self.nodes.len())
    }

    /// Lists the folders of each opened folder whose listing has landed since the last look.
    /// Where the landed folder is on the way down to the browser's folder, the walk down to it
    /// (which stopped there) runs again; any other landing leaves folded folders folded.
    fn land_pending(&mut self, state: &BrowserState) {
        let mut at = 0;
        while at < self.pending.len() {
            if self.pending[at].container.is_load_pending() {
                at += 1;
                continue;
            }
            let landed = self.pending.remove(at);
            let index = self.nodes.iter().position(|node| {
                !node.placeholder && node.expanded && node_keys(node) == landed.keys
            });
            if let Some(index) = index {
                self.land(index, landed.container.as_ref());
                let levels = state.path_levels();
                let on_the_way = landed.keys.len() <= levels.len()
                    && landed
                        .keys
                        .iter()
                        .zip(&levels)
                        .all(|(key, (name, place))| key_stands_for(key, name, place.as_ref()));
                if on_the_way {
                    self.synced_navigation = None;
                }
            }
        }
    }
}

impl TreeModel {
    /// Opens the tree down to the folder the list shows, `levels` being the browser's path below
    /// the root (`BrowserState::path_levels`). Walked by place and name (`node_stands_for`), and
    /// only the nodes on the way down are opened: the folder's ancestors, so the folder's row is
    /// visible, not the folder itself. Where the tree
    /// does not reach (a search's results, a hidden provider) the walk stops there.
    pub fn expand_to(&mut self, levels: &[PathLevel]) {
        self.walk(levels, true);
    }

    /// The row standing for the folder the list shows (`levels` from
    /// `BrowserState::path_levels`), the one drawn selected: `None` at the root, where the tree
    /// does not reach, or while a folder on the way is folded.
    pub fn showing_row(&self, levels: &[PathLevel]) -> Option<usize> {
        let mut parent: Option<usize> = None;
        for (level, (name, place)) in levels.iter().enumerate() {
            let index = self.child_standing_for(parent, name, place.as_ref())?;
            if level + 1 < levels.len() && !self.nodes[index].expanded {
                return None;
            }
            parent = Some(index);
        }
        parent
    }

    /// The row directly under `parent` (the root's rows for `None`) standing for the folder
    /// `name` at `place`.
    fn child_standing_for(
        &self,
        parent: Option<usize>,
        name: &str,
        place: Option<&TreePlace>,
    ) -> Option<usize> {
        let (start, depth) = match parent {
            Some(index) => (index + 1, self.nodes[index].depth + 1),
            None => (0, 0),
        };
        (start..self.nodes.len())
            .take_while(|&i| self.nodes[i].depth >= depth)
            .filter(|&i| self.nodes[i].depth == depth)
            .find(|&i| node_stands_for(&self.nodes[i], name, place))
    }

    /// The row standing for the folder at `levels` below the root, walked down by place and name
    /// (`node_stands_for`); `open` opens each folder on the way that is not open yet (not the
    /// last), else the walk stops at one that is not. `None` where the tree does not reach.
    fn walk(&mut self, levels: &[PathLevel], open: bool) -> Option<usize> {
        let mut parent: Option<usize> = None;
        for (level, (name, place)) in levels.iter().enumerate() {
            let index = self.child_standing_for(parent, name, place.as_ref())?;
            if level + 1 < levels.len() && !self.nodes[index].expanded {
                if !open {
                    return None;
                }
                self.toggle(index, false);
            }
            parent = Some(index);
        }
        parent
    }

    /// The showing container's content changed (a file added to a watched folder, a cloud folder
    /// listed again). Its row takes the container's name, and when open lists the container's
    /// folders under it again, each one that was open opened again and listed fresh. The tree
    /// holds links, not containers, so it finds the row as `expand_to` does, by place and name.
    /// The root has no row. Whether the row has an arrow follows the new listing, open or
    /// folded, once it has landed.
    pub fn content_changed(&mut self, state: &BrowserState) {
        let Some(container) = state.active_container() else {
            return;
        };
        let levels = state.path_levels();
        if levels.is_empty() {
            return;
        }
        let Some(index) = self.walk(&levels, false) else {
            return;
        };
        let name = container.name();
        if self.nodes[index].name != name {
            self.nodes[index].name = name.to_string();
            self.generation += 1;
        }
        if container.is_load_pending() {
            return;
        }
        if self.nodes[index].expanded {
            self.land(index, &*container);
            return;
        }
        if container.last_load_failed() {
            return;
        }
        let links = container.child_containers();
        let has_children = links.iter().any(|link| link.is_visible());
        let node = &mut self.nodes[index];
        if node.has_children != Some(has_children) {
            node.has_children = Some(has_children);
            let keys = node_keys(node);
            if has_children {
                self.childless.remove(&keys);
            } else if node.lazy {
                self.childless.insert(keys);
            }
            self.generation += 1;
        }
    }

    /// The link of the row drawn selected beside `state` (`showing_row`, as the tree draws it):
    /// an app reaches its own folder type through the link's `as_any`.
    pub fn selected_link(&self, state: &BrowserState) -> Option<Rc<dyn FolderLink>> {
        self.showing_row(&state.path_levels())
            .and_then(|index| self.nodes[index].path.last().cloned())
    }

    /// Opens the tree down to `state`'s folder when the browser has moved since the last time,
    /// and lists the opened folders whose listings have landed.
    pub fn sync_to(&mut self, state: &BrowserState) {
        if self.synced_writes != state.writes() {
            self.synced_writes = state.writes();
            self.relist(state);
        }
        self.land_pending(state);
        let navigation = state.navigation();
        if self.synced_navigation != Some(navigation) {
            self.synced_navigation = Some(navigation);
            self.expand_to(&state.path_levels());
        }
        let content = state.content_changes();
        if self.synced_content != content {
            self.synced_content = content;
            self.content_changed(state);
        }
    }

    /// The root's visible folders, ordered by name (ordinal, stable).
    fn root_nodes(&self, state: &BrowserState) -> Vec<TreeNode> {
        let mut links = state.root_child_containers();
        links.retain(|link| link.is_visible());
        links.sort_by(|a, b| a.name().cmp(b.name()));
        links
            .into_iter()
            .map(|link| self.node_for(link, &[], 0))
            .collect()
    }

    /// A folded row for `link` under `parents`, asking the link whether it has folders unless
    /// opening it already found none.
    fn node_for(
        &self,
        link: Rc<dyn FolderLink>,
        parents: &[Rc<dyn FolderLink>],
        depth: usize,
    ) -> TreeNode {
        let mut path = parents.to_vec();
        let name = link.name().to_string();
        let micro_icon = link.micro_icon().map(str::to_string);
        path.push(link);
        let (has_children, lazy) = if self.childless.contains(&path_keys(&path)) {
            (Some(false), true)
        } else {
            let known = path.last().and_then(|link| link.has_child_containers());
            (known, known.is_none())
        };
        TreeNode {
            name,
            micro_icon,
            depth,
            path,
            expanded: false,
            has_children,
            lazy,
            placeholder: false,
        }
    }
}

/// The "Loading..." row shown under `parent` while its listing is on its way.
fn loading_row(parent: &TreeNode) -> TreeNode {
    TreeNode {
        name: MessageItem::loading().name().to_string(),
        micro_icon: None,
        depth: parent.depth + 1,
        path: parent.path.clone(),
        expanded: false,
        has_children: Some(false),
        lazy: false,
        placeholder: true,
    }
}

/// Whether a row stands for the folder `name` at `place`: by place for a row on disk or in an
/// archive (the one thing about such a folder that cannot be two folders; such a row never stands
/// for a folder that is not there), else by name, ignoring case.
fn node_stands_for(node: &TreeNode, name: &str, place: Option<&TreePlace>) -> bool {
    if node.placeholder {
        return false;
    }
    match node
        .path
        .last()
        .and_then(|l| TreePlace::of_link(l.as_ref()))
    {
        Some(row_place) => place == Some(&row_place),
        None => names_match(&node.name, name),
    }
}

/// [`node_stands_for`] for one folder on a row's way down, by its key.
fn key_stands_for(key: &TreeKey, name: &str, place: Option<&TreePlace>) -> bool {
    match key {
        TreeKey::Place(row_place) => place == Some(row_place),
        TreeKey::Name(row_name) => names_match(row_name, name),
    }
}

fn names_match(row_name: &str, name: &str) -> bool {
    !name.is_empty() && row_name.to_lowercase() == name.to_lowercase()
}

/// A row's folder keys from the root down, what the tree knows a folder by between listings.
fn node_keys(node: &TreeNode) -> Vec<TreeKey> {
    path_keys(&node.path)
}
