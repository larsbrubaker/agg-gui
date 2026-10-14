//! Pointer gestures that only exist in hosted-card mode (see
//! [`super::hosted`]): dragging a card's edge to resize it (the bands are
//! in `hosted_resize.rs`), and raising the card the user presses to the
//! top of the paint order.
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

use super::hosted_resize::{edge_at, ResizeDrag, ResizeEdge};
use super::NodeEditor;
use crate::draw::{NodeLayoutInfo, NODE_WIDTH};
use crate::model::NodeId;

impl NodeEditor {
    /// True while a card's edge is being dragged.
    pub(super) fn hosted_resizing(&self) -> bool {
        self.hosted.resize.is_some()
    }

    /// The card a press at `canvas` lands on, top first, and the resize
    /// band it is in (`None`: the drawn card). A card's drawn body beats
    /// the band of a card beneath it and the other way round, so the
    /// topmost of the two takes the press.
    fn hosted_hit(
        &self,
        layouts: &[NodeLayoutInfo],
        canvas: [f64; 2],
    ) -> Option<(NodeId, Option<ResizeEdge>)> {
        layouts.iter().rev().find_map(|l| {
            if l.body_contains(canvas) {
                return Some((l.node_id, None));
            }
            let g = self.hosted.cards.get(&l.node_id)?;
            let bottom = l.top_left[1] - l.size[1];
            let local = [canvas[0] - l.top_left[0], canvas[1] - bottom];
            let edge = edge_at(l.size[0], l.size[1], g.vertical.is_some(), local)?;
            Some((l.node_id, Some(edge)))
        })
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
        let Some((id, None)) = self.hosted_hit(&layouts, canvas) else {
            return EventResult::Ignored;
        };
        if !self.hosted.cards.contains_key(&id) || self.hit_socket(&layouts, canvas).is_some() {
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
                let (id, edge) = self.hosted_hit(&layouts, canvas)?;
                let card = self.hosted.cards.get(&id)?;
                let min_height = card.vertical.unwrap_or(0.0);
                let layout = layouts.iter().find(|l| l.node_id == id)?;
                let (start_height, start_top) = (layout.size[1], layout.top_left[1]);
                self.raise_card(id);
                // A socket straddling the edge keeps its outer half.
                let edge = edge.filter(|_| self.hit_socket(&layouts, canvas).is_none())?;
                let start_width = self.model.lock().unwrap().node_width(id);
                self.hosted.resize = Some(ResizeDrag {
                    node: id,
                    edge,
                    start_width: start_width.unwrap_or(NODE_WIDTH),
                    start_height,
                    start_top,
                    start_canvas: canvas,
                    min_height,
                });
                Some(EventResult::Consumed)
            }
            Event::MouseMove { pos } => {
                let drag = self.hosted.resize?;
                let canvas = self.local_to_canvas(*pos);
                drag.apply(&mut *self.model.lock().unwrap(), canvas);
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
