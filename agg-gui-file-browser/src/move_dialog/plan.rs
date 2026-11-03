//! What a Move does with its items, given the Move dialog's browser as it stands
//! ([`move_verdict`]): the dialog asks it to say so and to name its button, and its accept opens
//! the folder it names ([`destination`]).
//!
//! The destination is the one folder selected in the dialog's list when exactly one is, otherwise
//! the folder the dialog shows. Whether the items move or copy is the drop's rule, which the
//! request carries (`MoveRequest::copies_to`): within one place they move; between places, or
//! out of a folder that keeps its items, they copy and the originals stay. A folder that can't
//! take them is refused with the reason in plain words: a read-only folder or the browser's top
//! ("Choose a folder you can add to."), the items' own folder ("... is already in ..."), and one
//! of the folders being moved or a folder inside one ("... can't go inside itself."). Folders are
//! told apart by the request's `PlaceRules::folder_key` and `open_folder_key` (by default their
//! place and location key, [`FolderKey`]); a folder is inside a moved one
//! when the moved folder is on the dialog's open path, since the dialog reaches every folder from
//! its root down. A copy lands only in a folder some place holds. Names are filled into the
//! translated lines in one pass ([`fill`]), so a name holding "{1}" reads as it is.

use std::rc::Rc;

use agg_gui_files::{tr, Folder, FolderLink, PlaceId};

use crate::dialogs::MoveRequest;
use crate::state::{BrowserEntry, BrowserState};

/// A folder as Move tells folders apart: its place and its location key (a path on disk), or
/// any text an app's `PlaceRules::folder_key` names it by (a server ID).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FolderKey {
    place: Option<PlaceId>,
    location: String,
}

impl FolderKey {
    /// The key of a folder in `place` that `location` names, unique within the place.
    pub fn new(place: Option<PlaceId>, location: impl Into<String>) -> Self {
        Self {
            place,
            location: location.into(),
        }
    }

    /// The key of the folder `link` opens, when it has a location key.
    pub fn of_link(link: &dyn FolderLink) -> Option<Self> {
        Some(Self {
            place: link.place(),
            location: link.location_key()?,
        })
    }

    /// The key of the open `folder`, when it has a location key.
    pub fn of_folder(folder: &dyn Folder) -> Option<Self> {
        Some(Self {
            place: folder.place(),
            location: folder.location_key()?,
        })
    }

    /// The place the folder is in.
    pub fn place(&self) -> Option<&PlaceId> {
        self.place.as_ref()
    }

    /// The text naming the folder within its place (a path on disk, a server ID).
    pub fn location(&self) -> &str {
        &self.location
    }
}

/// What accepting the Move dialog would do, with the line saying so.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MoveVerdict {
    /// The items move.
    Move(String),
    /// The items are copied and the originals stay.
    Copy(String),
    /// The folder can't take the items; why.
    Refused(String),
}

impl MoveVerdict {
    /// The line the dialog shows.
    pub fn text(&self) -> &str {
        match self {
            MoveVerdict::Move(text) | MoveVerdict::Copy(text) | MoveVerdict::Refused(text) => text,
        }
    }

    /// The accept button's word, translated: "Copy" for a copy, else "Move".
    pub fn verb(&self) -> String {
        match self {
            MoveVerdict::Copy(_) => tr("Copy"),
            _ => tr("Move"),
        }
    }

    /// Whether accepting does anything.
    pub fn is_refused(&self) -> bool {
        matches!(self, MoveVerdict::Refused(_))
    }
}

/// Where the dialog would put the items.
pub enum Destination {
    /// The one folder selected in the dialog's list.
    Row(Rc<dyn FolderLink>),
    /// The folder the dialog shows.
    Showing,
}

/// The dialog's destination now.
pub fn destination(dialog: &BrowserState) -> Destination {
    match dialog.selected_entries().as_slice() {
        [BrowserEntry::Container(link)] => Destination::Row(Rc::clone(link)),
        _ => Destination::Showing,
    }
}

/// `template` with `{0}`, `{1}`... replaced by `values` in one pass.
pub fn fill(template: &str, values: &[&str]) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let index = after
            .find('}')
            .and_then(|close| Some((close, after[..close].parse::<usize>().ok()?)));
        match index.and_then(|(close, i)| Some((close, *values.get(i)?))) {
            Some((close, value)) => {
                out.push_str(value);
                rest = &after[close + 1..];
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

fn cant_add() -> MoveVerdict {
    MoveVerdict::Refused(tr("Choose a folder you can add to."))
}

/// Why the folder `name` (one of the items) can't take them.
fn into_itself(name: &str) -> String {
    fill(
        &tr("{0} can't go inside itself. Choose another folder."),
        &[name],
    )
}

/// `one` filled with the item's name for one item, else `many` with the count, then `rest` as
/// `{1}`, `{2}`... (both already translated).
fn what(request: &MoveRequest, one: &str, many: &str, rest: &[&str]) -> String {
    let (template, first) = match request.items.as_slice() {
        [item] => (one, item.name().to_string()),
        items => (many, items.len().to_string()),
    };
    let mut values = vec![first.as_str()];
    values.extend_from_slice(rest);
    fill(template, &values)
}

/// Why the folder at `key`, below the dialog's open folders `path`, can't take the items: it is
/// their own folder, or one of them or inside one (an open folder or `key` itself, by the
/// request's `PlaceRules::folder_inside`).
fn refusal(request: &MoveRequest, key: Option<&FolderKey>, path: &[FolderKey]) -> Option<String> {
    if key.is_some() && key == request.source_key.as_ref() {
        let source = request.source_name.as_str();
        let text = match request.items.as_slice() {
            [item] => fill(&tr("{0} is already in {1}."), &[item.name(), source]),
            _ => fill(&tr("These items are already in {0}."), &[source]),
        };
        return Some(text);
    }
    let moved = request.items.iter().find_map(|item| {
        let folder = request.places.folder_key(item.as_container_link()?)?;
        path.iter()
            .chain(key)
            .any(|k| request.places.folder_inside(k, &folder))
            .then(|| item.name().to_string())
    });
    moved.map(|name| into_itself(&name))
}

/// What accepting the dialog showing `dialog` would do with `request`'s items.
pub fn move_verdict(request: &MoveRequest, dialog: &BrowserState) -> MoveVerdict {
    let path: Vec<Option<FolderKey>> = dialog
        .path_containers()
        .map(|c| request.places.open_folder_key(&*c))
        .collect();
    let keyed = |folders: &[Option<FolderKey>]| -> Vec<FolderKey> {
        folders.iter().flatten().cloned().collect()
    };
    let (name, copies) = match destination(dialog) {
        Destination::Row(link) => {
            if link.is_read_only() {
                return cant_add();
            }
            if let Some(item) = request.items.iter().find(|i| i.id() == link.id()) {
                return MoveVerdict::Refused(into_itself(item.name()));
            }
            let key = request.places.folder_key(link.as_ref());
            if let Some(why) = refusal(request, key.as_ref(), &keyed(&path)) {
                return MoveVerdict::Refused(why);
            }
            let place = link.place();
            let copies = (request.copies_to)(place.as_ref());
            // A copy lands only in a folder some place holds.
            if copies && place.is_none() {
                return cant_add();
            }
            (link.name().to_string(), copies)
        }
        Destination::Showing => {
            let Some(showing) = dialog
                .active_container()
                .filter(|_| dialog.depth() > 1)
                .filter(|c| c.as_writable().is_some())
            else {
                return cant_add();
            };
            let key = request.places.open_folder_key(&*showing);
            let above = keyed(&path[..path.len().saturating_sub(1)]);
            if let Some(why) = refusal(request, key.as_ref(), &above) {
                return MoveVerdict::Refused(why);
            }
            let copies = (request.copies_to)(showing.place().as_ref());
            (showing.name().to_string(), copies)
        }
    };
    let source = request.source_name.as_str();
    match copies {
        true => MoveVerdict::Copy(what(
            request,
            &tr("Copy {0} to {1}. The original stays in {2}."),
            &tr("Copy {0} items to {1}. The originals stay in {2}."),
            &[&name, source],
        )),
        false => MoveVerdict::Move(what(
            request,
            &tr("Move {0} to {1}."),
            &tr("Move {0} items to {1}."),
            &[&name],
        )),
    }
}
