//! The Move dialog: what a Move does with its items given the dialog's browser (`plan.rs`, the
//! place rules and refusals), and the dialog's page over a browser page of its own (`page.rs`).
//! [`SheetDialogs`](crate::SheetDialogs) shows it once given a root to browse from
//! (`with_browse_root`); an app with its own dialogs can place [`MovePage`] itself.

pub mod page;
pub mod plan;

pub use page::{
    dialog_options, BrowserPage, MoveAnswers, MovePage, DIALOG_THUMB_SIZE, MOVE_ITEM_SUMMARY,
    MOVE_ITEM_WINDOW,
};
pub use plan::{destination, fill, move_verdict, Destination, FolderKey, MoveVerdict};
