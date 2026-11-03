//! Move or copy: the rule a browser's drags and Move dialog follow when items leave one folder
//! for another ([`PlaceRules`], [`DefaultPlaces`]), and the hooks through which an app runs a copy
//! itself ([`PlaceRules::take_over_copy`], handed a [`CopyCx`]) or a move itself
//! ([`PlaceRules::take_over_move`], handed a [`MoveCx`]: a move between two folders of a remote
//! place whose step can only be recorded once the server answers).
//!
//! The rule rests on places (`agg_gui_files::PlaceId`, what a folder's or row's `place()`
//! answers). Items move within one place and are copied, keeping the originals, when:
//! - they come from a folder that cannot give them up (not writable, or writable but refusing
//!   `ContainerActions::RemoveItems`), or from no folder the browser knows;
//! - the copy modifier is held at the drop ([`copy_modifier_held`]: Option on the Mac, Ctrl
//!   elsewhere); it only ever turns a move into a copy, never a copy into a move;
//! - the source and the target are in two different known places. A folder in no place (a
//!   catalog, search results, a row whose folder is not made yet) never makes a drop cross: it
//!   counts as being where the other side is until opened.
//!
//! [`default_drop_copies`] is that rule before the drop, from the target row's place (what the
//! drag shows: a "+" badge, "Copy to"); [`default_crosses_places`] is its final word once the
//! target folder is open. The drag (`drag/`) reads them.
//!
//! The Move dialog tells folders apart by [`PlaceRules::folder_key`] (a folder row) and
//! [`PlaceRules::open_folder_key`] (an open folder), defaulting to the folder's place and
//! location key; an app whose folders have no location key (a remote place's folders, told by
//! their server IDs) answers keys of its own, so "already in" and "can't go inside itself" hold
//! for them too. Whether a folder is inside a moved folder is [`PlaceRules::folder_inside`]'s
//! word: by default a folder is inside only itself (the dialog sees the rest on its path), and an
//! app whose folders a root of their own can reach (a library folder on disk inside a moved
//! folder) answers by their locations.

use std::rc::Rc;

use agg_gui::Modifiers;
use agg_gui_files::{ContainerActions, Folder, FolderLink, Item, PlaceId, WritableFolder};

use crate::config::ErrorReport;
use crate::history::HistoryRecorder;
use crate::move_dialog::FolderKey;
use crate::state::OpenFolder;

/// Whether `modifiers` hold the copy modifier: Option on the Mac, Ctrl on Windows and Linux.
pub fn copy_modifier_held(modifiers: Modifiers) -> bool {
    if cfg!(target_os = "macos") {
        modifiers.alt
    } else {
        modifiers.ctrl
    }
}

/// Whether two places are both known and differ. An unknown place (`None`) never differs.
pub fn places_differ(source: Option<&PlaceId>, target: Option<&PlaceId>) -> bool {
    match (source, target) {
        (Some(source), Some(target)) => source != target,
        _ => false,
    }
}

/// Whether `source` can give its items up: it is writable and allows removing them.
fn gives_items_up(source: &dyn Folder) -> bool {
    source
        .as_writable()
        .is_some_and(|w| w.allow_action(ContainerActions::RemoveItems))
}

/// The rule (see the module header): whether items dropped from `source` (`None`: no folder the
/// browser knows) onto a folder in `target` are copied rather than moved.
pub fn default_drop_copies(
    source: Option<&dyn Folder>,
    target: Option<PlaceId>,
    copy_modifier: bool,
) -> bool {
    let Some(source) = source else {
        return true;
    };
    copy_modifier
        || !gives_items_up(source)
        || places_differ(source.place().as_ref(), target.as_ref())
}

/// Whether the opened folders `source` and `target` are in two different known places: the
/// drop's final word on a copy, once the target folder is open and answers its own place.
pub fn default_crosses_places(source: &dyn Folder, target: &dyn Folder) -> bool {
    places_differ(source.place().as_ref(), target.place().as_ref())
}

/// A copy the browser is about to run: the items, the opened folder they go into and its name
/// (for the notice and any failure), the folder they come from, where its history step goes and
/// where its failures go. Handed by value to [`PlaceRules::take_over_copy`], which keeps it when
/// it runs the copy and hands it back when it does not.
pub struct CopyCx {
    /// What is copied, in the order the user picked it.
    pub items: Vec<Rc<dyn Item>>,
    /// The opened folder the copies go into.
    pub target: Box<dyn Folder>,
    /// The target folder's name, as the user reads it.
    pub target_name: String,
    /// The open folder the items come from, `None` when the browser knows none (the browser
    /// borrows it only for the length of its own calls).
    pub source: Option<OpenFolder>,
    /// Records the copy's history step when it lands, from the copy's own callback.
    pub recorder: HistoryRecorder,
    /// Shows the user a copy's failure (already translated).
    pub report: ErrorReport,
}

/// What [`PlaceRules::take_over_copy`] did with a copy.
pub enum CopyTakeover {
    /// The app runs the copy: it reports the outcome and records any undo step itself.
    Taken,
    /// The app refused the copy and has already told the user why: nothing is copied, and the
    /// drop answers that it copied nothing.
    Refused,
    /// The app leaves the copy, handed back whole, to the browser.
    Declined(CopyCx),
}

/// A move the browser is about to run: the items, the writable folders they go into and come
/// from, and the target's name (for the notice and any failure). Handed to
/// [`PlaceRules::take_over_move`], which runs the move or hands it back. The folders are lent
/// for the call only: no browser state is borrowed while the hook runs, but the two folders are.
pub struct MoveCx<'a> {
    /// What moves, in the order the user picked it.
    pub items: &'a [Rc<dyn Item>],
    /// The opened folder the items go into.
    pub target: &'a mut dyn WritableFolder,
    /// The target folder's name, as the user reads it.
    pub target_name: &'a str,
    /// The open folder the items come from.
    pub source: &'a mut dyn WritableFolder,
}

/// What [`PlaceRules::take_over_move`] did with a move.
pub enum MoveTakeover<'a> {
    /// The app ran the move: how it went (an error is reported with the items that did move, as
    /// the browser's own move is). The app records any undo step itself (through the history's
    /// recorder, once its move lands).
    Taken(Result<(), String>),
    /// The app leaves the move, handed back whole, to the browser (the folders' recorded move).
    Declined(MoveCx<'a>),
}

/// How a browser decides between moving and copying, and who runs a copy. Every method has the
/// default the module header describes; an app overrides only what its places need.
pub trait PlaceRules {
    /// Whether items dropped from `source` onto a folder in `target` are copied (the module
    /// header's rule, [`default_drop_copies`]).
    fn drop_copies(
        &self,
        source: Option<&dyn Folder>,
        target: Option<PlaceId>,
        copy_modifier: bool,
    ) -> bool {
        default_drop_copies(source, target, copy_modifier)
    }

    /// Whether the opened folders are in different places, so the drop copies
    /// ([`default_crosses_places`]).
    fn crosses_places(&self, source: &dyn Folder, target: &dyn Folder) -> bool {
        default_crosses_places(source, target)
    }

    /// Runs `cx`'s copy the app's own way (a copy into a remote place as a task the user can
    /// watch), answering [`CopyTakeover::Taken`] (or [`CopyTakeover::Refused`] when it refuses
    /// the copy), or hands it back. The default hands every copy
    /// back.
    fn take_over_copy(&self, cx: CopyCx) -> CopyTakeover {
        CopyTakeover::Declined(cx)
    }

    /// Runs `cx`'s move the app's own way, answering [`MoveTakeover::Taken`] with how it went,
    /// or hands it back. The default hands every move back, to the folders' recorded move
    /// (`WritableFolder::move_in_recorded`).
    fn take_over_move<'a>(&self, cx: MoveCx<'a>) -> MoveTakeover<'a> {
        MoveTakeover::Declined(cx)
    }

    /// How the Move dialog tells apart the folder `link` opens (`None`: it can't tell, so the
    /// row is never "the items' own folder"). The default is its place and location key
    /// ([`FolderKey::of_link`]).
    fn folder_key(&self, link: &dyn FolderLink) -> Option<FolderKey> {
        FolderKey::of_link(link)
    }

    /// How the Move dialog tells apart the open `folder`; it must answer the key
    /// [`PlaceRules::folder_key`] answers for a row opening the same folder. The default is its
    /// place and location key ([`FolderKey::of_folder`]).
    fn open_folder_key(&self, folder: &dyn Folder) -> Option<FolderKey> {
        FolderKey::of_folder(folder)
    }

    /// Whether the folder keyed `inner` is `outer` or inside it, however the dialog reached it:
    /// the Move dialog refuses a destination on or below a folder it is moving. The default
    /// answers only `inner == outer`, the dialog's open folders standing for the rest.
    fn folder_inside(&self, inner: &FolderKey, outer: &FolderKey) -> bool {
        inner == outer
    }
}

/// The rules with nothing overridden: the module header's rule, and every copy run by the
/// browser.
#[derive(Clone, Copy, Debug, Default)]
pub struct DefaultPlaces;

impl PlaceRules for DefaultPlaces {}
