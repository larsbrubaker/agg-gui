//! GUI automation for agg-gui — the port of agg-sharp's `GuiAutomation`
//! assembly (`AutomationRunner`, `TypedKeyParser`, `SearchRegion`, ...).
//!
//! Tests drive a real widget tree through the same input entry points the
//! shells use, find widgets by name, and wait on conditions.  This crate is
//! test-only: consumers take it as a dev-dependency so watchdogs and image
//! matching stay out of product builds.  The design, including the slices
//! still to land (the runner, input simulation, the live driver), is
//! `docs/design/gui-automation.md`.
//!
//! Modules:
//! - [`keys`] — C#'s `Keys` key codes (what typed strings parse into).
//! - `key_mapping` — `Keys` strokes to agg-gui `Key`/`Modifiers`.
//! - [`typed_key_parser`] — `TypedKeyParser`: the `^`, `^+`, `{Token}` spelling.
//! - [`search_region`] — `ScreenRectangle` / `SearchRegion`.
//! - [`waits`] — `StaticDelay`, the wall-clock condition wait.
//! - [`driver`] — `HeadlessDriver` (an `App` on a software framebuffer and a
//!   virtual clock) and `HeadlessWindow` (C#'s `SystemWindow` for runner-less
//!   tests).
//! - [`probe`] — `ProbeWidget`, the stand-in for a plain C# `GuiWidget`.
//! - [`tree_query`] — `WidgetHandle`, lookup by name, screen and clipped
//!   rectangles, `ActuallyVisibleOnScreen`, `Parents<T>`/`Children<T>`.

pub mod driver;
mod key_mapping;
pub mod keys;
pub mod probe;
pub mod search_region;
pub mod tree_query;
pub mod typed_key_parser;
pub mod waits;

pub use driver::{ClockPolicy, FrameKind, HeadlessDriver, HeadlessWindow, UiDriver};
pub use keys::Keys;
pub use probe::{ProbeLog, ProbeWidget};
pub use search_region::{ScreenRectangle, SearchRegion};
pub use tree_query::WidgetHandle;
pub use typed_key_parser::{ParseError, TypedKey, TypedKeyParser};
pub use waits::static_delay;
