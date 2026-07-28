//! Colour controls for [`RichTextToolbar`](super::RichTextToolbar): the
//! text-colour and highlight swatch buttons and the floating colour-picker
//! overlay they open.
//!
//! A swatch button flips a shared [`PickerKind`] cell; the [`color_overlay`]
//! widget (a [`Rebuilder`]) watches that cell and shows a
//! [`color_wheel_picker_dialog_with_on_close`] while a swatch is open, driving a
//! **live preview** on the [`RichEditHandle`]:
//!
//! * open   → [`RichEditHandle::begin_preview`] (snapshot + suspend undo feed),
//! * drag   → `on_change` execs a fresh `SetTextColor` / `SetHighlight` so the
//!            selection recolours in context,
//! * Select → [`RichEditHandle::commit_preview`] banks the whole drag as one
//!            undo step,
//! * Cancel / × / Escape → [`RichEditHandle::cancel_preview`] restores the
//!            pre-dialog state.
//!
//! The picker dialog is a **modal** [`Window`](crate::widgets::window::Window),
//! so it paints through the clip-free global-overlay pass and clamps into the
//! viewport — the overlay can therefore live *inside* the (thin) toolbar widget
//! without being truncated by the toolbar's own child clip.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use crate::color::Color;
use crate::text::Font;
use crate::widget::Widget;
use crate::widgets::button::Button;
use crate::widgets::color_wheel_picker::{
    color_wheel_picker_dialog_with_on_close, ColorWheelPicker,
};
use crate::widgets::primitives::SizedBox;
use crate::widgets::rebuilder::Rebuilder;
use crate::widgets::window::CloseReason;

use super::super::commands::RichCommand;
use super::super::editor::RichEditHandle;
use super::PickerKind;

const ICON_TEXT_COLOR: &str = "\u{F031}"; // "font"
const ICON_HIGHLIGHT: &str = "\u{F043}"; // "tint"
const ICON_REMOVE_HIGHLIGHT: &str = "\u{F12D}"; // "eraser"

/// The text-colour swatch button (Font Awesome "font" glyph).
pub(super) fn text_color_button(
    font: &Arc<Font>,
    picker: &Rc<Cell<PickerKind>>,
) -> Box<dyn Widget> {
    color_button(font, ICON_TEXT_COLOR, picker, PickerKind::TextColor)
}

/// The highlight swatch button (Font Awesome "tint" glyph).
pub(super) fn highlight_button(font: &Arc<Font>, picker: &Rc<Cell<PickerKind>>) -> Box<dyn Widget> {
    color_button(font, ICON_HIGHLIGHT, picker, PickerKind::Highlight)
}

/// The Remove-highlight button (Font Awesome "eraser" glyph). A momentary action
/// that clears the selection's highlight through `SetHighlight(None)` — the sole
/// UI route to un-highlight text now that the colour picker dropped its "No
/// Color (Pass Through)" checkbox. Routes through the same `handle.exec` command
/// path the picker's apply uses (see [`apply_color`]).
pub(super) fn remove_highlight_button(
    font: &Arc<Font>,
    handle: &RichEditHandle,
) -> Box<dyn Widget> {
    let click_handle = handle.clone();
    Box::new(
        Button::new(ICON_REMOVE_HIGHLIGHT, Arc::clone(font))
            .with_font_size(13.0)
            .with_subtle()
            .with_active_fn(|| false)
            .on_click(move || click_handle.exec(&RichCommand::SetHighlight(None))),
    )
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
/// cell.  Placed as an internal child of the toolbar; the modal dialog it shows
/// paints through the global-overlay pass, so it renders over the editor even
/// though the toolbar is a thin strip.  Returns a zero-size layer while no
/// swatch is open.
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

/// Seed the wheel from the selection's current colour so the dialog opens on
/// what is already there (mirrors the demo).
fn initial_color(handle: &RichEditHandle, kind: PickerKind) -> Color {
    let common = handle.common_style_of_selection();
    match kind {
        PickerKind::TextColor => match common.text_color {
            Some(Some(c)) => c,
            _ => Color::rgb(0.2, 0.45, 0.88),
        },
        PickerKind::Highlight => match common.highlight {
            Some(Some(c)) => c,
            // No existing / mixed highlight: open on a visible default. (The old
            // alpha-0 seed pre-checked the "No Color" box; with that box gone,
            // `allow_none` is off and nothing can clear the picker's
            // `pass_through` flag — a zero-alpha seed would strand it emitting
            // `None` forever, so a highlight could never be applied.)
            _ => Color::rgb(1.0, 0.92, 0.23),
        },
        PickerKind::None => Color::rgb(0.2, 0.45, 0.88),
    }
}

/// Apply a picker colour to the selection for `kind`.  Text colour only applies
/// a concrete colour; highlight forwards `opt` through `SetHighlight`, which
/// clears the highlight on `None` (the picker no longer offers a "No Color"
/// choice here, so in practice `opt` is always `Some`).
fn apply_color(handle: &RichEditHandle, kind: PickerKind, opt: Option<Color>) {
    match kind {
        PickerKind::TextColor => {
            if let Some(c) = opt {
                handle.exec(&RichCommand::SetTextColor(c));
            }
        }
        PickerKind::Highlight => handle.exec(&RichCommand::SetHighlight(opt)),
        PickerKind::None => {}
    }
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
    // The "No Color (Pass Through)" checkbox is OFF for both rich-text pickers:
    // the owner reported it as confusing in this context. The core
    // `ColorWheelPicker` still supports it via `with_allow_none` for other hosts.
    let allow_none = false;

    // Snapshot the committed document + selection and suspend undo feeding for
    // the duration of the dialog: live previewing exec's a fresh colour on every
    // drag frame, so without this each mutation would seed an undo step and a
    // Cancel would strand a stray entry. `commit_preview` (Select) collapses the
    // drag into one step; `cancel_preview` (Cancel / × / Escape) restores this.
    handle.begin_preview();
    let initial = initial_color(handle, kind);

    let change_handle = handle.clone();
    let sel_handle = handle.clone();
    let cancel_handle = handle.clone();
    let close_handle = handle.clone();
    let sel_picker = Rc::clone(picker);
    let cancel_picker = Rc::clone(picker);
    let close_picker = Rc::clone(picker);

    let widget = ColorWheelPicker::new(initial, Arc::clone(font))
        .with_allow_none(allow_none)
        .with_show_alpha(true)
        .with_font_size(12.0)
        // Live preview: recolour the selection in context as the user drags.
        .on_change(move |opt| apply_color(&change_handle, kind, opt))
        // Select = commit: apply the final colour, then bank one undo step.
        .on_select(move |opt| {
            apply_color(&sel_handle, kind, opt);
            sel_handle.commit_preview();
            sel_picker.set(PickerKind::None);
            crate::animation::request_draw();
        })
        // Cancel button = restore the pre-dialog snapshot.
        .on_cancel(move || {
            cancel_handle.cancel_preview();
            cancel_picker.set(PickerKind::None);
            crate::animation::request_draw();
        });
    let title = match kind {
        PickerKind::Highlight => "Highlight colour",
        _ => "Text colour",
    };
    // The window's × button, Escape, and click-away close the dialog through a
    // route that bypasses the picker's Cancel button; forward each to the right
    // teardown so the preview session can't dangle:
    //   * click-away → commit the live change (one undo step) when the user
    //     actually recoloured, else close silently (no undo residue);
    //   * × / Escape → cancel, restoring the pre-dialog colour.
    color_wheel_picker_dialog_with_on_close(widget, title, move |reason| {
        match reason {
            CloseReason::ClickAway if close_handle.is_preview_dirty() => {
                close_handle.commit_preview();
            }
            _ => close_handle.cancel_preview(),
        }
        close_picker.set(PickerKind::None);
        crate::animation::request_draw();
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::rich_text::model::{Block, InlineStyle, RichDoc};
    use crate::widgets::rich_text::view::SharedResolver;
    use crate::widgets::rich_text::RichTextEdit;

    const FONT_BYTES: &[u8] = include_bytes!("../../../../../demo/assets/CascadiaCode.ttf");

    fn handle_over(doc: RichDoc) -> RichEditHandle {
        let font = Arc::new(Font::from_slice(FONT_BYTES).expect("test font loads"));
        let resolver: SharedResolver = Rc::new(move |_: &InlineStyle| Arc::clone(&font));
        let editor = RichTextEdit::new(doc, resolver);
        let handle = editor.handle();
        handle.select_all();
        handle
    }

    /// Regression: with the "No Color" checkbox removed (`allow_none` off), the
    /// highlight picker must NOT open in pass-through. It previously seeded an
    /// alpha-0 colour to pre-check that box; with the box gone nothing can clear
    /// the picker's `pass_through` flag, so a zero-alpha seed would strand it
    /// emitting `None` forever — the user could never apply a highlight to
    /// un-highlighted text. The seed must therefore be a visible default.
    #[test]
    fn highlight_seed_over_plain_text_is_visible_not_pass_through() {
        let handle = handle_over(RichDoc::from_blocks(vec![Block::plain("hi")]));
        // Sanity: a plain selection reports a uniform "no highlight".
        assert_eq!(
            handle.common_style_of_selection().highlight,
            Some(None),
            "plain text is uniformly un-highlighted"
        );

        let seed = initial_color(&handle, PickerKind::Highlight);
        assert!(
            seed.a > 0.0,
            "highlight seed must be opaque, got alpha {}",
            seed.a
        );

        // Build the picker exactly as production does (allow_none off) and prove
        // it is NOT stuck in pass-through — it yields a concrete colour.
        let font = Arc::new(Font::from_slice(FONT_BYTES).expect("test font loads"));
        let picker = ColorWheelPicker::new(seed, font)
            .with_allow_none(false)
            .with_show_alpha(true);
        assert!(
            picker.current_color().is_some(),
            "highlight picker must not open in pass-through (would emit None forever)"
        );
    }
}
