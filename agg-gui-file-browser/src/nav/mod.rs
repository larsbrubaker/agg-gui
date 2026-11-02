//! The browser's navigation pieces, which a browser's composition lays out around its tree and
//! list: the view options and sorting menu buttons and the Undo and Redo buttons (`buttons.rs`,
//! `menu_button.rs`), the tree|list splitter (`splitter.rs`) and the provider message under a
//! list-only browser's list (`provider_message.rs`). The breadcrumb trail with its up button is
//! `bread_crumb.rs`. Every name, colour and picture they show is the config's look's.

pub mod buttons;
pub mod menu_button;
pub mod provider_message;
pub mod splitter;
