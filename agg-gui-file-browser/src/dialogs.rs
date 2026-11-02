//! The questions a browser asks the user, through the app's dialogs ([`BrowserDialogs`]):
//! confirming a change ([`Confirmation`]: removing items), asking for a name ([`NameRequest`]:
//! a new folder, a rename), picking files to add, and picking the folder a Move goes to
//! ([`MoveRequest`]). [`SheetDialogs`](crate::SheetDialogs) (`sheet_dialogs/`) is a ready set
//! on agg-gui's `ModalSheet` for an app without its own: it confirms, asks for names and, given a
//! root to browse from (`SheetDialogs::with_browse_root`), shows the Move dialog
//! (`move_dialog/`); it offers no file picking (agg-gui has no file picker).
//!
//! The Move dialog only picks: it answers with the folder it opened for the items and whether
//! they copy ([`MovePick`]), and the browser's Move moves or copies them there itself, as a drop
//! into that folder does (`drop_into`), recorded for Undo.
//!
//! The browser asks only what [`BrowserDialogs::offers`] says the app can show, and leaves the
//! action that needs a dialog it lacks unavailable (greyed in its menu), so nothing is ever done
//! without the question being asked. [`NoDialogs`] offers none: a browser without dialogs adds,
//! names and removes nothing through its menus. Each method's default answers as if the user
//! cancelled, so a call that reaches it changes nothing.
//!
//! An answer may come before the call returns or long after (a dialog the user takes time
//! over); the browser is not borrowed while it waits.

use std::rc::Rc;

use agg_gui_files::{tr, Folder, Item, PlaceId};

use crate::move_dialog::plan::FolderKey;

/// A dialog a browser can ask for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DialogKind {
    /// Confirm a change ([`BrowserDialogs::confirm`]).
    Confirm,
    /// Ask for a name ([`BrowserDialogs::ask_name`]).
    AskName,
    /// Pick files to add ([`BrowserDialogs::pick_files`]).
    PickFiles,
    /// Pick the folder a Move goes to ([`BrowserDialogs::pick_move_target`]).
    PickMoveTarget,
    /// Pick the folder and name a save goes to ([`BrowserDialogs::pick_save_target`]).
    PickSaveTarget,
}

/// Whether items copy rather than move into a folder in the given place.
pub type CopiesTo = Rc<dyn Fn(Option<&PlaceId>) -> bool>;

/// The Move dialog's question: the items moving, already gathered (the selection is cleared by
/// the move), and the folder they leave: its place, name and identity (`FolderKey`), and the
/// copy rule, so the dialog can say whether a folder it offers moves or copies them and refuse
/// one that can't take them (`move_dialog/plan.rs`).
#[derive(Clone)]
#[non_exhaustive]
pub struct MoveRequest {
    pub items: Vec<Rc<dyn Item>>,
    pub source_place: Option<PlaceId>,
    /// The name of the folder the items leave ("These items are already in Downloads.").
    pub source_name: String,
    /// The folder the items leave, as the dialog tells folders apart.
    pub source_key: Option<FolderKey>,
    /// Whether the items copy rather than move into a folder in the given place: the browser's
    /// `PlaceRules` over the folder they leave, and always when that folder keeps its items.
    pub copies_to: CopiesTo,
}

impl MoveRequest {
    /// `items` leaving a folder in `source_place`, of no known name or identity, copying into
    /// another place and moving within one.
    pub fn new(items: Vec<Rc<dyn Item>>, source_place: Option<PlaceId>) -> Self {
        let place = source_place.clone();
        Self {
            items,
            source_place,
            source_name: String::new(),
            source_key: None,
            copies_to: Rc::new(move |target| target != place.as_ref()),
        }
    }

    /// The request with the folder the items leave: its name, identity and copy rule.
    pub fn with_source(
        mut self,
        name: impl Into<String>,
        key: Option<FolderKey>,
        copies_to: CopiesTo,
    ) -> Self {
        self.source_name = name.into();
        self.source_key = key;
        self.copies_to = copies_to;
        self
    }
}

impl std::fmt::Debug for MoveRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MoveRequest")
            .field(
                "items",
                &self
                    .items
                    .iter()
                    .map(|i| i.name().to_string())
                    .collect::<Vec<_>>(),
            )
            .field("source_place", &self.source_place)
            .field("source_name", &self.source_name)
            .field("source_key", &self.source_key)
            .finish()
    }
}

/// What the user picked in the Move dialog: the folder the items go into, opened, and whether
/// they copy (the originals staying where they are).
pub struct MovePick {
    pub target: Box<dyn Folder>,
    pub copy: bool,
}

impl std::fmt::Debug for MovePick {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MovePick")
            .field("target", &self.target.name())
            .field("copy", &self.copy)
            .finish()
    }
}

/// Hears what the user picked for a Move, or `None` when they cancelled.
pub type PickedMoveTarget = Box<dyn FnOnce(Option<MovePick>)>;

/// A change the user confirms: the dialog's title, what it says, and the confirming button's
/// label, all already translated.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Confirmation {
    pub title: String,
    pub message: String,
    pub confirm_label: String,
}

impl Confirmation {
    pub fn new(
        title: impl Into<String>,
        message: impl Into<String>,
        confirm_label: impl Into<String>,
    ) -> Self {
        Self {
            title: title.into(),
            message: message.into(),
            confirm_label: confirm_label.into(),
        }
    }
}

/// A name the user types: the dialog's title, its prompt, the name it starts with, and the
/// confirming button's label, all already translated.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct NameRequest {
    pub title: String,
    pub prompt: String,
    pub initial_name: String,
    pub confirm_label: String,
}

impl NameRequest {
    pub fn new(
        title: impl Into<String>,
        prompt: impl Into<String>,
        initial_name: impl Into<String>,
        confirm_label: impl Into<String>,
    ) -> Self {
        Self {
            title: title.into(),
            prompt: prompt.into(),
            initial_name: initial_name.into(),
            confirm_label: confirm_label.into(),
        }
    }
}

/// Hears the files the user picked, as items (none when they cancelled).
pub type PickedFiles = Box<dyn FnOnce(Vec<Rc<dyn Item>>)>;

/// The app's dialogs (see the module header).
pub trait BrowserDialogs {
    /// Whether the app can show a `kind` dialog. The default offers none.
    fn offers(&self, kind: DialogKind) -> bool {
        let _ = kind;
        false
    }

    /// Asks the user to confirm `question`; `answer` hears whether they did. The default answers
    /// no.
    fn confirm(&self, question: Confirmation, answer: Box<dyn FnOnce(bool)>) {
        let _ = question;
        answer(false);
    }

    /// Asks the user for a name; `answer` hears it, or `None` when they cancelled. The default
    /// answers `None`.
    fn ask_name(&self, request: NameRequest, answer: Box<dyn FnOnce(Option<String>)>) {
        let _ = request;
        answer(None);
    }

    /// Asks the user to pick files to add; `answer` hears them as items (none when they
    /// cancelled). The default answers none.
    fn pick_files(&self, answer: PickedFiles) {
        answer(Vec::new());
    }

    /// Asks the user where `request`'s items go; `answer` hears the folder picked (the browser
    /// then moves or copies them there as a drop onto it would), or `None` when they cancelled.
    /// The default answers `None`.
    fn pick_move_target(&self, request: MoveRequest, answer: PickedMoveTarget) {
        let _ = request;
        answer(None);
    }

    /// Asks the user where a save goes and under what name (the app's Save As); `answer` hears
    /// the folder and the file name, or `None` when they cancelled. Saving is the app's. The
    /// default answers `None`.
    fn pick_save_target(&self, request: SaveRequest, answer: PickedSaveTarget) {
        let _ = request;
        answer(None);
    }
}

/// Turns the name the user typed into the file name to save under (an app's extension added:
/// "Bracket" to "Bracket.mcx"), or says why it can't be used (already translated).
pub type FileNamer = Rc<dyn Fn(&str) -> Result<String, String>>;

/// The file namer adding `extension` (".mcx") unless the name already ends with it, in any
/// case.
pub fn append_extension(extension: &str) -> FileNamer {
    let extension = extension.to_string();
    Rc::new(move |typed: &str| {
        let lower = typed.to_lowercase();
        match lower.ends_with(&extension.to_lowercase()) {
            true => Ok(typed.to_string()),
            false => Ok(format!("{typed}{extension}")),
        }
    })
}

/// The Save dialog's question: its heading, the label over the name field, the name it starts
/// with, and how the typed name becomes the file name ([`FileNamer`], the name as typed by
/// default).
#[derive(Clone)]
#[non_exhaustive]
pub struct SaveRequest {
    pub title: String,
    pub name_label: String,
    pub initial_name: String,
    pub namer: FileNamer,
}

impl SaveRequest {
    /// A save starting with `initial_name`, headed "Save As", its field labelled "Name".
    pub fn new(initial_name: impl Into<String>) -> Self {
        Self {
            title: tr("Save As"),
            name_label: tr("Name"),
            initial_name: initial_name.into(),
            namer: Rc::new(|typed: &str| Ok(typed.to_string())),
        }
    }

    /// The request headed `title` with its field labelled `name_label` (both translated).
    pub fn with_labels(mut self, title: impl Into<String>, name_label: impl Into<String>) -> Self {
        self.title = title.into();
        self.name_label = name_label.into();
        self
    }

    /// The request naming its file through `namer` (an extension added or checked).
    pub fn with_namer(mut self, namer: FileNamer) -> Self {
        self.namer = namer;
        self
    }
}

impl std::fmt::Debug for SaveRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SaveRequest")
            .field("title", &self.title)
            .field("initial_name", &self.initial_name)
            .finish()
    }
}

/// Where the user chose to save: the folder (opened, handed over by the dialog), the file name
/// (the namer's), and whether a file of that name is there and the user agreed to replace it.
pub struct SavePick {
    pub folder: Box<dyn Folder>,
    pub file_name: String,
    pub replaces: bool,
}

impl std::fmt::Debug for SavePick {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SavePick")
            .field("folder", &self.folder.name())
            .field("file_name", &self.file_name)
            .field("replaces", &self.replaces)
            .finish()
    }
}

/// Hears where a save goes, or `None` when the user cancelled.
pub type PickedSaveTarget = Box<dyn FnOnce(Option<SavePick>)>;

/// The dialogs of an app with none (see the module header).
#[derive(Clone, Copy, Debug, Default)]
pub struct NoDialogs;

impl BrowserDialogs for NoDialogs {}
