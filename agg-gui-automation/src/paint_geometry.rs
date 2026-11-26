//! Where painting puts a widget, and what of it painting lets show: agg-gui's
//! own replay of painting's transform and clip chain
//! (`agg_gui::widget::paint_geometry`), re-exported so the name lookups
//! ([`crate::tree_query`], [`crate::name_pick`]) and the painted frame agree
//! at every clip edge, and so agg-gui's `find_widget_visible_rect` and this
//! crate answer from the same rules. The rules are listed in that module.

pub use agg_gui::widget::paint_geometry::*;
