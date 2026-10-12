//! GUI automation for agg-gui — the port of agg-sharp's `GuiAutomation`
//! assembly (`AutomationRunner`, `TypedKeyParser`, `SearchRegion`, ...).
//!
//! Tests drive a real widget tree through the same input entry points the
//! shells use, find widgets by name, and wait on conditions.  This crate is
//! test-only: consumers take it as a dev-dependency so watchdogs and image
//! matching stay out of product builds.  The design, including the slices
//! still to land (the live driver, UI-panic capture, stack dumps), is
//! `docs/design/gui-automation.md`.
//!
//! Modules:
//! - [`keys`] — C#'s `Keys` key codes (what typed strings parse into).
//! - `key_mapping` — `Keys` strokes to agg-gui `Key`/`Modifiers`.
//! - [`typed_key_parser`] — `TypedKeyParser`: the `^`, `^+`, `{Token}` spelling.
//! - [`search_region`] — `ScreenRectangle` / `SearchRegion`.
//! - [`waits`] — `StaticDelay`, the wall-clock condition wait.
//! - [`driver`] — `HeadlessDriver` (an `App` on a pluggable painter, a
//!   software framebuffer by default, and a virtual clock) and `HeadlessWindow` (C#'s `SystemWindow` for runner-less
//!   tests).
//! - [`probe`] — `ProbeWidget`, the stand-in for a plain C# `GuiWidget`.
//! - [`execute`] — `show_window_and_execute_tests`: one run on its own UI
//!   thread, with the bring-up and test budgets, the close protocol (the
//!   `close_window` callback, veto, close budget, forced close), `AutomationWindow`, and `AutomationError`.
//! - [`runner`] — `AutomationRunner`, what a test body drives: its
//!   configuration (`AutomationConfig` with C#'s defaults), its waits
//!   (`delay`, `wait_for`, `assert`, `wait_for_draw`, `wait_for_idle`) and its name lookups
//!   (`get_widget_by_name`, `wait_for_name`, `wait_for_widget_enabled`, ...)
//!   and its pointer gestures (`click_by_name`, `double_click_by_name`, `right_click_by_name`,
//!   `move_to_by_name`, `set_mouse_cursor_position`, ...) and drags
//!   (`drag_by_name`, `drop_by_name`, `drag_drop_by_name`, `drag_widget`,
//!   `drag_to_position`, `drop`) and keyboard (`type_text`,
//!   `press_modifier_keys`/`release_modifier_keys` with `ModifierKeys`,
//!   `select_all`, `select_none`) and image searches (`get_current_screen`,
//!   `image_exists`, `wait_for_image`) and image gestures (`click_image`,
//!   `drag_image`, `drop_image`, `drag_drop_image`).
//! - [`image_match`] — `FindLeastSquaresMatch`: one image inside another.
//! - [`image_source`] — `ImageNeedle` and `ImageSource`: needles by file name
//!   (C#'s images folder, or an application's own loader).
//! - [`input`] — `InputMethod` (C# `IInputMethod`) and `SimulatedInput`
//!   (`AggInputMethods`): pointer and keyboard input through the shells'
//!   input forwarder.
//! - [`pointer_reach`] — `PointerReach`: whether a press can get to a widget.
//! - [`pointer_state`] — C#'s `UnderMouseState`, `MouseCaptured`,
//!   `ChildHasMouseCaptured` and `Focused` for a found widget.
//! - [`tree_query`] — `WidgetHandle`, `NamedHit`, lookup by name, screen and
//!   clipped rectangles, `ActuallyVisibleOnScreen`, inherited enabled state,
//!   `Parents<T>`/`Children<T>`.

pub mod driver;
pub mod execute;
pub mod image_match;
pub mod image_source;
pub mod input;
mod key_mapping;
pub mod keys;
pub mod pointer_reach;
pub mod pointer_state;
pub mod probe;
pub mod runner;
pub mod search_region;
pub mod tree_query;
pub mod typed_key_parser;
pub mod waits;

pub use driver::{
    BackgroundBusy, ClockPolicy, FrameKind, HeadlessDriver, HeadlessPainter, HeadlessWindow,
    SoftwarePainter, UiDriver,
};
pub use execute::{show_window_and_execute_tests, AutomationError, AutomationWindow, RunOptions};
pub use image_match::{find_least_squares_match, LeastSquaresMatch};
pub use image_source::{decode_needle, DirectoryImageSource, ImageNeedle, ImageSource};
pub use input::{InputMethod, MouseAction, Point2D, SimulatedInput};
pub use keys::Keys;
pub use probe::{ProbeLog, ProbeWidget};
pub use runner::{
    AutomationConfig, AutomationRunner, ClickOpts, ClickOrigin, DragDropOpts, DragOpts,
    ImageDragDropOpts, ImageOpts, ModifierKeys, WaitOpts,
};
pub use search_region::{ScreenRectangle, SearchRegion};
pub use tree_query::{NamedHit, WidgetHandle};
pub use typed_key_parser::{ParseError, TypedKey, TypedKeyParser};
pub use waits::static_delay;
