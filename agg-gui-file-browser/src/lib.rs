//! `agg-gui-file-browser`: the widgets of agg-gui's file browser, over the folder and item model
//! in `agg-gui-files`.
//!
//! Rules every module follows:
//!
//! - **The look is the app's.** Every colour, size, icon and widget name a widget draws or
//!   carries comes from one [`BrowserLook`]: [`BrowserStyle`] for colours and sizes,
//!   [`BrowserLook::icon`] for artwork (else the icon's Font Awesome glyph, [`BrowserIcon`]) and
//!   [`BrowserNames`] for the names automation, tests and help articles find widgets by.
//!   [`DefaultLook`] is agg-gui's theme with Font Awesome glyphs.
//! - **Text the user reads** goes through `agg_gui_files::tr`, the translation hook an app points
//!   at its own translations once; every English literal this crate hands to `tr` is in
//!   [`ENGLISH_TEXTS`], so the app can check its translations cover them (a test keeps the list
//!   and the literals the same).
//! - **Names are fitted, never overdrawn.** A name drawn in a room too small for it is cut with
//!   [`fit_name`] (keeping a file's extension) or [`fit_text`] and clipped with
//!   [`fill_text_within`]; the whole name stays the widget's tooltip.
//!
//! Modules (each item is also re-exported at the root): `look` (the look and its defaults),
//! `fitted_text` (fitting names to their room), `link_label` (a text link), `icon_button` (a
//! square icon button), `bread_crumb` (the breadcrumb trail and its builder).
//!
//! The widgets come from MatterCAD's library (mattercad-rust's `mattercad-app`, same author),
//! published here under agg-gui's MIT licence.

pub mod bread_crumb;
pub mod fitted_text;
pub mod icon_button;
pub mod link_label;
pub mod look;

pub use bread_crumb::{folder_bread_crumb_for, FolderBreadCrumbWidget, UP_ICON_SIZE};
pub use fitted_text::{
    centered_text_baseline, fill_text_within, fit_keeping_extension, fit_name, fit_name_with,
    fit_text, name_extension, MIN_STEM_CHARS,
};
pub use icon_button::BrowserIconButton;
pub use link_label::LinkLabel;
pub use look::{glyph_face, BrowserIcon, BrowserLook, BrowserNames, BrowserStyle, DefaultLook};

/// Every English literal this crate hands to `agg_gui_files::tr`.
pub const ENGLISH_TEXTS: &[&str] = &["Click to go back"];
