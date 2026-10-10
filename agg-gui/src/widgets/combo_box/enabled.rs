//! Enabled state for [`super::ComboBox`] — the agg-gui counterpart of
//! agg-sharp's `GuiWidget.Enabled` on a `DropDownList`.
//!
//! A combo built with [`ComboBox::with_enabled_fn`] asks its predicate live
//! (the same convention as `Button::with_enabled_fn` and
//! `SegmentedControl::with_enabled_fn`).  While it answers `false` the combo
//! reports `is_enabled() == false`, is not focusable, refuses to open, drops
//! pointer and keyboard input, and closes its list if it was open.  The
//! disabled paint follows `DropDownList`: the outline is drawn at alpha 30
//! (`BorderColor` → `new Color(color, 30)`), the closed label and the arrow
//! at alpha 50 (`TextWidget`'s disabled colour, which the drop arrow also
//! uses through `DropDownList.TextColor`), and the fill is unchanged.  Both
//! alphas are settable through [`ComboBoxDisabledStyle`].
//!
//! Split out of `combo_box.rs` to keep that file under the 800-line limit;
//! the closed-box painter in `style.rs` consumes the resolved alphas.

use super::*;
use crate::color::Color;

/// How a disabled [`ComboBox`] paints.  Alphas are 0–255 and replace the
/// colour's own alpha, as agg-sharp's `new Color(color, alpha)` does.
///
/// Defaults match agg-sharp's `DropDownList`: `border_alpha` 30 and
/// `label_alpha` 50.  The fill is never changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ComboBoxDisabledStyle {
    /// Alpha of the closed box's outline while disabled.
    pub border_alpha: u8,
    /// Alpha of the closed box's label text and drop arrow while disabled.
    pub label_alpha: u8,
}

impl Default for ComboBoxDisabledStyle {
    fn default() -> Self {
        Self {
            border_alpha: 30,
            label_alpha: 50,
        }
    }
}

impl ComboBox {
    /// Gate the combo on a live predicate (`None`, the default, is always
    /// enabled).  See the module docs for what a disabled combo does.
    pub fn with_enabled_fn(mut self, f: impl Fn() -> bool + 'static) -> Self {
        self.enabled_fn = Some(Rc::new(f));
        self
    }

    /// Override how the combo paints while disabled.  See
    /// [`ComboBoxDisabledStyle`].
    pub fn with_disabled_style(mut self, style: ComboBoxDisabledStyle) -> Self {
        self.disabled_style = style;
        self
    }

    /// The current disabled-paint style.
    pub fn disabled_style(&self) -> ComboBoxDisabledStyle {
        self.disabled_style
    }

    pub(super) fn enabled_now(&self) -> bool {
        self.enabled_fn.as_ref().map(|f| f()).unwrap_or(true)
    }

    /// `color` as the disabled outline when disabled, unchanged otherwise.
    pub(super) fn border_for_state(&self, color: Color) -> Color {
        if self.enabled_now() {
            color
        } else {
            color.with_alpha(self.disabled_style.border_alpha as f32 / 255.0)
        }
    }

    /// `color` as the disabled label / arrow when disabled, unchanged otherwise.
    pub(super) fn text_for_state(&self, color: Color) -> Color {
        if self.enabled_now() {
            color
        } else {
            color.with_alpha(self.disabled_style.label_alpha as f32 / 255.0)
        }
    }

    /// Close the list and drop its hover / drag state (focus loss, window
    /// deactivation).
    pub(super) fn close_list(&mut self) {
        let was_open = self.open;
        self.open = false;
        self.hovered_item = None;
        self.scrollbar.hovered_bar = false;
        self.scrollbar.hovered_thumb = false;
        self.scrollbar.dragging = false;
        self.middle_dragging = false;
        if was_open {
            crate::animation::request_draw();
        }
    }

    /// Close the list and drop hover / drag state when the predicate has
    /// turned `false`.  Returns whether the combo is disabled.
    pub(super) fn close_if_disabled(&mut self) -> bool {
        if self.enabled_now() {
            return false;
        }
        let was_open = self.open;
        self.open = false;
        self.hovered_item = None;
        self.button_hovered = false;
        self.scrollbar.hovered_bar = false;
        self.scrollbar.hovered_thumb = false;
        self.scrollbar.dragging = false;
        self.middle_dragging = false;
        if was_open {
            crate::animation::request_draw();
        }
        true
    }

    /// `on_event` while disabled: focus bookkeeping still runs (so the focus
    /// outline is not left on), everything else is dropped.
    pub(super) fn on_disabled_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::FocusGained => self.set_focused(true),
            Event::FocusLost => self.set_focused(false),
            _ => {}
        }
        EventResult::Ignored
    }
}
