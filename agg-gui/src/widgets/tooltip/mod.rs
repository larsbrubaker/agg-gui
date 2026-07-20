//! `Tooltip` — a wrapper widget that shows egui-style hover help.
//!
//! Tooltips come in two flavours:
//!
//! * **Lightweight** (the default, used by dozens of call sites): text lines
//!   submitted during the widget paint pass and drawn at the end of the frame
//!   by [`crate::widget::App`] via a global queue ([`render`]). Fire-and-forget,
//!   no hit-testing.
//!
//! * **Interactive** (opt-in via [`Tooltip::with_interactive_content`]): the tip
//!   is a real floating UI surface hosting a child widget tree (labels,
//!   hyperlinks, a nested tooltip). It participates in the global-overlay
//!   hit-test so the pointer can move *into* it without dismissing it, and it
//!   supports one level of nesting — see [`interactive`]. This mirrors egui's
//!   `on_hover_ui` tooltips.
//!
//! # Usage
//!
//! ```ignore
//! Tooltip::new(
//!     Box::new(Button::new("Hover me", font.clone()).on_click(|| {})),
//!     "This is a tooltip",
//!     font.clone(),
//! )
//! ```

pub mod controller;
mod interactive;
mod render;
mod timings;

pub(crate) use render::{begin_tooltip_frame, paint_global_tooltips};
pub use timings::{set_tooltip_timings, tooltip_timings, TooltipTimings};
#[doc(hidden)]
pub use timings::{
    advance_tooltip_test_clock, reset_tooltip_test_state, set_tooltip_test_clock,
};

use timings::{last_tooltip_visible_at, note_tooltip_visible, tooltip_now};

use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;
use web_time::Instant;

use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Point, Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::text::Font;
use crate::widget::{current_mouse_world, Widget};

use render::{submit_tooltip, TooltipRequest};

/// Compile-time default initial hover delay, kept as a stable reference for
/// tests and for interactive-tip code paths. Runtime code reads the (possibly
/// OS-overridden) value via [`tooltip_timings`]; this equals the default there.
const TOOLTIP_INITIAL_DELAY: Duration = timings::DEFAULT_INITIAL_DELAY;
pub(super) const TOOLTIP_FONT_SIZE: f64 = 12.0;
pub(super) const TOOLTIP_PAD_X: f64 = 8.0;
pub(super) const TOOLTIP_PAD_Y: f64 = 6.0;
pub(super) const TOOLTIP_GAP: f64 = 4.0;
/// Extra vertical offset for pointer-anchored tooltips.  They should
/// read as attached below the cursor rather than hugging it.
pub(super) const POINTER_TOOLTIP_EXTRA_DROP: f64 = 10.0;
pub(super) const SCREEN_MARGIN: f64 = 4.0;

#[derive(Clone)]
pub(super) enum TooltipLineKind {
    Text,
    Code,
    Link,
}

#[derive(Clone)]
pub(super) struct TooltipLine {
    pub text: String,
    pub kind: TooltipLineKind,
}

/// A wrapper widget that shows a text tooltip on hover.
pub struct Tooltip {
    bounds: Rect,
    /// The wrapped child widget is stored in `children[0]`.
    children: Vec<Box<dyn Widget>>,
    base: WidgetBase,

    /// Time when the pointer entered the widget.  `None` when the
    /// pointer is outside. Wall-clock timing gives consistent tooltip
    /// latency even when the app is not repainting continuously.
    hover_started_at: Option<Instant>,
    /// Whether the cursor is currently inside the widget bounds.
    hovered: bool,
    /// Whether this tooltip was visible on the previous paint. Used
    /// to invalidate when the delayed tooltip appears or disappears,
    /// not just when hover state changes.
    tooltip_visible: bool,
    /// Delay to apply for the current hover, captured when the pointer entered:
    /// the full initial delay, or the shorter reshow delay if another tip was
    /// visible within the reshow window (move-along-the-toolbar).
    pending_delay: Duration,
    /// When the current tip became visible, used to auto-dismiss after
    /// [`TooltipTimings::autopop`].
    tip_shown_at: Option<Instant>,
    /// Suppresses (re)showing until the pointer leaves and re-enters. Set on any
    /// mouse press over the child and on autopop, mirroring Windows: a click or a
    /// timed-out tip stays hidden until you move off and back onto the control.
    suppressed: bool,
    /// Whether a mouse button is currently held; tips never show while pressed.
    pointer_down: bool,
    /// Last known cursor position in local coordinates.
    cursor: Point,

    font: Arc<Font>,
    lines: Vec<TooltipLine>,
    disabled_lines: Vec<TooltipLine>,
    disabled_when: Option<Rc<dyn Fn() -> bool>>,
    at_pointer: bool,

    // --- Interactive-mode state (see `interactive`) ---------------------
    /// When `true`, the tip is a hit-testable surface hosting `content`
    /// instead of the lightweight text queue.
    interactive: bool,
    /// The interactive tip's child widget tree. Not part of `children`, so
    /// it is not laid out / painted / hit-tested by the normal tree walk —
    /// [`interactive`] manages it manually at the floating tip position.
    content: Option<Box<dyn Widget>>,
    /// Natural size of `content` from its last layout.
    content_size: Size,
    /// Latched open-state for the interactive tip. Stays open while the
    /// pointer is over the anchor OR the tip; closes after a grace period
    /// once it leaves both (or on Escape).
    tip_open: bool,
    /// Whether the pointer is currently over the interactive tip surface.
    tip_hovered: bool,
    /// Panel rectangle in this widget's LOCAL coordinate space, captured
    /// during the last overlay paint and reused for hit-testing.
    tip_panel_local: Option<Rect>,
    /// Where `content` is painted within the panel (local coords).
    content_origin_local: Point,
    /// When the pointer left both anchor and tip; the tip closes once this
    /// exceeds [`interactive::TOOLTIP_CLOSE_GRACE`].
    close_requested_at: Option<Instant>,
    /// Path of the content descendant the pointer last hovered, so we can
    /// clear its hover (and any nested tooltip) when the pointer moves off.
    last_content_path: Option<Vec<usize>>,
}

impl Tooltip {
    /// Create a new `Tooltip` wrapping `child` with `text` as the tip message.
    pub fn new(child: Box<dyn Widget>, text: impl Into<String>, font: Arc<Font>) -> Self {
        Self {
            bounds: Rect::default(),
            children: vec![child],
            base: WidgetBase::new(),
            hover_started_at: None,
            hovered: false,
            tooltip_visible: false,
            pending_delay: TOOLTIP_INITIAL_DELAY,
            tip_shown_at: None,
            suppressed: false,
            pointer_down: false,
            cursor: Point::ORIGIN,
            font,
            lines: text_to_lines(text),
            disabled_lines: Vec::new(),
            disabled_when: None,
            at_pointer: true,
            interactive: false,
            content: None,
            content_size: Size::new(0.0, 0.0),
            tip_open: false,
            tip_hovered: false,
            tip_panel_local: None,
            content_origin_local: Point::ORIGIN,
            close_requested_at: None,
            last_content_path: None,
        }
    }

    /// Add another hover text block, matching egui's ability to chain
    /// `.on_hover_text(...)` calls.
    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        self.lines.extend(text_to_lines(text));
        self
    }

    /// Add a code-styled line to the tooltip.
    pub fn with_code_line(mut self, text: impl Into<String>) -> Self {
        self.lines.push(TooltipLine {
            text: text.into(),
            kind: TooltipLineKind::Code,
        });
        self
    }

    /// Add a link-styled line to the tooltip.  Tooltip overlays are
    /// informational; the line is styled like a link but does not receive
    /// pointer events.
    pub fn with_link_line(mut self, text: impl Into<String>) -> Self {
        self.lines.push(TooltipLine {
            text: text.into(),
            kind: TooltipLineKind::Link,
        });
        self
    }

    /// Turn this tooltip into an **interactive** surface hosting `content`
    /// (a small widget tree of labels / hyperlinks). The lightweight text
    /// lines are ignored while interactive. The pointer can enter the tip
    /// without dismissing it, and a tooltip-bearing widget inside `content`
    /// shows its own (nested) tip. Mirrors egui's `on_hover_ui`.
    pub fn with_interactive_content(mut self, content: Box<dyn Widget>) -> Self {
        self.interactive = true;
        self.content = Some(content);
        self.at_pointer = false;
        self
    }

    /// Place the tooltip relative to the mouse cursor instead of the widget.
    /// This is the default; kept for call-site clarity.
    pub fn at_pointer(mut self) -> Self {
        self.at_pointer = true;
        self
    }

    /// Place the tooltip relative to the wrapped widget instead of the
    /// mouse cursor.
    pub fn at_widget(mut self) -> Self {
        self.at_pointer = false;
        self
    }

    /// Use alternate tooltip text while `disabled_when` returns true.
    pub fn with_disabled_text(
        mut self,
        text: impl Into<String>,
        disabled_when: impl Fn() -> bool + 'static,
    ) -> Self {
        self.disabled_lines = text_to_lines(text);
        self.disabled_when = Some(Rc::new(disabled_when));
        self
    }

    pub fn with_margin(mut self, m: Insets) -> Self {
        self.base.margin = m;
        self
    }
    pub fn with_h_anchor(mut self, h: HAnchor) -> Self {
        self.base.h_anchor = h;
        self
    }
    pub fn with_v_anchor(mut self, v: VAnchor) -> Self {
        self.base.v_anchor = v;
        self
    }

    /// Delay to apply for a hover starting now: the shorter reshow delay when a
    /// tip was visible within the reshow window, otherwise the full initial
    /// delay. Captured at pointer-enter into [`Self::pending_delay`].
    fn effective_delay(&self) -> Duration {
        let t = tooltip_timings();
        match last_tooltip_visible_at() {
            Some(at) if tooltip_now().saturating_duration_since(at) <= t.reshow_window() => {
                t.reshow_delay
            }
            _ => t.initial_delay,
        }
    }

    /// Whether the tip should be visible right now, ignoring autopop (handled
    /// separately in [`Self::update_visibility`]).
    fn should_show_tip(&self) -> bool {
        if self.suppressed || self.pointer_down || !self.hovered {
            return false;
        }
        match self.hover_started_at {
            Some(started) => {
                tooltip_now().saturating_duration_since(started) >= self.pending_delay
            }
            None => false,
        }
    }

    /// Advance the lightweight tip's visibility state machine. Applies autopop,
    /// flips [`Self::tooltip_visible`], warms the shared reshow window, and
    /// schedules the wall-clock wake that drives the delayed show / auto-dismiss
    /// without continuous repainting. Returns whether the tip is now visible.
    ///
    /// Split out from `paint_overlay` so the state machine is unit-testable
    /// without a `DrawCtx`.
    fn update_visibility(&mut self) -> bool {
        // Auto-dismiss a tip that has sat open too long, and keep it hidden
        // until the pointer leaves and re-enters (Windows AUTOPOP behaviour).
        if self.tooltip_visible {
            if let Some(shown) = self.tip_shown_at {
                let autopop = tooltip_timings().autopop;
                let elapsed = tooltip_now().saturating_duration_since(shown);
                if elapsed >= autopop {
                    self.hide_tip();
                    self.suppressed = true;
                    return false;
                }
                // Re-arm the autopop wake every visible frame: the deadline is
                // read-and-cleared by the host each frame, so an intervening
                // repaint would otherwise drop it and the dismiss never fires.
                crate::animation::request_draw_after(autopop - elapsed);
            }
        }

        if self.should_show_tip() {
            if !self.tooltip_visible {
                self.tooltip_visible = true;
                self.tip_shown_at = Some(tooltip_now());
                crate::animation::request_draw();
                // Wake at the autopop deadline so a still pointer still dismisses.
                crate::animation::request_draw_after(tooltip_timings().autopop);
            }
            note_tooltip_visible();
            return true;
        }

        if self.tooltip_visible {
            self.hide_tip();
        } else if let Some(remaining) = self.remaining_delay() {
            // Not yet time to show: schedule the wake at the show deadline.
            if remaining.is_zero() {
                crate::animation::request_draw();
            } else {
                crate::animation::request_draw_after(remaining);
            }
        }
        false
    }

    /// Time left before the pending show fires, or `None` when no show is
    /// pending (not hovered, suppressed, or a button is held).
    fn remaining_delay(&self) -> Option<Duration> {
        if !self.hovered || self.suppressed || self.pointer_down {
            return None;
        }
        let elapsed = tooltip_now().saturating_duration_since(self.hover_started_at?);
        Some(self.pending_delay.saturating_sub(elapsed))
    }

    /// Hide the tip and clear its display timer, invalidating so the overlay
    /// clears on the next paint.
    fn hide_tip(&mut self) {
        if self.tooltip_visible {
            crate::animation::request_draw();
        }
        self.tooltip_visible = false;
        self.tip_shown_at = None;
    }

    fn active_lines(&self) -> Vec<TooltipLine> {
        if self.disabled_when.as_ref().map(|f| f()).unwrap_or(false)
            && !self.disabled_lines.is_empty()
        {
            self.disabled_lines.clone()
        } else {
            self.lines.clone()
        }
    }
}

impl Widget for Tooltip {
    fn type_name(&self) -> &'static str {
        "Tooltip"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
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

    /// Forward the wrapped child's size constraints so the tooltip is a
    /// transparent layout wrapper. Without this a constrained child — e.g. a
    /// [`ComboBox`](crate::widgets::combo_box::ComboBox) whose `max_size` caps its
    /// width — would lose that cap when wrapped and placed in a `FlexRow`, because
    /// the row reads the *wrapper's* constraints, not the child's, and would let
    /// the combo stretch across the whole row.
    fn min_size(&self) -> Size {
        self.children
            .first()
            .map(|c| c.min_size())
            .unwrap_or(Size::ZERO)
    }
    fn max_size(&self) -> Size {
        self.children
            .first()
            .map(|c| c.max_size())
            .unwrap_or(Size::MAX)
    }

    /// Expose the tip text to the inspector and to tests, so a tooltip's presence
    /// and message can be asserted by walking the widget tree (there is no public
    /// downcast for `dyn Widget`).
    fn properties(&self) -> Vec<(&'static str, String)> {
        vec![(
            "tooltip",
            self.lines
                .iter()
                .map(|l| l.text.as_str())
                .collect::<Vec<_>>()
                .join("\n"),
        )]
    }

    fn is_focusable(&self) -> bool {
        self.children
            .first()
            .map(|c| c.is_focusable())
            .unwrap_or(false)
    }

    fn layout(&mut self, available: Size) -> Size {
        let s = if let Some(child) = self.children.first_mut() {
            let cs = child.layout(available);
            child.set_bounds(Rect::new(0.0, 0.0, cs.width, cs.height));
            cs
        } else {
            available
        };
        self.bounds = Rect::new(0.0, 0.0, s.width, s.height);
        s
    }

    fn paint(&mut self, _: &mut dyn DrawCtx) {}

    fn paint_overlay(&mut self, ctx: &mut dyn DrawCtx) {
        // Interactive tips paint their surface in `paint_global_overlay`,
        // not through the lightweight text queue.
        if self.interactive {
            return;
        }

        // Advance the delay/reshow/autopop state machine. It handles its own
        // invalidation and schedules the wall-clock wake for the delayed show
        // or auto-dismiss, so no busy-repaint loop is needed.
        if !self.update_visibility() {
            return;
        }

        let anchor = if self.at_pointer {
            current_mouse_world().unwrap_or(self.cursor)
        } else {
            let mut x = self.bounds.width * 0.5;
            // Widget-anchored tooltips should appear below the
            // hovered widget by default (MatterCAD-style). In
            // agg-gui's Y-up coords, the bottom edge is y=0; the
            // global paint step will offset the panel by
            // `TOOLTIP_GAP` from this anchor.
            let mut y = 0.0;
            ctx.root_transform().transform(&mut x, &mut y);
            Point::new(x, y)
        };
        submit_tooltip(TooltipRequest {
            font: Arc::clone(&self.font),
            lines: self.active_lines(),
            anchor,
            at_pointer: self.at_pointer,
        });
    }

    fn paint_global_overlay(&mut self, ctx: &mut dyn DrawCtx) {
        if self.interactive {
            self.paint_interactive_tip(ctx);
        }
    }

    fn hit_test_global_overlay(&self, local_pos: Point) -> bool {
        self.interactive && self.interactive_hit(local_pos)
    }

    fn on_unconsumed_key(
        &mut self,
        key: &crate::event::Key,
        _modifiers: crate::event::Modifiers,
    ) -> EventResult {
        if self.interactive {
            self.interactive_unconsumed_key(key)
        } else {
            EventResult::Ignored
        }
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        if self.interactive {
            return self.on_interactive_event(event);
        }
        match event {
            Event::MouseMove { pos } => {
                let was = self.hovered;
                self.hovered = self.hit_test(*pos);
                self.cursor = *pos;
                if self.hovered && !was {
                    // Entering: start the hover timer, picking the reshow delay
                    // when the subsystem is still warm from a recent tip.
                    self.hover_started_at = Some(tooltip_now());
                    self.pending_delay = self.effective_delay();
                    self.suppressed = false;
                    crate::animation::request_draw_after(self.pending_delay);
                } else if !self.hovered && was {
                    // Leaving: reset so the next entry re-arms cleanly and any
                    // press/autopop suppression clears.
                    self.hover_started_at = None;
                    self.suppressed = false;
                    self.hide_tip();
                }
                if self.hovered != was {
                    crate::animation::request_draw();
                }
                self.children
                    .first_mut()
                    .map(|child| child.on_event(event))
                    .unwrap_or(EventResult::Ignored)
            }
            Event::MouseDown { .. } => {
                // A press over the control hides the tip and keeps it hidden
                // until the pointer leaves and re-enters.
                self.pointer_down = true;
                self.suppressed = true;
                self.hide_tip();
                self.children
                    .first_mut()
                    .map(|child| child.on_event(event))
                    .unwrap_or(EventResult::Ignored)
            }
            Event::MouseUp { .. } => {
                self.pointer_down = false;
                self.children
                    .first_mut()
                    .map(|child| child.on_event(event))
                    .unwrap_or(EventResult::Ignored)
            }
            Event::MouseWheel { .. } => {
                self.hovered = false;
                self.hover_started_at = None;
                self.suppressed = false;
                self.hide_tip();
                self.children
                    .first_mut()
                    .map(|child| child.on_event(event))
                    .unwrap_or(EventResult::Ignored)
            }
            _ => self
                .children
                .first_mut()
                .map(|child| child.on_event(event))
                .unwrap_or(EventResult::Ignored),
        }
    }

    fn hit_test(&self, local_pos: Point) -> bool {
        local_pos.x >= 0.0
            && local_pos.x <= self.bounds.width
            && local_pos.y >= 0.0
            && local_pos.y <= self.bounds.height
    }
}

fn text_to_lines(text: impl Into<String>) -> Vec<TooltipLine> {
    text.into()
        .lines()
        .map(|line| TooltipLine {
            text: line.to_owned(),
            kind: TooltipLineKind::Text,
        })
        .collect()
}

#[cfg(test)]
mod tests;
