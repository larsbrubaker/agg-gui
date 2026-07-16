//! Colour controls for [`RichTextToolbar`](super::RichTextToolbar): the
//! text-colour and highlight swatch buttons and the floating colour-picker
//! overlay they open.
//!
//! A swatch button flips a shared [`PickerKind`] cell; the [`color_overlay`]
//! widget (a [`Rebuilder`]) watches that cell and shows a
//! [`color_wheel_picker_dialog`] while a swatch is open, applying the chosen
//! colour through the [`RichEditHandle`] on *Select*.
//!
//! ## Live-preview status
//!
//! The owner's spec calls for wiring these through a `begin/commit/cancel`
//! preview session on the handle so the selection previews the colour live
//! while the wheel is dragged.  That session API is **not yet on `main`** (it is
//! in review on another branch), so this implementation applies the colour only
//! on *Select* (commit) and treats *Cancel* as a no-op — no live preview.
//!
//! TODO(color-preview): once the `RichEditHandle` preview-session API lands,
//! drive `on_change` → `begin/continue_preview`, `on_select` → `commit_preview`,
//! and `on_cancel` → `cancel_preview` so the selection previews live.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use crate::color::Color;
use crate::text::Font;
use crate::widget::Widget;
use crate::widgets::button::Button;
use crate::widgets::color_wheel_picker::{color_wheel_picker_dialog, ColorWheelPicker};
use crate::widgets::primitives::SizedBox;
use crate::widgets::rebuilder::Rebuilder;

use super::super::commands::RichCommand;
use super::super::editor::RichEditHandle;
use super::PickerKind;

const ICON_TEXT_COLOR: &str = "\u{F031}"; // "font"
const ICON_HIGHLIGHT: &str = "\u{F043}"; // "tint"

/// The text-colour swatch button (Font Awesome "font" glyph).
pub(super) fn text_color_button(font: &Arc<Font>, picker: &Rc<Cell<PickerKind>>) -> Box<dyn Widget> {
    color_button(font, ICON_TEXT_COLOR, picker, PickerKind::TextColor)
}

/// The highlight swatch button (Font Awesome "tint" glyph).
pub(super) fn highlight_button(font: &Arc<Font>, picker: &Rc<Cell<PickerKind>>) -> Box<dyn Widget> {
    color_button(font, ICON_HIGHLIGHT, picker, PickerKind::Highlight)
}

/// A colour swatch button that opens the floating picker for `kind`.
fn color_button(
    font: &Arc<Font>,
    icon: &str,
    picker: &Rc<Cell<PickerKind>>,
    kind: PickerKind,
) -> Box<dyn Widget> {
    let picker = Rc::clone(picker);
    Box::new(
        Button::new(icon, Arc::clone(font))
            .with_font_size(13.0)
            .with_subtle()
            .on_click(move || {
                picker.set(kind);
                crate::animation::request_draw();
            }),
    )
}

/// Build the floating colour-picker overlay bound to the toolbar's `picker`
/// cell.  Add the returned widget to a top-level [`Stack`](crate::widgets::primitives::Stack)
/// (via `add_aligned`) that spans the editor area so the dialog can float over
/// the content — a thin toolbar strip cannot host it directly.  Returns a
/// zero-size layer while no swatch is open.
pub(super) fn color_overlay(
    font: &Arc<Font>,
    handle: &RichEditHandle,
    picker: &Rc<Cell<PickerKind>>,
) -> Box<dyn Widget> {
    let ver_picker = Rc::clone(picker);
    let build_font = Arc::clone(font);
    let build_handle = handle.clone();
    let build_picker = Rc::clone(picker);
    Box::new(Rebuilder::new(
        move || match ver_picker.get() {
            PickerKind::None => 0,
            PickerKind::TextColor => 1,
            PickerKind::Highlight => 2,
        },
        move || build_picker_dialog(&build_font, &build_handle, &build_picker),
    ))
}

fn build_picker_dialog(
    font: &Arc<Font>,
    handle: &RichEditHandle,
    picker: &Rc<Cell<PickerKind>>,
) -> Box<dyn Widget> {
    let kind = picker.get();
    if kind == PickerKind::None {
        return Box::new(SizedBox::new().with_width(0.0).with_height(0.0));
    }
    let allow_none = kind == PickerKind::Highlight;
    let initial = Color::rgb(0.2, 0.45, 0.88);
    let sel_handle = handle.clone();
    let sel_picker = Rc::clone(picker);
    let cancel_picker = Rc::clone(picker);
    // TODO(color-preview): once the handle exposes a preview session, add an
    // `.on_change(...)` here that previews `opt` on the selection live.
    let widget = ColorWheelPicker::new(initial, Arc::clone(font))
        .with_allow_none(allow_none)
        .with_show_alpha(true)
        .with_font_size(12.0)
        .on_select(move |opt| {
            match kind {
                PickerKind::TextColor => {
                    if let Some(c) = opt {
                        sel_handle.exec(&RichCommand::SetTextColor(c));
                    }
                }
                PickerKind::Highlight => sel_handle.exec(&RichCommand::SetHighlight(opt)),
                PickerKind::None => {}
            }
            sel_picker.set(PickerKind::None);
            crate::animation::request_draw();
        })
        .on_cancel(move || {
            cancel_picker.set(PickerKind::None);
            crate::animation::request_draw();
        });
    let title = match kind {
        PickerKind::Highlight => "Highlight colour",
        _ => "Text colour",
    };
    color_wheel_picker_dialog(widget, title)
}
