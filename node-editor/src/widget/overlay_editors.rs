//! Floating overlay editors spawned from a property-row click, plus the
//! numeric helpers they share with the canvas drag path.
//!
//! Split out of `widget/events.rs` (which kept the mouse / wheel / key
//! handlers) so both files stay under the project's 800-line guardrail.
//! As a submodule of `widget`, this file retains direct access to
//! `NodeEditor`'s private fields.
//!
//! Three row editors live here, all sharing one pattern: build an
//! agg-gui dialog whose callbacks route writes back through
//! `set_property` (live preview, coalesced into one undo step by hosts
//! that coalesce) and flip a shared close-flag the editor drains on the
//! next event / layout pass.
//!
//!   - [`NodeEditor::open_color_picker`] — `ColorWheelPicker` for a
//!     `Color` row.
//!   - [`NodeEditor::open_text_editor`] — single-line `TextField` for a
//!     `Text` row.
//!   - [`NodeEditor::open_number_editor`] — single-line numeric
//!     `TextField` for the keyboard-entry half of the `DragValue`
//!     contract on a NumberDrag row.
//!
//! [`scrub_value`] is the shared snap-then-clamp used both here (Enter /
//! live-preview commits) and by the drag path in `events.rs`.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::Color;

use crate::model::{NodeId, PropertyValue};

use super::NodeEditor;

impl NodeEditor {
    /// Spawn the [`agg_gui::ColorWheelPicker`] dialog as a floating
    /// overlay over the canvas.  The picker's callbacks route writes
    /// back through `set_property` for live preview / commit / cancel
    /// and flip a shared close-flag that the editor drains on the next
    /// event or layout pass.
    pub(super) fn open_color_picker(
        &mut self,
        node_id: NodeId,
        prop_name: String,
        initial: [f32; 4],
    ) {
        let Some(font) = agg_gui::font_settings::current_system_font() else {
            return;
        };
        let initial_color = Color::rgba(initial[0], initial[1], initial[2], initial[3]);
        let original = initial; // captured for `on_cancel` revert

        let model_change = Arc::clone(&self.model);
        let model_select = Arc::clone(&self.model);
        let model_cancel = Arc::clone(&self.model);
        let model_win = Arc::clone(&self.model);
        let name_change = prop_name.clone();
        let name_select = prop_name.clone();
        let name_win = prop_name.clone();
        let name_cancel = prop_name;
        let close_flag = Rc::new(Cell::new(false));
        let close_select = Rc::clone(&close_flag);
        let close_cancel = Rc::clone(&close_flag);
        let close_win = Rc::clone(&close_flag);

        let picker = agg_gui::ColorWheelPicker::new(initial_color, font.clone())
            .with_allow_none(false)
            .with_show_alpha(true)
            .on_change(move |c| {
                let value = color_to_property(c, original);
                model_change
                    .lock()
                    .unwrap()
                    .set_property(node_id, &name_change, value);
            })
            .on_select(move |c| {
                let value = color_to_property(c, original);
                model_select
                    .lock()
                    .unwrap()
                    .set_property(node_id, &name_select, value);
                close_select.set(true);
            })
            .on_cancel(move || {
                model_cancel.lock().unwrap().set_property(
                    node_id,
                    &name_cancel,
                    PropertyValue::Color(original),
                );
                close_cancel.set(true);
            });

        // Use the `_with_on_close` variant so the window-chrome dismissals
        // (× button, Escape, click-away) — a route entirely separate from
        // the picker's own OK / Cancel buttons — still flip the shared
        // close-flag. Without it a chrome dismissal left the overlay
        // stranded (its close-flag never set), and the stale overlay
        // swallowed the next colour-row click on another node. Escape / ×
        // are cancel-style and revert the live preview to the pre-edit
        // colour; a click-away keeps the previewed change (matches the
        // colour dialog's documented `CloseReason` semantics).
        let dialog = agg_gui::color_wheel_picker_dialog_with_on_close(
            picker,
            "Color Picker",
            move |reason| {
                if matches!(
                    reason,
                    agg_gui::CloseReason::Escape | agg_gui::CloseReason::CloseButton
                ) {
                    model_win.lock().unwrap().set_property(
                        node_id,
                        &name_win,
                        PropertyValue::Color(original),
                    );
                }
                close_win.set(true);
            },
        );

        // If a host sink is installed (AtomArtist's app shell does
        // this), hand the dialog off so it can live at the
        // screen-level Stack — that's what lets the user drag the
        // picker outside the editor pane. Otherwise fall back to the
        // legacy in-editor overlay path (gallery demo + tests rely
        // on this).
        if let Some(sink) = self.overlay_sink.as_mut() {
            sink(dialog, close_flag);
        } else {
            self.overlay = Some(dialog);
            self.overlay_close_flag = Some(close_flag);
        }
        self.backbuffer.invalidate();
        agg_gui::animation::request_draw();
    }

    /// Spawn a floating single-line [`agg_gui::TextField`] editor for a
    /// `Text` property row.  Mirrors [`Self::open_color_picker`]: the
    /// field's callbacks route writes back through `set_property` (live
    /// on each keystroke — coalesced into one undo step by hosts that
    /// coalesce, exactly like a slider drag) and flip a shared close-flag
    /// the editor drains on the next event / layout pass.
    ///
    /// Commit vs. cancel matches the colour picker's semantics: **Enter**
    /// commits the typed value, while a cancel-style dismissal (**Escape**,
    /// the × button, or a click-away) reverts the live-previewed edits to
    /// the pre-edit string. The revert runs from the window's `on_close`
    /// unless Enter already committed (tracked via a shared flag) so the
    /// commit isn't clobbered.
    pub(super) fn open_text_editor(
        &mut self,
        node_id: NodeId,
        prop_name: String,
        initial: String,
        pill_rect: [f64; 4],
    ) {
        let Some(font) = agg_gui::font_settings::current_system_font() else {
            return;
        };

        // Captured for the on_close revert — the value we restore when
        // the user cancels instead of pressing Enter.
        let original = initial.clone();

        let model_change = Arc::clone(&self.model);
        let model_enter = Arc::clone(&self.model);
        let model_cancel = Arc::clone(&self.model);
        let name_change = prop_name.clone();
        let name_enter = prop_name.clone();
        let name_cancel = prop_name;
        let close_flag = Rc::new(Cell::new(false));
        let close_enter = Rc::clone(&close_flag);
        let close_win = Rc::clone(&close_flag);
        // Set by the Enter path so the window's cancel-style `on_close`
        // (Escape / × / click-away) knows a commit already happened and
        // must NOT revert over it.
        let committed = Rc::new(Cell::new(false));
        let committed_enter = Rc::clone(&committed);
        let committed_win = Rc::clone(&committed);

        // Stable focus id so the field grabs the keyboard the instant
        // the overlay opens.  High sentinel bits keep it clear of other
        // focus-by-request ids the host app might use.
        let focus_id: agg_gui::focus::FocusId = 0xE1D0_0000_0000_0000 ^ node_id.0;

        let mut field = agg_gui::TextField::new(font.clone())
            .with_text(initial)
            .with_focus_id(focus_id)
            .on_change(move |s| {
                model_change.lock().unwrap().set_property(
                    node_id,
                    &name_change,
                    PropertyValue::Text(s.to_string()),
                );
            })
            .on_enter(move |s| {
                model_enter.lock().unwrap().set_property(
                    node_id,
                    &name_enter,
                    PropertyValue::Text(s.to_string()),
                );
                committed_enter.set(true);
                close_enter.set(true);
            });
        // Select the whole string on focus so the first keystroke
        // replaces it — the common "retype this label" case.
        field.select_all_on_focus = true;

        let dialog = self.build_inline_editor(field, pill_rect, move |reason| {
            // Escape reverts the pre-edit string (undoing any live
            // on_change preview); a click-away COMMITS — it keeps the live
            // value, matching the number editor and DragValue's
            // lose-focus-commits contract. Enter already committed, so its
            // flag suppresses the Escape revert too.
            if matches!(
                reason,
                agg_gui::CloseReason::Escape | agg_gui::CloseReason::CloseButton
            ) && !committed_win.get()
            {
                model_cancel.lock().unwrap().set_property(
                    node_id,
                    &name_cancel,
                    PropertyValue::Text(original.clone()),
                );
            }
            close_win.set(true);
        });

        if let Some(sink) = self.overlay_sink.as_mut() {
            sink(dialog, close_flag);
        } else {
            self.overlay = Some(dialog);
            self.overlay_close_flag = Some(close_flag);
        }
        agg_gui::focus::request_focus(focus_id);
        self.backbuffer.invalidate();
        agg_gui::animation::request_draw();
    }

    /// Spawn a floating single-line numeric editor for a NumberDrag
    /// property row that was clicked without dragging.  This is the
    /// keyboard-entry half of the [`agg_gui::widgets::DragValue`]
    /// contract, reusing the same floating [`agg_gui::TextField`] overlay
    /// machinery as [`Self::open_text_editor`] — mounting a real focusable
    /// child widget in the retained, canvas-space back-buffer paint path
    /// isn't practical, so the row mirrors DragValue's look (arrows +
    /// centred value, painted by `slider::paint_editor_drag`) and its
    /// interaction contract (3px drag threshold = scrub, plain click =
    /// edit) rather than embedding the widget itself.
    ///
    /// Value flow matches the text editor exactly: live `on_change`
    /// previews (coalesced into one undo step by hosts that coalesce),
    /// **Enter** commits the parsed value after snap + clamp, and a
    /// cancel-style dismissal (**Escape**, ×, click-away) reverts to the
    /// pre-edit value.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn open_number_editor(
        &mut self,
        node_id: NodeId,
        prop_name: String,
        original_value: f64,
        min: Option<f64>,
        max: Option<f64>,
        step: Option<f64>,
        decimals: usize,
        pill_rect: [f64; 4],
    ) {
        let Some(font) = agg_gui::font_settings::current_system_font() else {
            return;
        };

        let initial = format_editor_number(original_value, decimals);

        let model_change = Arc::clone(&self.model);
        let model_enter = Arc::clone(&self.model);
        let model_cancel = Arc::clone(&self.model);
        let name_change = prop_name.clone();
        let name_enter = prop_name.clone();
        let name_cancel = prop_name;
        let close_flag = Rc::new(Cell::new(false));
        let close_enter = Rc::clone(&close_flag);
        let close_win = Rc::clone(&close_flag);
        let committed = Rc::new(Cell::new(false));
        let committed_enter = Rc::clone(&committed);
        let committed_win = Rc::clone(&committed);

        let focus_id: agg_gui::focus::FocusId = 0xE2D0_0000_0000_0000 ^ node_id.0;

        let mut field = agg_gui::TextField::new(font.clone())
            .with_text(initial)
            .with_focus_id(focus_id)
            .on_change(move |s| {
                // Live preview: only push a parseable value (skips
                // in-progress input like "" / "-" / "1."). Snap + clamp so
                // the preview matches what Enter will commit.
                if let Ok(raw) = s.trim().parse::<f64>() {
                    let v = scrub_value(raw, 0.0, step, min, max);
                    model_change.lock().unwrap().set_property(
                        node_id,
                        &name_change,
                        PropertyValue::Number(v),
                    );
                }
            })
            .on_enter(move |s| {
                if let Ok(raw) = s.trim().parse::<f64>() {
                    let v = scrub_value(raw, 0.0, step, min, max);
                    model_enter.lock().unwrap().set_property(
                        node_id,
                        &name_enter,
                        PropertyValue::Number(v),
                    );
                } else {
                    // Unparseable text on Enter is treated like Escape:
                    // revert to the pre-edit value, discarding any live
                    // preview left by the last parseable `on_change`.
                    model_enter.lock().unwrap().set_property(
                        node_id,
                        &name_enter,
                        PropertyValue::Number(original_value),
                    );
                }
                committed_enter.set(true);
                close_enter.set(true);
            });
        field.select_all_on_focus = true;

        let dialog = self.build_inline_editor(field, pill_rect, move |reason| {
            // Escape reverts to the pre-edit value (undoing any live
            // preview); a click-away COMMITS — it keeps whatever the live
            // `on_change` last wrote, matching DragValue's lose-focus-commits
            // contract. Enter already committed via `on_enter`, so its flag
            // suppresses the Escape revert too.
            if matches!(
                reason,
                agg_gui::CloseReason::Escape | agg_gui::CloseReason::CloseButton
            ) && !committed_win.get()
            {
                model_cancel.lock().unwrap().set_property(
                    node_id,
                    &name_cancel,
                    PropertyValue::Number(original_value),
                );
            }
            close_win.set(true);
        });

        if let Some(sink) = self.overlay_sink.as_mut() {
            sink(dialog, close_flag);
        } else {
            self.overlay = Some(dialog);
            self.overlay_close_flag = Some(close_flag);
        }
        agg_gui::focus::request_focus(focus_id);
        self.backbuffer.invalidate();
        agg_gui::animation::request_draw();
    }

    /// Wrap a chrome-less [`agg_gui::TextField`] in a frameless, modal
    /// [`agg_gui::Window`] positioned exactly over the clicked value pill.
    /// Shared by the number and string inline editors: both want an in-place
    /// borderless field with the same dismissal contract (Enter commits via
    /// the field's own `on_enter`; Escape reverts; click-away commits — the
    /// per-editor `on_close` closure distinguishes the reasons).
    ///
    /// Placement space depends on where the overlay lands: an installed
    /// `overlay_sink` hoists the dialog to a screen-level host, so it needs
    /// **app-absolute** bounds; the in-editor fallback paints in the pane's
    /// own local space.
    fn build_inline_editor(
        &self,
        field: agg_gui::TextField,
        pill_rect: [f64; 4],
        on_close: impl FnMut(agg_gui::CloseReason) + 'static,
    ) -> Box<dyn agg_gui::Widget> {
        let font = agg_gui::font_settings::current_system_font()
            .expect("caller already verified the system font is installed");
        let pill = if self.overlay_sink.is_some() {
            self.pill_abs_rect(pill_rect)
        } else {
            self.pill_local_rect(pill_rect)
        };
        // The pill rect is zoom-scaled, so a heavy zoom-out shrinks it to a
        // few px — too small to click into or read. Grow to a minimum
        // readable size, centred on the pill so the field stays over the
        // value it edits. The window's `with_constrain(true)` still keeps
        // the (possibly enlarged) rect inside the viewport at layout time.
        const MIN_W: f64 = 60.0;
        const MIN_H: f64 = 20.0;
        let mut bounds = pill;
        if bounds.width < MIN_W {
            bounds.x = pill.center().x - MIN_W * 0.5;
            bounds.width = MIN_W;
        }
        if bounds.height < MIN_H {
            bounds.y = pill.center().y - MIN_H * 0.5;
            bounds.height = MIN_H;
        }
        Box::new(
            agg_gui::Window::new("inline-editor", font, Box::new(field))
                .with_chrome(false)
                .with_bounds(bounds)
                .with_min_size(agg_gui::Size::new(bounds.width, bounds.height))
                .with_resizable(false)
                .with_constrain(true)
                .with_modal(true)
                .with_click_away(agg_gui::ClickAwayAction::Close)
                .on_close(on_close),
        )
    }
}

/// Compute a scrubbed numeric value from a drag: `start + dx`, snapped to
/// `step` (when set) then clamped to `[min, max]`. Snap-then-clamp matches
/// [`agg_gui::widgets::DragValue`]'s `apply_step_and_clamp` so a NumberDrag
/// row and the standalone widget produce the same numbers.
///
/// `min` / `max` may arrive from mixed sources — the model's
/// `PropertyView` and the `NumberAttrs` fall-back are merged at the
/// `DraggingProperty` call site — so an inverted `min > max` pair is
/// possible if a host declares contradictory bounds. We resolve it
/// deliberately and without panicking: the manual `< min` then `> max`
/// compares apply the max clamp last, so an inverted pair pins the value
/// to `max`. This is intentionally *not* `f64::clamp`, which panics when
/// `min > max` (and misbehaves on NaN bounds). A `debug_assert` surfaces
/// the contradiction in debug builds without weaponising it in release.
pub(super) fn scrub_value(
    start: f64,
    dx: f64,
    step: Option<f64>,
    min: Option<f64>,
    max: Option<f64>,
) -> f64 {
    debug_assert!(
        match (min, max) {
            (Some(mn), Some(mx)) => mn <= mx,
            _ => true,
        },
        "scrub_value got inverted bounds (min > max); resolving to max"
    );
    let mut v = start + dx;
    if let Some(st) = step {
        if st > 0.0 {
            v = (v / st).round() * st;
        }
    }
    if let Some(mn) = min {
        if v < mn {
            v = mn;
        }
    }
    if let Some(mx) = max {
        if v > mx {
            v = mx;
        }
    }
    v
}

/// Format a numeric value for the inline keyboard editor's initial text.
/// Integer rows (`decimals == 0`) drop the fractional part; otherwise the
/// value is shown with a fixed number of decimals, matching DragValue.
fn format_editor_number(v: f64, decimals: usize) -> String {
    if decimals == 0 {
        format!("{}", v.round() as i64)
    } else {
        format!("{:.*}", decimals, v)
    }
}

/// Pack a picker-side `Option<Color>` back into a `PropertyValue::Color`,
/// falling back to `original.a = 0.0` for the pass-through ("No Color")
/// case so hosts that don't model pass-through still see a sensible
/// zero-alpha colour.
fn color_to_property(c: Option<Color>, original: [f32; 4]) -> PropertyValue {
    match c {
        Some(col) => PropertyValue::Color([col.r, col.g, col.b, col.a]),
        None => PropertyValue::Color([original[0], original[1], original[2], 0.0]),
    }
}
