//! Pointer gestures that only exist in hosted-card mode (see
//! [`super::hosted`]): dragging a card's right edge to resize it, and
//! raising the card the user presses to the top of the paint order.
//!
//! [`NodeEditor::hosted_preview_event`] is the editor's
//! `Widget::preview_event`: the framework offers it a press before the
//! card's body widget sees it, so a press on a slider or text field inside
//! a card selects and raises the card (as a press anywhere in a MatterCAD
//! node card does, through agg-sharp's `MouseDown` firing on every widget
//! under the pointer) while the slider still gets the press.
//!
//! [`NodeEditor::hosted_on_event`] runs at the start of the editor's
//! `on_event`, before the regular handlers in `events.rs`; it consumes
//! only the resize gesture and otherwise lets the event continue, so a
//! press on a card still selects and drags it as on a simplified card.

use agg_gui::{Event, EventResult, MouseButton};

use super::hosted::{ResizeDrag, MIN_HOSTED_CARD_WIDTH};
use super::hosted_card::RESIZE_GRIP;
use super::NodeEditor;
use crate::draw::{NODE_WIDTH, TITLE_HEIGHT};

impl NodeEditor {
    /// True while a card's right edge is being dragged.
    pub(super) fn hosted_resizing(&self) -> bool {
        self.hosted.resize.is_some()
    }

    /// Preview of a press routed into the hosted layer: a left press on a
    /// card's body or title bar selects the card (the same rule as a press
    /// that reaches the editor: Shift adds, a press on a selected card keeps
    /// the selection) and raises it (unless `with_raise_on_click(false)`).
    /// Sockets and the resize band are left to
    /// the editor's own handlers. Never consumes. Selection order only
    /// changes `hosted.order`; the layer is re-sorted at the next layout, so
    /// the dispatch path in flight stays valid.
    pub(super) fn hosted_preview_event(&mut self, event: &Event) -> EventResult {
        let Event::MouseDown {
            pos,
            button: MouseButton::Left,
            modifiers,
        } = event
        else {
            return EventResult::Ignored;
        };
        if self.hosted.factory.is_none() || self.overlay.is_some() || self.popup.is_open() {
            return EventResult::Ignored;
        }
        let canvas = self.local_to_canvas(*pos);
        let layouts = self.snapshot_layouts();
        let Some(top) = layouts.iter().rev().find(|l| l.body_contains(canvas)) else {
            return EventResult::Ignored;
        };
        let id = top.node_id;
        if !self.hosted.cards.contains_key(&id) || self.hit_socket(&layouts, canvas).is_some() {
            return EventResult::Ignored;
        }
        let right = top.top_left[0] + top.size[0];
        if canvas[0] >= right - RESIZE_GRIP && canvas[1] < top.top_left[1] - TITLE_HEIGHT {
            return EventResult::Ignored;
        }
        let before = self.selected.clone();
        if !modifiers.shift && !self.selected.contains(&id) {
            self.selected.clear();
        }
        self.selected.insert(id);
        if self.selected != before {
            self.notify_primary_selection(Some(id));
            self.backbuffer.invalidate();
            agg_gui::animation::request_draw();
        }
        self.raise_card(id);
        EventResult::Ignored
    }

    /// Hosted-card handling ahead of the regular handlers: `Some` when the
    /// event was used up here.
    pub(super) fn hosted_on_event(&mut self, event: &Event) -> Option<EventResult> {
        self.hosted.factory.as_ref()?;
        match event {
            Event::MouseDown {
                pos,
                button: MouseButton::Left,
                ..
            } => {
                let canvas = self.local_to_canvas(*pos);
                let layouts = self.snapshot_layouts();
                let top = layouts.iter().rev().find(|l| l.body_contains(canvas))?;
                if !self.hosted.cards.contains_key(&top.node_id) {
                    return None;
                }
                let id = top.node_id;
                self.raise_card(id);
                let right = top.top_left[0] + top.size[0];
                let in_band = canvas[0] >= right - RESIZE_GRIP
                    && canvas[1] < top.top_left[1] - TITLE_HEIGHT
                    && self.hit_socket(&layouts, canvas).is_none();
                if !in_band {
                    return None;
                }
                let start_width = self.model.lock().unwrap().node_width(id);
                self.hosted.resize = Some(ResizeDrag {
                    node: id,
                    start_width: start_width.unwrap_or(NODE_WIDTH),
                    start_canvas_x: canvas[0],
                });
                Some(EventResult::Consumed)
            }
            Event::MouseMove { pos } => {
                let drag = self.hosted.resize?;
                let canvas = self.local_to_canvas(*pos);
                let width =
                    (drag.start_width + canvas[0] - drag.start_canvas_x).max(MIN_HOSTED_CARD_WIDTH);
                self.model.lock().unwrap().set_node_width(drag.node, width);
                agg_gui::animation::request_draw();
                Some(EventResult::Consumed)
            }
            Event::MouseUp { .. } => {
                self.hosted.resize.take()?;
                agg_gui::animation::request_draw();
                Some(EventResult::Consumed)
            }
            _ => None,
        }
    }
}
