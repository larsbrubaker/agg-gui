//! The Move dialog: what a Move does with its items given the dialog's browser (`plan.rs`, the
//! place rules and refusals), its logic without widgets (`controller.rs`'s [`MoveController`]:
//! the verdict, accepting, cancelling), where a picking dialog opens and what it remembers
//! (`place.rs`, shared with the Save dialog's controller), and the dialog's default page over a
//! browser page of its own (`page.rs`).
//! [`SheetDialogs`](crate::SheetDialogs) shows it once given a root to browse from
//! (`with_browse_root`); an app with its own dialogs can place [`MovePage`] itself, or its own page over a
//! [`MoveController`].

pub mod controller;
pub mod page;
pub(crate) mod place;
pub mod plan;

pub use controller::{MoveAnswers, MoveController};
pub use page::{
    dialog_options, BrowserPage, MovePage, DIALOG_THUMB_SIZE, MOVE_ITEM_SUMMARY, MOVE_ITEM_WINDOW,
};
pub use plan::{destination, fill, move_verdict, Destination, FolderKey, MoveVerdict};
