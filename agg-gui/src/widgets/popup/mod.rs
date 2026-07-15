//! A thin popup abstraction layered on top of the menu system.
//!
//! The framework already ships a full popup implementation inside
//! [`super::menu`]: `PopupMenu`/`PopupMenuState` handle overlay painting, hit
//! testing, keyboard navigation, submenu cascades, and dismiss-on-outside. What
//! that system deliberately does *not* offer is arbitrary anchor placement —
//! its popups always open left-aligned and extend straight up or down from a
//! bar or a click point.
//!
//! This module fills exactly that gap, and nothing more:
//!
//! - [`RectAlign`] / [`Align2`] / [`Align`] — parent/child anchor placement math
//!   (egui's `RectAlign`), with named presets and a pixel gap. Pure functions,
//!   unit-tested against production code.
//! - [`Popup`] — a content-agnostic controller owning open-state, anchor,
//!   placement, gap, and [`PopupCloseBehavior`]. A host widget supplies the
//!   content and painting.
//!
//! For nested-menu content, reuse [`PopupMenu`](super::menu::PopupMenu) rather
//! than growing this module — its submenu mechanism is the intended way to nest.

pub mod align;
pub mod behavior;

pub use align::{clamp_rect, Align, Align2, RectAlign};
pub use behavior::{Popup, PopupClickOutcome, PopupCloseBehavior};
