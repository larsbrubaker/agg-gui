//! `AutomationFileDialog`: the file dialog an application shows while it is
//! under automation — the port of agg-sharp's
//! `GuiAutomation/AutomationDialogProvider.cs`.
//!
//! C# opened a separate 600 × 200 `SystemWindow` titled "TestAutomation File
//! Input" holding a warning label and a `TextEditWidget` named
//! **"Automation Dialog TextEdit"**; a test typed the path(s) into it and
//! pressed Enter (`CompleteDialog`, ported as
//! [`AutomationRunner::complete_dialog`](crate::AutomationRunner::complete_dialog)).
//! agg-gui has no file-dialog abstraction and a test window has no second
//! OS window, so the dialog is an in-canvas [`ModalSheet`] of the same size,
//! handed to a [`ModalPresenter`]: the run's own window
//! ([`AutomationWindow`](crate::AutomationWindow), with its built-in overlay
//! layer in `overlay.rs`) or an application's popup layer.
//!
//! Enter closes it: the text is split on `;` and each piece has its quotes
//! trimmed, giving [`FileDialogResult::file_name`] and
//! [`FileDialogResult::file_names`]; text of two characters or fewer leaves
//! both empty (cancelled), as in C#. Escape closes it with nothing chosen.
//! The callback runs on idle (C# `UiThread.RunOnIdle`), after the key that
//! closed the dialog has finished.
//!
//! C#'s `SelectFolderDialog` threw `NotImplementedException` and is left out
//! until a test needs it; `ResolveFilePath` and `ShowFileInFolder` are
//! [`AutomationFileDialog::resolve_file_path`] (the path unchanged) and
//! [`AutomationFileDialog::show_file_in_folder`] (nothing).

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use agg_gui::widgets::{FlexColumn, Label, ModalSheet, TextField};
use agg_gui::{Color, HAnchor, Insets, Size, Widget};

mod overlay;

pub use overlay::AutomationModalPresenter;

/// The name of the dialog's path field (C#'s `fileNameInput.Name`), which a
/// test waits for, types into and presses Enter in.
pub const AUTOMATION_DIALOG_TEXT_EDIT: &str = "Automation Dialog TextEdit";

/// C#'s warning label, shown above the path field.
pub const AUTOMATION_DIALOG_WARNING: &str =
    "This dialog should not appear outside of automation tests.\nNotify technical support if visible";

/// C# `new SystemWindow(600, 200)`: the dialog's panel size.
pub const AUTOMATION_DIALOG_SIZE: Size = Size {
    width: 600.0,
    height: 200.0,
};

/// Whatever shows an application-modal widget above the application: the
/// run's [`AutomationWindow`](crate::AutomationWindow) (its built-in overlay
/// layer) or an application's own popup layer.
///
/// `present` is called on the UI thread, typically from inside an event
/// handler, so it takes `&self` and the presenter keeps the modal until its
/// next layout. The modal hides itself (its `is_visible` turns `false`) when
/// it is done; the presenter then drops it.
pub trait ModalPresenter {
    /// Show `modal` above everything else until it hides itself.
    fn present(&self, modal: Box<dyn Widget>);
}

/// Which C# dialog was asked for: `OpenFileDialog` or `SaveFileDialog`.
/// Both show the same dialog; the kind is handed back in the result so one
/// callback can serve both.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileDialogKind {
    Open,
    Save,
}

/// What the dialog hands its callback (C#'s `OpenFileDialogParams` /
/// `SaveFileDialogParams` after the dialog filled in `FileName` and
/// `FileNames`). Cancelled: `file_name` is `None` and `file_names` empty.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileDialogResult {
    pub kind: FileDialogKind,
    pub file_name: Option<String>,
    pub file_names: Vec<String>,
}

impl FileDialogResult {
    /// The result for `text` as typed into the dialog (see
    /// [`parse_dialog_text`]).
    pub fn from_text(kind: FileDialogKind, text: &str) -> Self {
        let file_names = parse_dialog_text(text).unwrap_or_default();
        Self {
            kind,
            file_name: file_names.first().cloned(),
            file_names,
        }
    }

    /// Nothing chosen (Escape, or text too short to be a path).
    pub fn cancelled(kind: FileDialogKind) -> Self {
        Self {
            kind,
            file_name: None,
            file_names: Vec::new(),
        }
    }

    /// Whether nothing was chosen.
    pub fn is_cancelled(&self) -> bool {
        self.file_name.is_none()
    }
}

/// C#'s reading of the typed text: `None` when it is two characters or
/// fewer (C# `fileText.Length > 2`, counted in UTF-16 units as C# does),
/// otherwise the text split on `;` with `"` trimmed from both ends of each
/// piece (C# `Split(';').Select(f => f.Trim('"'))`; spaces are kept).
pub fn parse_dialog_text(text: &str) -> Option<Vec<String>> {
    if text.encode_utf16().count() <= 2 {
        return None;
    }
    Some(
        text.split(';')
            .map(|piece| piece.trim_matches('"').to_string())
            .collect(),
    )
}

/// The callback a shown dialog answers to.
type DialogCallback = Box<dyn FnOnce(FileDialogResult)>;

std::thread_local! {
    /// Results waiting for their idle turn. `ui_thread::run_on_idle` takes
    /// only `Send` actions, while an application's dialog callback holds its
    /// own UI state (`Rc`s); so the callback stays here, on the UI thread
    /// that showed the dialog, and the idle action posted from that same
    /// thread only pops and runs it.
    static PENDING_RESULTS: RefCell<VecDeque<(DialogCallback, FileDialogResult)>> =
        RefCell::new(VecDeque::new());
}

/// Focus ids for dialog fields: a tag in the high bits keeps them clear of
/// an application's own small ids.
static NEXT_FOCUS_ID: AtomicU64 = AtomicU64::new(0x4155_5444_0000_0000);

/// Queue `callback(result)` for the next idle drain of this thread's queue.
fn post_result(callback: DialogCallback, result: FileDialogResult) {
    PENDING_RESULTS.with(|q| q.borrow_mut().push_back((callback, result)));
    agg_gui::ui_thread::run_on_idle(|| {
        let next = PENDING_RESULTS.with(|q| q.borrow_mut().pop_front());
        if let Some((callback, result)) = next {
            callback(result);
        }
    });
}

/// C# `AutomationDialogProvider` (the provider has no state of its own
/// beyond C#'s never-set `LastDirectoryUsed`, so it is a namespace).
pub struct AutomationFileDialog;

impl AutomationFileDialog {
    /// C# `OpenFileDialog` / `SaveFileDialog`: show the dialog through
    /// `presenter`, focus its path field, and run `callback` on idle once
    /// Enter (or Escape) closes it. See the module docs.
    pub fn show(
        presenter: &dyn ModalPresenter,
        kind: FileDialogKind,
        callback: impl FnOnce(FileDialogResult) + 'static,
    ) {
        presenter.present(Box::new(Self::build(kind, Box::new(callback))));
    }

    /// C# `ResolveFilePath`: the path unchanged.
    pub fn resolve_file_path(path: &str) -> String {
        path.to_string()
    }

    /// C# `ShowFileInFolder`: nothing to show under automation.
    pub fn show_file_in_folder(_file_name: &str) {}

    /// The sheet: C#'s warning label over the named path field. Focus is
    /// requested at once; the presenter's next layout puts the field in the
    /// tree before the `App` services the request (C# focused it on `Load`).
    fn build(kind: FileDialogKind, callback: DialogCallback) -> ModalSheet {
        let visible = Rc::new(Cell::new(true));
        // The callback runs once, by whichever of Enter or Escape comes first.
        let pending: Rc<RefCell<Option<DialogCallback>>> = Rc::new(RefCell::new(Some(callback)));
        let typed = Rc::new(RefCell::new(String::new()));
        let close = {
            let visible = Rc::clone(&visible);
            let pending = Rc::clone(&pending);
            move |result: FileDialogResult| {
                visible.set(false);
                agg_gui::animation::request_draw();
                if let Some(callback) = pending.borrow_mut().take() {
                    post_result(callback, result);
                }
            }
        };
        let close = Rc::new(close);

        let focus_id = NEXT_FOCUS_ID.fetch_add(1, Ordering::Relaxed);
        let font = Arc::new(agg_gui::fonts::standard_ui_font());
        let warning = Label::new(AUTOMATION_DIALOG_WARNING, Arc::clone(&font))
            .with_font_size(15.0)
            .with_color(PINK)
            .with_wrap(true)
            .with_margin(Insets::all(20.0))
            .with_h_anchor(HAnchor::STRETCH);
        let on_enter = Rc::clone(&close);
        let on_change = Rc::clone(&typed);
        let field = TextField::new(font)
            .with_focus_id(focus_id)
            .with_margin(Insets::symmetric(30.0, 15.0))
            .with_h_anchor(HAnchor::STRETCH)
            .on_change(move |text| *on_change.borrow_mut() = text.to_string())
            .on_enter(move |text| on_enter(FileDialogResult::from_text(kind, text)))
            .with_name(AUTOMATION_DIALOG_TEXT_EDIT);
        let content = FlexColumn::new()
            .with_background(DARK_GRAY)
            .add(Box::new(warning))
            .add(Box::new(field));

        let on_default = Rc::clone(&close);
        let on_cancel = Rc::clone(&close);
        let sheet = ModalSheet::new(visible, Box::new(content))
            .with_panel_size(AUTOMATION_DIALOG_SIZE)
            // Enter with the field unfocused still commits what was typed.
            .with_default_action(move || {
                on_default(FileDialogResult::from_text(kind, &typed.borrow()))
            })
            .with_cancel_action(move || on_cancel(FileDialogResult::cancelled(kind)));
        agg_gui::focus::request_focus(focus_id);
        sheet
    }
}

/// agg-sharp `Color.Pink` (255, 192, 203).
const PINK: Color = Color::rgb(1.0, 192.0 / 255.0, 203.0 / 255.0);

/// agg-sharp `Color.DarkGray` (169, 169, 169), C#'s window background.
const DARK_GRAY: Color = Color::rgb(169.0 / 255.0, 169.0 / 255.0, 169.0 / 255.0);
