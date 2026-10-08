//! The pointer's relation to a found widget — C#'s `UnderMouseState`,
//! `MouseCaptured`, `ChildHasMouseCaptured` and `Focused`, read from the
//! [`App`] for a [`WidgetHandle`] (C# reads them straight off the widget).
//!
//! The `App` keeps the hovered chain, the captured path and the focused path
//! (`agg_gui::App::under_mouse_state`, `captured_path`, `focused_path`);
//! these resolve the handle first, so a widget that moved among its siblings
//! is still answered for, and one that left the tree reads as not under the
//! mouse, not captured and not focused. The runner exposes the same queries
//! (`runner/mod.rs`); ported tests that drive a
//! [`HeadlessWindow`](crate::HeadlessWindow) call these directly.

use agg_gui::{App, UnderMouseState};

use crate::tree_query::WidgetHandle;

/// C# `widget.UnderMouseState`.
pub fn under_mouse_state(app: &App, widget: &WidgetHandle) -> UnderMouseState {
    widget
        .resolve(app.root())
        .map_or(UnderMouseState::NotUnderMouse, |path| {
            app.under_mouse_state(&path)
        })
}

/// C# `widget.MouseCaptured`: the widget itself holds pointer capture.
pub fn mouse_captured(app: &App, widget: &WidgetHandle) -> bool {
    match (widget.resolve(app.root()), app.captured_path()) {
        (Some(path), Some(captured)) => path == captured,
        _ => false,
    }
}

/// C# `widget.ChildHasMouseCaptured`: a descendant holds pointer capture.
pub fn child_has_mouse_captured(app: &App, widget: &WidgetHandle) -> bool {
    match (widget.resolve(app.root()), app.captured_path()) {
        (Some(path), Some(captured)) => captured.len() > path.len() && captured.starts_with(&path),
        _ => false,
    }
}

/// C# `widget.Focused`: the widget itself holds keyboard focus.
pub fn focused(app: &App, widget: &WidgetHandle) -> bool {
    match (widget.resolve(app.root()), app.focused_path()) {
        (Some(path), Some(focus)) => path == focus,
        _ => false,
    }
}
