//! Move or copy: the rule a browser's drags and Move dialog follow when items leave one folder
//! for another ([`PlaceRules`], [`DefaultPlaces`]), and the hook through which an app runs a copy
//! itself ([`PlaceRules::take_over_copy`], handed a [`CopyCx`]).
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

use std::rc::Rc;

use agg_gui::Modifiers;
use agg_gui_files::{ContainerActions, Folder, Item, PlaceId};

use crate::config::ErrorReport;
use crate::history::HistoryRecorder;
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
    /// The app leaves the copy, handed back whole, to the browser.
    Declined(CopyCx),
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
    /// watch), answering [`CopyTakeover::Taken`], or hands it back. The default hands every copy
    /// back.
    fn take_over_copy(&self, cx: CopyCx) -> CopyTakeover {
        CopyTakeover::Declined(cx)
    }
}

/// The rules with nothing overridden: the module header's rule, and every copy run by the
/// browser.
#[derive(Clone, Copy, Debug, Default)]
pub struct DefaultPlaces;

impl PlaceRules for DefaultPlaces {}
