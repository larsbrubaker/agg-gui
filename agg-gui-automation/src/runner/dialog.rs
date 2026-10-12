//! `complete_dialog`: answering an [`AutomationFileDialog`](crate::AutomationFileDialog)
//! — the port of MatterCAD's `CompleteDialog` test extension
//! (`MatterCADLibraryAutomationUtilities.cs`), which belongs with the dialog
//! it drives, so every application's tests share it.
//!
//! C# also had a branch for real OS file dialogs (a timed wait, since they
//! are outside the widget tree); an automation run always shows the
//! in-canvas dialog, so only the branch that waits on the dialog's focus is
//! ported.

use crate::dialog::AUTOMATION_DIALOG_TEXT_EDIT;
use crate::runner::waits::{DEFAULT_CHECK_INTERVAL_MILLISECONDS, DEFAULT_CONDITION_WAIT_SECONDS};

use super::{AutomationRunner, WaitOpts};

/// How long [`AutomationRunner::complete_dialog`] waits for the dialog to
/// show and then to go away (C#'s `RequireWidget(..., 5)` and
/// `WaitForWidgetDisappear(..., 5)`).
pub const COMPLETE_DIALOG_WAIT_SECONDS: f64 = 5.0;

impl AutomationRunner {
    /// C# `CompleteDialog(textValue)`: wait up to five seconds for the
    /// dialog's "Automation Dialog TextEdit" (panicking with C#'s
    /// `RequireWidget` message when it never shows), wait for it to hold
    /// focus (C# `WaitFor(() => widget.ContainsFocus)`, five seconds, going
    /// on either way), type `text_value`, press Enter, and wait up to five
    /// seconds for the dialog to go away. Several paths are typed as one
    /// `;`-separated string of quoted paths.
    pub fn complete_dialog(&mut self, text_value: &str) -> &mut Self {
        let opts = WaitOpts {
            secs_to_wait: COMPLETE_DIALOG_WAIT_SECONDS,
            ..WaitOpts::default()
        };
        let Some(widget) = self.get_widget_by_name(AUTOMATION_DIALOG_TEXT_EDIT, &opts) else {
            panic!(
                "{}",
                crate::StartupFailureLog::append_to(&format!(
                    "Required widget '{AUTOMATION_DIALOG_TEXT_EDIT}' was not found within {COMPLETE_DIALOG_WAIT_SECONDS} seconds."
                ))
            );
        };
        self.wait_for(
            |runner| {
                // C# `ContainsFocus`: the widget or one of its descendants.
                match (
                    widget.resolve(runner.app().root()),
                    runner.app().focused_path(),
                ) {
                    (Some(path), Some(focus)) => focus.starts_with(&path),
                    _ => false,
                }
            },
            DEFAULT_CONDITION_WAIT_SECONDS,
            DEFAULT_CHECK_INTERVAL_MILLISECONDS,
        );
        self.type_text(text_value).type_text("{Enter}");
        self.wait_for_widget_disappear(AUTOMATION_DIALOG_TEXT_EDIT, COMPLETE_DIALOG_WAIT_SECONDS);
        self
    }
}
