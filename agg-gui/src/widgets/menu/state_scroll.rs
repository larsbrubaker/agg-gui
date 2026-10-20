//! Scrolling an overflowing popup panel: the wheel, the scroll bar, a finger
//! drag and the keyboard highlight, on [`PopupMenuState`].
//!
//! `geometry.rs` / `overflow.rs` decide which panels scroll and clamp each
//! stored offset into range; this file stores the offsets (one per level,
//! keyed by the level's path prefix) and moves them.  Ports the behaviour of
//! agg-sharp's `PopupWidget` scroll host (`ScrollableWidget` with its
//! `ScrollIntoView` minimum scroll and `PopupMenuScrollWindow`).

use crate::event::{Event, EventResult, MouseButton};
use crate::geometry::{Point, Size};
use crate::widgets::scrollbar::minimum_scroll_to_show;

use super::super::geometry::contains;
use super::super::model::MenuEntry;
use super::PopupMenuState;

/// Logical px one wheel notch scrolls, as the combo box and scroll views.
const WHEEL_STEP: f64 = 40.0;

/// A drag scrolling one level: the level, what is dragged, the pointer y at
/// the press and the offset then.
#[derive(Clone, Debug, PartialEq)]
pub(in super::super) struct ScrollDrag {
    prefix: Vec<usize>,
    by: DragBy,
    grab_y: f64,
    start_offset: f64,
}

/// What a [`ScrollDrag`] holds, which decides how the pointer's travel maps
/// to the offset and which release ends it.
#[derive(Clone, Copy, Debug, PartialEq)]
enum DragBy {
    /// The scroll bar's thumb, under the left button: the thumb follows the
    /// pointer, so the rows move the other way, scaled by content / track.
    Bar,
    /// A finger on the rows (the touch layer's middle press): the rows
    /// follow the finger one for one, as a phone's lists do.
    Finger,
}

impl DragBy {
    fn button(self) -> MouseButton {
        match self {
            DragBy::Bar => MouseButton::Left,
            DragBy::Finger => MouseButton::Middle,
        }
    }
}

impl PopupMenuState {
    /// The stored scroll offset of the level at `prefix` (0 at its first
    /// row).  Layout clamps it into the panel's range.
    pub fn scroll_offset(&self, prefix: &[usize]) -> f64 {
        self.scroll_offsets
            .iter()
            .find(|(p, _)| p == prefix)
            .map_or(0.0, |(_, offset)| *offset)
    }

    /// Store `offset` for the level at `prefix`, dropping the offsets of
    /// levels no longer open (a submenu reopened starts at its first row,
    /// as agg-sharp builds it afresh).
    pub fn set_scroll_offset(&mut self, prefix: &[usize], offset: f64) {
        let open_path = self.open_path.clone();
        self.scroll_offsets
            .retain(|(p, _)| p != prefix && open_path.starts_with(p));
        self.scroll_offsets.push((prefix.to_vec(), offset));
    }

    /// The wheel over an open popup: scroll the topmost panel under `pos` if
    /// it scrolls.  Consumed over any panel, so the wheel never reaches the
    /// content behind an open menu; ignored elsewhere.
    pub(super) fn handle_wheel(
        &mut self,
        items: &[MenuEntry],
        pos: Point,
        delta_y: f64,
        viewport: Size,
    ) -> EventResult {
        let layouts = self.layouts(items, viewport);
        let Some(layout) = layouts.iter().rev().find(|l| contains(l.rect, pos)) else {
            return EventResult::Ignored;
        };
        if let Some(scroll) = layout.scroll {
            // Positive delta_y (wheel forward) shows the content above.
            let max = scroll.max_offset(layout.rect.height);
            let next = (scroll.offset - delta_y * WHEEL_STEP).clamp(0.0, max);
            if (next - scroll.offset).abs() > 1e-9 {
                let prefix = layout.path_prefix.clone();
                self.set_scroll_offset(&prefix, next);
                // The row under the pointer changed.
                self.update_hover(items, pos, viewport);
                crate::animation::request_draw();
            }
        }
        EventResult::Consumed
    }

    /// True while a finger drag is scrolling a panel, so hosts route its
    /// events to the menu rather than to a widget row under the finger.
    pub(in super::super) fn is_finger_scrolling(&self) -> bool {
        self.scroll_drag
            .as_ref()
            .is_some_and(|drag| drag.by == DragBy::Finger)
    }

    /// Bring the highlighted row fully into its panel with the least
    /// movement (C# `ScrollableWidget.ScrollIntoView`'s minimum scroll); a
    /// row already in view leaves the panel where it is.
    pub(super) fn reveal_hover(&mut self, items: &[MenuEntry], viewport: Size) {
        let Some((&idx, prefix)) = self.hover_path.as_ref().and_then(|p| p.split_last()) else {
            return;
        };
        let prefix = prefix.to_vec();
        let layouts = self.layouts(items, viewport);
        let Some(layout) = layouts.iter().find(|l| l.path_prefix == prefix) else {
            return;
        };
        let Some(scroll) = layout.scroll else {
            return;
        };
        let Some(row) = layout.rows.iter().find(|r| r.item_index == Some(idx)) else {
            return;
        };
        let wanted = minimum_scroll_to_show(
            scroll.offset,
            layout.rect.y,
            layout.rect.y + layout.rect.height,
            row.rect,
        )
        .clamp(0.0, scroll.max_offset(layout.rect.height));
        if (wanted - scroll.offset).abs() > 1e-9 {
            self.set_scroll_offset(&prefix, wanted);
        }
    }

    /// Drag scrolling.  A left press on a panel's scroll bar strip starts a
    /// thumb drag; a finger drag that starts on a panel that scrolls (the
    /// touch layer offers it as a middle press with
    /// [`crate::touch_emulation::touch_pan_travel`] set) moves the rows with
    /// the finger.  Moves scroll, clamped to the panel's range as the wheel
    /// is; the release of the pressed button, or a lost capture, ends it.
    /// A finger drag never picks a row: its press, moves and release are all
    /// taken here.  Returns `None` for events this does not take, which
    /// leaves a finger drag on a panel that fits to `handle_event`.
    pub(super) fn handle_scroll_bar(
        &mut self,
        items: &[MenuEntry],
        event: &Event,
        viewport: Size,
    ) -> Option<EventResult> {
        match event {
            Event::MouseDown {
                pos,
                button: MouseButton::Left,
                ..
            } => {
                let layouts = self.layouts(items, viewport);
                let layout = layouts.iter().rev().find(|l| contains(l.rect, *pos))?;
                let scroll = layout.scroll.filter(|s| contains(s.bar, *pos))?;
                self.scroll_drag = Some(ScrollDrag {
                    prefix: layout.path_prefix.clone(),
                    by: DragBy::Bar,
                    grab_y: pos.y,
                    start_offset: scroll.offset,
                });
                Some(EventResult::Consumed)
            }
            Event::MouseDown {
                pos,
                button: MouseButton::Middle,
                ..
            } if crate::touch_emulation::touch_pan_travel().is_some() => {
                let layouts = self.layouts(items, viewport);
                let layout = layouts.iter().rev().find(|l| contains(l.rect, *pos))?;
                let scroll = layout.scroll?;
                self.scroll_drag = Some(ScrollDrag {
                    prefix: layout.path_prefix.clone(),
                    by: DragBy::Finger,
                    grab_y: pos.y,
                    start_offset: scroll.offset,
                });
                // The finger is scrolling, not choosing.
                self.hover_path = None;
                crate::animation::request_draw();
                Some(EventResult::Consumed)
            }
            Event::MouseMove { pos } => {
                let drag = self.scroll_drag.clone()?;
                let layouts = self.layouts(items, viewport);
                let Some(layout) = layouts.iter().find(|l| l.path_prefix == drag.prefix) else {
                    self.scroll_drag = None;
                    return None;
                };
                let scroll = layout.scroll?;
                let moved = match drag.by {
                    DragBy::Bar => {
                        let h = layout.rect.height.max(1.0);
                        // The thumb spans h * h / content of an h tall track,
                        // so a pointer move of d moves the rows d * content / h.
                        (drag.grab_y - pos.y) * scroll.content_height / h
                    }
                    // Y-up: a finger moving up carries the rows up, showing
                    // the ones below (a larger offset).
                    DragBy::Finger => pos.y - drag.grab_y,
                };
                let next =
                    (drag.start_offset + moved).clamp(0.0, scroll.max_offset(layout.rect.height));
                self.set_scroll_offset(&drag.prefix, next);
                crate::animation::request_draw();
                Some(EventResult::Consumed)
            }
            Event::MouseUp { button, .. }
                if self
                    .scroll_drag
                    .as_ref()
                    .is_some_and(|drag| drag.by.button() == *button) =>
            {
                self.scroll_drag = None;
                Some(EventResult::Consumed)
            }
            Event::MouseCaptureLost => self.scroll_drag.take().map(|_| EventResult::Consumed),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::event::{Event, Modifiers, MouseButton};
    use crate::geometry::{Point, Size};

    use super::super::super::model::{MenuEntry, MenuItem};
    use super::super::MenuAnchorKind;
    use super::super::PopupMenuState;

    const WINDOW: Size = Size {
        width: 400.0,
        height: 300.0,
    };

    fn tall() -> Vec<MenuEntry> {
        (0..20)
            .map(|i| {
                MenuEntry::Item(
                    MenuItem::action(format!("Row {i}"), format!("{i}")).row_height(48.0),
                )
            })
            .collect()
    }

    fn open() -> PopupMenuState {
        let mut state = PopupMenuState::default();
        state.open_at(Point::new(10.0, 290.0), MenuAnchorKind::Context);
        state
    }

    fn offset(state: &PopupMenuState, items: &[MenuEntry]) -> f64 {
        state.layouts(items, WINDOW)[0]
            .scroll
            .expect("scrolls")
            .offset
    }

    #[test]
    fn rust_only_wheel_scrolls_an_overflowing_panel_within_its_range() {
        let _guard = crate::input_profile::profile_test_lock();
        crate::input_profile::set_input_profile(crate::input_profile::InputProfile::Desktop);
        crate::touch_state::clear_last_touch_event_for_testing();
        let mut items = tall();
        let mut state = open();
        let wheel = |delta_y: f64| Event::MouseWheel {
            pos: Point::new(50.0, 150.0),
            delta_y,
            delta_x: 0.0,
            modifiers: Modifiers::default(),
        };
        // Wheel back (negative) shows the rows below
        state.handle_event(&mut items, &wheel(-1.0), WINDOW);
        assert_eq!(offset(&state, &items), 40.0);
        // Never past either end
        state.handle_event(&mut items, &wheel(100.0), WINDOW);
        assert_eq!(offset(&state, &items), 0.0);
        state.handle_event(&mut items, &wheel(-100.0), WINDOW);
        let layout = state.layouts(&items, WINDOW).remove(0);
        let scroll = layout.scroll.expect("scrolls");
        assert_eq!(scroll.offset, scroll.content_height - layout.rect.height);
        // The last row then sits on the panel's bottom edge
        assert_eq!(layout.rows[19].rect.y, layout.rect.y);
    }

    #[test]
    fn rust_only_dragging_the_scroll_bar_scrolls_and_clicks_on_it_activate_nothing() {
        let _guard = crate::input_profile::profile_test_lock();
        crate::input_profile::set_input_profile(crate::input_profile::InputProfile::Desktop);
        crate::touch_state::clear_last_touch_event_for_testing();
        let mut items = tall();
        let mut state = open();
        let layout = state.layouts(&items, WINDOW).remove(0);
        let bar = layout.scroll.expect("scrolls").bar;
        let x = bar.x + bar.width * 0.5;
        let top = bar.y + bar.height - 10.0;
        let (_, response) = state.handle_event(
            &mut items,
            &Event::MouseDown {
                pos: Point::new(x, top),
                button: MouseButton::Left,
                modifiers: Modifiers::default(),
            },
            WINDOW,
        );
        assert_eq!(response, super::super::MenuResponse::None);
        assert!(state.open, "a press on the scroll bar keeps the menu open");
        // Dragging the thumb down by 10 % of the track scrolls 10 % of the content
        let h = layout.rect.height;
        state.handle_event(
            &mut items,
            &Event::MouseMove {
                pos: Point::new(x, top - h * 0.1),
            },
            WINDOW,
        );
        let content = layout.scroll.expect("scrolls").content_height;
        assert!((offset(&state, &items) - content * 0.1).abs() < 1e-6);
        state.handle_event(
            &mut items,
            &Event::MouseUp {
                pos: Point::new(x, top - h * 0.1),
                button: MouseButton::Left,
                modifiers: Modifiers::default(),
            },
            WINDOW,
        );
        // Released: a move no longer scrolls
        let before = offset(&state, &items);
        state.handle_event(
            &mut items,
            &Event::MouseMove {
                pos: Point::new(x, 20.0),
            },
            WINDOW,
        );
        assert_eq!(offset(&state, &items), before);
    }

    #[test]
    fn rust_only_a_reopened_menu_starts_at_its_first_row() {
        let mut state = PopupMenuState::default();
        state.open_at(Point::new(10.0, 290.0), MenuAnchorKind::Context);
        state.set_scroll_offset(&[], 120.0);
        assert_eq!(state.scroll_offset(&[]), 120.0);
        state.close();
        state.open_at(Point::new(10.0, 290.0), MenuAnchorKind::Context);
        assert_eq!(state.scroll_offset(&[]), 0.0);
    }
}
