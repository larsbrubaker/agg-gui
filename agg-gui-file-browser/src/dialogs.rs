//! The questions a browser asks the user, through the app's dialogs ([`BrowserDialogs`]):
//! confirming a change ([`Confirmation`]: removing items), asking for a name ([`NameRequest`]:
//! a new folder, a rename), picking files to add, picking the folder a Move goes to
//! ([`MoveRequest`]), and the app's own Save As and Open ([`SaveRequest`], [`OpenRequest`]).
//! [`SheetDialogs`](crate::SheetDialogs) (`sheet_dialogs/`) is a ready set on agg-gui's
//! `ModalSheet` for an app without its own: it confirms, asks for names and, given a root to
//! browse from (`SheetDialogs::with_browse_root`), shows the Move, Save and Open dialogs; it
//! offers no picking of files to add (agg-gui has no system file picker).
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
//! The Save dialog picks too: a folder that takes files and a file name ([`SaveRequest`] says
//! how a typed name becomes one and how a file's name shows in the field; [`SavePick`] answers),
//! and the app writes the file. Before replacing a file the dialog asks; an app with more to say
//! about a target (the file is open elsewhere) asks its own question through
//! [`SaveRequest::with_replace_question`]. The Open dialog ([`OpenRequest`]) picks files to open
//! from the folders it browses, answering with them and their folder ([`OpenPick`]); reading
//! them is the app's.
//!
//! An answer may come before the call returns or long after (a dialog the user takes time
//! over); the browser is not borrowed while it waits.

use std::rc::Rc;

use agg_gui_files::{tr, Folder, Item, PlaceId};

use crate::config::ItemFilter;
use crate::move_dialog::plan::FolderKey;
use crate::places::{DefaultPlaces, PlaceRules};
use crate::state::OpenFolder;

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
    /// Pick files to open ([`BrowserDialogs::pick_open_target`]).
    PickOpenTarget,
}

/// Whether items copy rather than move into a folder in the given place.
pub type CopiesTo = Rc<dyn Fn(Option<&PlaceId>) -> bool>;

/// The folder a Move's items leave, as it stands: its place, name, identity and copy rule.
#[derive(Clone)]
#[non_exhaustive]
pub struct MoveSource {
    pub place: Option<PlaceId>,
    pub name: String,
    pub key: Option<FolderKey>,
    pub copies_to: CopiesTo,
}

impl MoveSource {
    /// The folder in `place` named `name`, told apart by `key`, copying by `copies_to`.
    pub fn new(
        place: Option<PlaceId>,
        name: impl Into<String>,
        key: Option<FolderKey>,
        copies_to: CopiesTo,
    ) -> Self {
        Self {
            place,
            name: name.into(),
            key,
            copies_to,
        }
    }
}

/// Reads the folder the items leave now; `None` once the app no longer shows it.
pub type LiveSource = Rc<dyn Fn() -> Option<MoveSource>>;

/// The Move dialog's question: the items moving, already gathered (the selection is cleared by
/// the move), and the folder they leave: its place, name and identity (`FolderKey`), and the
/// copy rule, so the dialog can say whether a folder it offers moves or copies them and refuse
/// one that can't take them (`move_dialog/plan.rs`). An app whose source can change while the
/// dialog is open (the user signs out, the folder is deleted, a share's role changes) gives it
/// live ([`MoveRequest::with_live_source`]): each verdict reads it, and a source gone takes
/// nothing.
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
    /// How the dialog tells folders apart (`PlaceRules::folder_key`): the browser's rules.
    pub places: Rc<dyn PlaceRules>,
    /// The source read at each verdict in place of the fields above, when given.
    pub live_source: Option<LiveSource>,
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
            places: Rc::new(DefaultPlaces),
            live_source: None,
        }
    }

    /// The request with the rules telling its folders apart (`PlaceRules::folder_key`).
    pub fn with_places(mut self, places: Rc<dyn PlaceRules>) -> Self {
        self.places = places;
        self
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

    /// The request reading the folder the items leave from `source` at each verdict.
    pub fn with_live_source(mut self, source: LiveSource) -> Self {
        self.live_source = Some(source);
        self
    }

    /// The request as it stands: the live source read into it, or `None` once that is gone.
    pub fn current(&self) -> Option<std::borrow::Cow<'_, MoveRequest>> {
        let Some(live) = &self.live_source else {
            return Some(std::borrow::Cow::Borrowed(self));
        };
        let source = live()?;
        let mut now = self.clone();
        now.source_place = source.place;
        now.source_name = source.name;
        now.source_key = source.key;
        now.copies_to = source.copies_to;
        now.live_source = None;
        Some(std::borrow::Cow::Owned(now))
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
            .field("live_source", &self.live_source.is_some())
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

    /// Asks the user which files to open (the app's Open); `answer` hears them and the folder
    /// they are in, or `None` when they cancelled. Reading them is the app's. The default answers
    /// `None`.
    fn pick_open_target(&self, request: OpenRequest, answer: PickedOpenTarget) {
        let _ = request;
        answer(None);
    }
}

/// Turns the name the user typed into the file name to save under (an app's extension added:
/// "Bracket" to "Bracket.mcx"), or says why it can't be used (already translated).
pub type FileNamer = Rc<dyn Fn(&str) -> Result<String, String>>;

/// A file's name as the Save dialog shows it to the user: in the name field when the user clicks
/// the file, and in the question before replacing it ("Bracket.mcx" shown as "Bracket").
pub type ShownName = Rc<dyn Fn(&str) -> String>;

/// The app's own question before a save ([`SaveRequest::with_replace_question`]): asked of every
/// save the namer accepted, with the state let go; `None` keeps the dialog's own (asking to
/// replace a file already there, saving straight away otherwise).
pub type ReplaceQuestionHook = Rc<dyn Fn(&SaveTarget<'_>) -> Option<ReplaceQuestion>>;

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

/// `name` without a trailing `extension` (in any case), as the user reads a file of the app's:
/// "Bracket.MCX" is "Bracket". A name that is only the extension, or ends otherwise, stays whole.
pub fn without_extension(name: &str, extension: &str) -> String {
    let cut = name.len().saturating_sub(extension.len());
    let ends = cut > 0
        && name.is_char_boundary(cut)
        && name[cut..].to_lowercase() == extension.to_lowercase();
    match ends {
        true => name[..cut].to_string(),
        false => name.to_string(),
    }
}

/// The Save dialog's question: its heading, the label over the name field, the name it starts
/// with, how the typed name becomes the file name ([`FileNamer`], the name as typed by default),
/// how a file's name shows ([`ShownName`], whole by default), the app's own question before a
/// save, if it has one ([`ReplaceQuestionHook`]), and the name field's grey hint while empty and
/// the focus id it is given focus through when the dialog opens (`agg_gui::focus`), if any.
#[derive(Clone)]
#[non_exhaustive]
pub struct SaveRequest {
    pub title: String,
    pub name_label: String,
    pub initial_name: String,
    pub namer: FileNamer,
    pub shown_name: ShownName,
    pub replace_question: Option<ReplaceQuestionHook>,
    pub placeholder: Option<String>,
    pub focus_id: Option<agg_gui::focus::FocusId>,
}

impl SaveRequest {
    /// A save starting with `initial_name`, headed "Save As", its field labelled "Name".
    pub fn new(initial_name: impl Into<String>) -> Self {
        Self {
            title: tr("Save As"),
            name_label: tr("Name"),
            initial_name: initial_name.into(),
            namer: Rc::new(|typed: &str| Ok(typed.to_string())),
            shown_name: Rc::new(|name: &str| name.to_string()),
            replace_question: None,
            placeholder: None,
            focus_id: None,
        }
    }

    /// The request whose name field shows `placeholder` (translated) while empty ("Enter a Design
    /// Name Here").
    pub fn with_placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }

    /// The request whose name field carries the focus id `id` and is given focus when the dialog
    /// opens (`SaveController::update` asks for it once), so typing straight away names the save.
    pub fn with_focus_id(mut self, id: agg_gui::focus::FocusId) -> Self {
        self.focus_id = Some(id);
        self
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

    /// The request saving files of the app's `extension` (".mcx"): added to a typed name that
    /// lacks it ([`append_extension`]), and left off a file's name where the user reads it
    /// ([`without_extension`]): a click on "Bracket.mcx" fills in "Bracket".
    pub fn with_extension(mut self, extension: &str) -> Self {
        self.namer = append_extension(extension);
        let extension = extension.to_string();
        self.shown_name = Rc::new(move |name: &str| without_extension(name, &extension));
        self
    }

    /// The request showing a file's name as `shown_name` gives it (an app leaving off every
    /// extension it loads).
    pub fn with_shown_name(mut self, shown_name: ShownName) -> Self {
        self.shown_name = shown_name;
        self
    }

    /// The request asking `hook` before each save whether the app has its own question (the
    /// file is open elsewhere and will close); see [`ReplaceQuestionHook`].
    pub fn with_replace_question(mut self, hook: ReplaceQuestionHook) -> Self {
        self.replace_question = Some(hook);
        self
    }
}

impl std::fmt::Debug for SaveRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SaveRequest")
            .field("title", &self.title)
            .field("initial_name", &self.initial_name)
            .field("replace_question", &self.replace_question.is_some())
            .finish()
    }
}

/// Where a save is about to go, as the app's question hook is handed it: the folder (borrowed
/// while the hook runs; the browser's state is not), the file name (the namer's), and whether a
/// file of that name is listed there.
#[non_exhaustive]
pub struct SaveTarget<'a> {
    pub folder: &'a dyn Folder,
    pub file_name: &'a str,
    pub exists: bool,
}

/// A question the Save dialog asks before saving, in the page: its title, what it says, the
/// button that saves (`None` when the save can't go ahead, so the user can only go back) and the
/// button that goes back to the page, all already translated.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct ReplaceQuestion {
    pub title: String,
    pub message: String,
    pub confirm_label: Option<String>,
    pub cancel_label: String,
}

impl ReplaceQuestion {
    /// A question whose `confirm_label` button saves, with a Cancel button going back.
    pub fn new(
        title: impl Into<String>,
        message: impl Into<String>,
        confirm_label: impl Into<String>,
    ) -> Self {
        Self {
            title: title.into(),
            message: message.into(),
            confirm_label: Some(confirm_label.into()),
            cancel_label: tr("Cancel"),
        }
    }

    /// A message saying why the save can't go ahead now, with only a Cancel button going back.
    pub fn refusal(title: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            message: message.into(),
            confirm_label: None,
            cancel_label: tr("Cancel"),
        }
    }

    /// The question with its going-back button labelled `label` ("OK" for a refusal).
    pub fn with_cancel_label(mut self, label: impl Into<String>) -> Self {
        self.cancel_label = label.into();
        self
    }

    /// The dialog's own question before replacing the file the user reads as `shown_name`:
    /// "Confirm Save As", ""Bracket" already exists. Do you want to replace it?", Replace.
    pub fn replace_existing(shown_name: &str) -> Self {
        let message =
            tr("\"{0}\" already exists.\nDo you want to replace it?").replace("{0}", shown_name);
        Self::new(tr("Confirm Save As"), message, tr("Replace"))
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

/// The item filter keeping files whose names end with one of `extensions` (".stl"), in any
/// case: an Open dialog's file types.
pub fn extension_filter(extensions: &[&str]) -> ItemFilter {
    let extensions: Vec<String> = extensions.iter().map(|e| e.to_lowercase()).collect();
    Rc::new(move |item: &dyn Item| {
        let name = item.name().to_lowercase();
        extensions.iter().any(|e| name.ends_with(e.as_str()))
    })
}

/// The Open dialog's question: its heading, whether the user may pick several files, and which
/// files it lists ([`ItemFilter`], every file by default; folders always list, so the user can
/// browse).
#[derive(Clone)]
#[non_exhaustive]
pub struct OpenRequest {
    pub title: String,
    pub multiple: bool,
    pub filter: Option<ItemFilter>,
}

impl OpenRequest {
    /// An Open of one file of any kind, headed "Open".
    pub fn new() -> Self {
        Self {
            title: tr("Open"),
            multiple: false,
            filter: None,
        }
    }

    /// The request headed `title` (translated).
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// The request letting the user pick several files at once.
    pub fn with_multiple(mut self, multiple: bool) -> Self {
        self.multiple = multiple;
        self
    }

    /// The request listing only the files `filter` keeps ([`extension_filter`]).
    pub fn with_filter(mut self, filter: ItemFilter) -> Self {
        self.filter = Some(filter);
        self
    }
}

impl Default for OpenRequest {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for OpenRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenRequest")
            .field("title", &self.title)
            .field("multiple", &self.multiple)
            .field("filter", &self.filter.is_some())
            .finish()
    }
}

/// What the user picked to open: the files, in the order they were selected, and the folder
/// listing them (shared: the dialog's browser has let it go once it answers).
pub struct OpenPick {
    pub folder: OpenFolder,
    pub items: Vec<Rc<dyn Item>>,
}

impl std::fmt::Debug for OpenPick {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let items: Vec<&str> = self.items.iter().map(|i| i.name()).collect();
        f.debug_struct("OpenPick")
            .field("folder", &self.folder.borrow().name().to_string())
            .field("items", &items)
            .finish()
    }
}

/// Hears the files picked to open, or `None` when the user cancelled.
pub type PickedOpenTarget = Box<dyn FnOnce(Option<OpenPick>)>;

/// The dialogs of an app with none (see the module header).
#[derive(Clone, Copy, Debug, Default)]
pub struct NoDialogs;

impl BrowserDialogs for NoDialogs {}
