//! `RichMarkdownEditWidget`'s agg-gui side — its `Widget` implementation: the
//! event routing of agg-sharp's `RichMarkdownEditWidget.cs` (`OnMouseDown`,
//! `OnMouseMove`, `OnMouseUp`, `OnKeyDown`, `OnContainsFocusChanged`), the
//! scroll bar C#'s `ScrollableWidget` drew (presses over it stay with it,
//! C#'s `OverText`), the auto-scroll timer (`RichEditorMouse`'s
//! `UiThread.RunOnIdle(AutoScrollStep, 0.05)`) and painting
//! (`RichDocumentView.OnDraw`: a stale style relays out before the paint).
//! The widget's state and API are in `edit_widget`.
//!
//! Events go to the state in the widget's own coordinates (y up, 0 at its
//! bottom), the editor coordinates `editor_state` works in.  The click count
//! is agg-gui's [`current_click_count`]; the command key is Cmd with mac
//! bindings and Ctrl otherwise (the state reads the modifiers).  A layout or
//! edit error (a block that will not lay out) is not the user's to see: the
//! event is still consumed and the editor keeps its last good layout, as C#'s
//! exception-free paths did.

use std::time::Duration;

use crate::cursor::{set_cursor_icon, CursorIcon};
use crate::draw_ctx::DrawCtx;
use crate::event::{current_click_count, current_modifiers, Event, EventResult, Key, MouseButton};
use crate::geometry::{Point, Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::widget::Widget;
use crate::widgets::scroll_view::{current_scroll_style, current_scroll_visibility};
use crate::widgets::scrollbar::{
    paint_prepared_scrollbar, ScrollbarGeometry, ScrollbarOrientation, DEFAULT_GRAB_MARGIN,
};

use super::document_view::RichViewPaint;
use super::edit_widget::RichMarkdownEditWidget;
use super::selection::RichSelection;

/// How often a drag held past the edge scrolls (C#'s 0.05 s).
const AUTO_SCROLL_INTERVAL: Duration = Duration::from_millis(50);

impl RichMarkdownEditWidget {
    fn scroll_geometry(&self) -> ScrollbarGeometry {
        ScrollbarGeometry {
            orientation: ScrollbarOrientation::Vertical,
            track_start: 0.0,
            track_end: self.bounds.height.max(0.0),
            cross_end: (self.bounds.width - 2.0).max(0.0),
            hit_margin: DEFAULT_GRAB_MARGIN,
        }
    }

    // The scroll bar reads the state's offset and content height before every use.
    fn sync_scroll_bar(&mut self) {
        self.vbar.enabled = true;
        self.vbar.content = self.state.view.height();
        self.vbar.offset = self.state.scroll_offset_from_top();
    }

    /// Whether `pos` is over the scroll bar (C#'s `OverText` is false there).
    fn over_scroll_bar(&mut self, pos: Point) -> bool {
        self.sync_scroll_bar();
        let viewport = self.bounds.height;
        self.vbar.can_scroll(viewport)
            && self
                .vbar
                .pos_in_hover(pos, current_scroll_style(), self.scroll_geometry())
    }

    fn apply_scroll_bar(&mut self) {
        let offset = self.vbar.offset;
        self.set_scroll_offset_from_top(offset);
    }

    fn mouse_down(
        &mut self,
        pos: Point,
        button: MouseButton,
        modifiers: crate::event::Modifiers,
    ) -> EventResult {
        if button == MouseButton::Left && self.over_scroll_bar(pos) {
            let (viewport, style, geom) = (
                self.bounds.height,
                current_scroll_style(),
                self.scroll_geometry(),
            );
            if !self.vbar.begin_drag(pos, viewport, style, geom)
                && self.vbar.page_at(pos, viewport, style, geom)
            {
                self.apply_scroll_bar();
            }
            return EventResult::Consumed;
        }
        let clicks = current_click_count().max(1);
        let _ = self.state.mouse_down(pos, button, clicks, modifiers);
        self.after_input();
        EventResult::Consumed
    }

    fn mouse_move(&mut self, pos: Point) -> EventResult {
        self.sync_scroll_bar();
        let (viewport, style, geom) = (
            self.bounds.height,
            current_scroll_style(),
            self.scroll_geometry(),
        );
        if self.vbar.dragging {
            if self.vbar.drag_to(pos, viewport, style, geom) {
                self.apply_scroll_bar();
            }
            return EventResult::Consumed;
        }
        let hover_changed = self.vbar.update_hover(pos, viewport, style, geom);
        self.state.mouse_move(pos);
        if self.state.auto_scrolling() && self.auto_scroll_due.is_none() {
            self.arm_auto_scroll();
        }
        if self.vbar.hovered_bar {
            set_cursor_icon(CursorIcon::Default);
        } else {
            set_cursor_icon(self.state.cursor_at(pos, current_modifiers()));
        }
        self.after_input();
        if hover_changed {
            EventResult::Consumed
        } else {
            EventResult::ConsumedQuiet
        }
    }

    fn mouse_up(&mut self) -> EventResult {
        if self.vbar.dragging {
            self.vbar.dragging = false;
            return EventResult::Consumed;
        }
        self.state.mouse_up();
        self.after_input();
        EventResult::Consumed
    }

    fn key_down(&mut self, key: &Key, modifiers: crate::event::Modifiers) -> EventResult {
        let page = (self.bounds.height - 16.0).max(20.0);
        let page_step = match key {
            Key::PageUp => Some(-page),
            Key::PageDown => Some(page),
            _ => None,
        };
        if let Some(step) = page_step {
            // The editor leaves PageUp/PageDown to its scroll area, as C# did.
            let to = self.state.scroll_offset_from_top() + step;
            self.set_scroll_offset_from_top(to);
            return EventResult::Consumed;
        }
        let handled = self.state.key_down(key, modifiers).unwrap_or(true);
        self.after_input();
        if handled {
            EventResult::Consumed
        } else {
            EventResult::Ignored
        }
    }

    // After any input: the Raw hosts follow the document, and a placed selection restarts the blink.
    fn after_input(&mut self) {
        self.sync_raw_hosts();
        self.restart_blink_on_placement();
    }

    fn restart_blink_on_placement(&mut self) {
        let placements = self.state.placements();
        if placements != self.seen_placements {
            self.seen_placements = placements;
            self.blink.restart(self.focused);
        }
    }

    fn arm_auto_scroll(&mut self) {
        self.auto_scroll_due = Some(crate::clock::now() + AUTO_SCROLL_INTERVAL);
        crate::animation::request_draw_after_tagged(
            AUTO_SCROLL_INTERVAL,
            "markdown_editing::auto_scroll",
        );
    }

    /// Runs the auto-scroll step once it is due, re-arming while it scrolls.
    pub fn run_due_auto_scroll(&mut self) {
        let Some(due) = self.auto_scroll_due else {
            return;
        };
        if crate::clock::now() < due {
            return;
        }
        self.auto_scroll_due = None;
        if self.state.auto_scroll_step() {
            self.arm_auto_scroll();
        }
        self.place_raw_hosts();
    }

    /// Whether the auto-scroll timer is armed.
    pub fn auto_scroll_armed(&self) -> bool {
        self.auto_scroll_due.is_some()
    }

    fn set_focus(&mut self, focused: bool) {
        self.focused = focused;
        // The caret and the empty hint both depend on focus.
        self.blink.restart(focused);
        crate::animation::request_draw();
    }

    fn paint_document(&mut self, ctx: &mut dyn DrawCtx) {
        // A DeviceScale or text-size change since the last layout: every block re-lays out first.
        let _ = self.size_state();
        let _ = self.relayout_arrived_images();
        self.run_due_auto_scroll();
        self.restart_blink_on_placement();
        self.sync_raw_hosts();

        let origin = self.state.view_origin();
        let visible = self.state.visible_bounds();
        let empty_hint = if self.state.is_empty() {
            self.empty_hint.as_deref()
        } else {
            None
        };
        let selection: RichSelection = self.state.selection();
        let images = std::rc::Rc::clone(&self.shared);
        let loaded = move |atom: &super::inline::InlineAtom| images.images.borrow().loaded(atom);
        let paint = RichViewPaint {
            document: &self.state.document,
            colors: &self.colors,
            loaded_image: &loaded,
            selection: &selection,
            caret_at_line_end: self.state.caret_at_line_end(),
            focused: self.focused,
            caret_showing: self.blink.showing(),
            empty_hint,
            visible: Some((visible.y, visible.y + visible.height)),
        };
        ctx.save();
        ctx.clip_rect(0.0, 0.0, self.bounds.width, self.bounds.height);
        ctx.translate(0.0, origin);
        self.state.view.draw(ctx, &paint);
        ctx.restore();
        self.blink.tick(self.focused);
    }
}

impl Widget for RichMarkdownEditWidget {
    crate::widgets::widget_as_any!();

    fn type_name(&self) -> &'static str {
        "RichMarkdownEditWidget"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }
    fn margin(&self) -> Insets {
        self.base.margin
    }
    fn widget_base(&self) -> Option<&WidgetBase> {
        Some(&self.base)
    }
    fn widget_base_mut(&mut self) -> Option<&mut WidgetBase> {
        Some(&mut self.base)
    }
    fn h_anchor(&self) -> HAnchor {
        self.base.h_anchor
    }
    fn v_anchor(&self) -> VAnchor {
        self.base.v_anchor
    }

    fn layout(&mut self, available: Size) -> Size {
        // A scrollable widget keeps no minimum: the editor narrows with its panel.
        self.bounds.width = available.width.max(0.0);
        self.bounds.height = available.height.max(0.0);
        let _ = self.size_state();
        self.sync_raw_hosts();
        Size::new(self.bounds.width, self.bounds.height)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        self.paint_document(ctx);
    }

    fn paint_overlay(&mut self, ctx: &mut dyn DrawCtx) {
        self.sync_scroll_bar();
        let geom = self.scroll_geometry();
        if let Some(bar) = self.vbar.prepare_paint(
            self.bounds.height,
            current_scroll_style(),
            current_scroll_visibility(),
            geom,
        ) {
            paint_prepared_scrollbar(ctx, bar);
        }
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseDown {
                pos,
                button,
                modifiers,
            } => self.mouse_down(*pos, *button, *modifiers),
            Event::MouseMove { pos } => self.mouse_move(*pos),
            Event::MouseUp { .. } => self.mouse_up(),
            Event::MouseWheel { delta_y, .. } => {
                let before = self.state.scroll_offset_from_top();
                self.set_scroll_offset_from_top(before - delta_y);
                if self.state.scroll_offset_from_top() != before {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            Event::KeyDown { key, modifiers } => self.key_down(key, *modifiers),
            Event::FocusGained => {
                self.set_focus(true);
                EventResult::Consumed
            }
            Event::FocusLost => {
                self.set_focus(false);
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    fn is_focusable(&self) -> bool {
        true
    }

    fn focus_id(&self) -> Option<crate::focus::FocusId> {
        Some(self.focus_id)
    }

    fn accepts_text_input(&self) -> bool {
        true
    }

    fn needs_draw(&self) -> bool {
        if !self.is_visible() {
            return false;
        }
        self.shared.images.borrow().loading()
            || self.vbar.animation_active()
            || self.children.iter().any(|c| c.needs_draw())
    }

    fn next_draw_deadline(&self) -> Option<web_time::Instant> {
        if !self.is_visible() {
            return None;
        }
        let blink = if self.focused {
            self.blink
                .until_edge()
                .map(|until| crate::clock::now() + until)
        } else {
            None
        };
        [blink, self.auto_scroll_due].into_iter().flatten().min()
    }
}
