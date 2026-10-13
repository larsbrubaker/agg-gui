//! `TextArea`'s enabled state: agg-sharp's `GuiWidget.Enabled` on a
//! `ThemedTextEditWidget`, the same behaviour as `TextField`'s
//! (`text_field/enabled.rs`). A disabled area takes neither focus nor input and
//! draws its hint faded (`TextWidget`'s disabled colour, `new Color(textColor,
//! 50)`: the hint's own colour at alpha 50 of 255). Its fill, border and typed
//! text are drawn as before, as in C#.
//!
//! The state is a plain flag ([`with_enabled`](TextArea::with_enabled),
//! [`set_enabled`](TextArea::set_enabled)) and, for an owner that holds the
//! area only as a boxed `dyn Widget`, an optional shared cell
//! ([`with_enabled_cell`](TextArea::with_enabled_cell)); the area is enabled
//! when both say so. Paint reads the state live; the cached bitmap follows at
//! the next layout (the cache signature tracks it), so write the cell and call
//! [`crate::animation::request_layout`].

use std::cell::Cell;
use std::rc::Rc;

use super::TextArea;
use crate::color::Color;

/// C# `new Color(textColor, 50)`: the alpha a disabled `TextWidget` draws at.
const DISABLED_ALPHA: f32 = 50.0 / 255.0;

impl TextArea {
    /// Enable or disable the area (default enabled). A disabled area takes
    /// neither focus nor input and draws its hint faded.
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Read the enabled state from a shared cell at every layout and paint, in
    /// addition to [`with_enabled`](Self::with_enabled): the area is enabled
    /// when both are. Write the cell, then call
    /// [`crate::animation::request_layout`].
    pub fn with_enabled_cell(mut self, cell: Rc<Cell<bool>>) -> Self {
        self.enabled_cell = Some(cell);
        self
    }

    /// Change the flag set by [`with_enabled`](Self::with_enabled) after the
    /// area is built; an area that is disabled drops focus and its cached
    /// bitmap is re-rendered.
    pub fn set_enabled(&mut self, enabled: bool) {
        if self.enabled == enabled {
            return;
        }
        self.enabled = enabled;
        if !enabled {
            self.release_focus_state();
        }
        self.cache.invalidate();
        crate::animation::request_layout();
        crate::animation::request_draw();
    }

    /// Whether the area is enabled now (the flag and the cell, if any).
    pub(super) fn enabled_now(&self) -> bool {
        self.enabled && self.enabled_cell.as_ref().is_none_or(|c| c.get())
    }

    /// The hint's colour while disabled: `hint` at C#'s disabled alpha.
    pub(super) fn disabled_hint_color(hint: Color) -> Color {
        hint.with_alpha(DISABLED_ALPHA)
    }

    /// Stop editing: what losing focus does, minus the notifications (the
    /// host's focus bookkeeping sends `FocusLost`, which ends any composition
    /// and fires the edit-complete callback).
    pub(super) fn release_focus_state(&mut self) {
        self.selecting_drag = false;
        self.hovered = false;
        self.focused = false;
        self.focus_time = None;
    }
}
