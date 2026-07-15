//! Miscellaneous demo windows: Interactive Container, Font Book, and Misc Demos.
//!
//! These demos showcase layout containers, custom painting, and Unicode glyph
//! display without requiring external state or animation.

#![allow(unused_imports)]
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::widget::paint_subtree;
use agg_gui::{
    Checkbox, CollapsingHeader, Color, DragValue, DrawCtx, Event, EventResult, FlexColumn, FlexRow,
    Font, Label, MouseButton, Point, RadioGroup, Rect, ScrollView, Separator, Size, SizedBox,
    Slider, Widget,
};

mod interactive_container;
mod misc_demos;
mod tree_section;
pub use interactive_container::interactive_container;
pub use misc_demos::misc_demos;

// The Interactive Container demo lives in the `interactive_container`
// submodule (re-exported above) to keep this file within the line limit.

// font_book is in the sibling module font_book.rs (re-exported from windows.rs).
