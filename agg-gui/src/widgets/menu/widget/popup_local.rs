//! `PopupMenu` hosted by an ordinary widget: widget-local anchoring and
//! widget rows.
//!
//! Menu geometry is clamped against the viewport, so [`PopupMenu`] works in
//! root (screen) coordinates.  A widget that owns a menu (a dropdown button,
//! a split button) only knows its own local space; these helpers keep the
//! local → root offset (the host's *root origin*) on the menu so the host
//! never converts by hand:
//!
//! 1. call [`PopupMenu::sync_root_origin`] from the host's `paint` (or
//!    [`PopupMenu::set_root_origin`] when the host knows it),
//! 2. open with [`PopupMenu::open_at_local`],
//! 3. forward events with [`PopupMenu::handle_local_event`] and paint with
//!    [`PopupMenu::paint_local`] from `paint_global_overlay`.
//!
//! A menu opened at a local anchor follows its host: when the root origin
//! changes the anchor is re-applied.
//!
//! Widget rows ([`super::super::MenuItem::widget_row`]) are registered here
//! too; the routing / painting lives in `menu/row_widgets.rs`.

use std::sync::Arc;

use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Point, Size};
use crate::text::Font;
use crate::widget::Widget;

use super::super::model::{MenuEntry, MenuItem};
use super::super::state::MenuResponse;
use super::PopupMenu;

impl PopupMenu {
    /// Register (or replace) the widget shown in the
    /// [`MenuItem::widget_row`] row with this `id`.
    pub fn set_row_widget(&mut self, id: usize, widget: Box<dyn Widget>) {
        self.row_widgets.set(id, widget);
    }

    /// Unregister the widget for row `id`, returning it.
    pub fn remove_row_widget(&mut self, id: usize) -> Option<Box<dyn Widget>> {
        self.row_widgets.remove(id)
    }

    /// Append a top-level row of `height` hosting `widget`; returns its id.
    pub fn push_widget_row(&mut self, widget: Box<dyn Widget>, height: f64) -> usize {
        let id = self.row_widgets.next_free_id();
        self.items
            .push(MenuEntry::Item(MenuItem::widget_row(id, height)));
        self.row_widgets.set(id, widget);
        id
    }

    /// Builder form of [`PopupMenu::push_widget_row`].
    pub fn with_widget_row(mut self, widget: Box<dyn Widget>, height: f64) -> Self {
        self.push_widget_row(widget, height);
        self
    }

    /// Run `f` on the widget registered for row `id` (inspection, tests,
    /// or updating it while the menu is open).
    pub fn with_row_widget<R>(&self, id: usize, f: impl FnOnce(&mut dyn Widget) -> R) -> Option<R> {
        self.row_widgets.with(id, f)
    }

    /// The host widget's (0, 0) in root coordinates, as last synced.
    pub fn root_origin(&self) -> Point {
        self.root_origin
    }

    /// Set the host widget's (0, 0) in root coordinates.  A menu opened with
    /// [`PopupMenu::open_at_local`] moves with it.
    pub fn set_root_origin(&mut self, origin: Point) {
        if origin == self.root_origin {
            return;
        }
        self.root_origin = origin;
        if let Some(anchor) = self.local_anchor {
            self.state.anchor = Point::new(anchor.x + origin.x, anchor.y + origin.y);
        }
    }

    /// Record the host's root origin from its paint context: call from the
    /// host's `paint` with the ctx positioned at the host's (0, 0).
    pub fn sync_root_origin(&mut self, ctx: &dyn DrawCtx) {
        let (mut x, mut y) = (0.0, 0.0);
        ctx.root_transform().transform(&mut x, &mut y);
        // `root_transform` carries the device scale; menu geometry is logical.
        let scale = crate::device_scale::device_scale().max(1e-6);
        self.set_root_origin(Point::new(x / scale, y / scale));
    }

    /// Open the menu with its top-left corner at `anchor`, given in the host
    /// widget's local coordinates (the menu hangs down from it, clamped to
    /// the viewport like [`PopupMenu::open_at`]).
    pub fn open_at_local(&mut self, anchor: Point) {
        let o = self.root_origin;
        self.open_at(Point::new(anchor.x + o.x, anchor.y + o.y));
        self.local_anchor = Some(anchor);
    }

    /// The local anchor of a menu opened with [`PopupMenu::open_at_local`].
    pub fn local_anchor(&self) -> Option<Point> {
        self.local_anchor
    }

    /// [`PopupMenu::handle_event`] for an event in the host's local
    /// coordinates.
    pub fn handle_local_event(
        &mut self,
        event: &Event,
        viewport: Size,
    ) -> (EventResult, MenuResponse) {
        let event = shift_event(event, self.root_origin);
        self.handle_event(&event, viewport)
    }

    /// [`PopupMenu::body_contains`] for a point in the host's local
    /// coordinates.
    pub fn body_contains_local(&self, pos: Point, viewport: Size) -> bool {
        let o = self.root_origin;
        self.body_contains(Point::new(pos.x + o.x, pos.y + o.y), viewport)
    }

    /// [`PopupMenu::paint`] from a ctx positioned at the host's (0, 0)
    /// (e.g. the host's `paint_global_overlay`).
    pub fn paint_local(
        &mut self,
        ctx: &mut dyn DrawCtx,
        font: Arc<Font>,
        font_size: f64,
        viewport: Size,
    ) {
        let o = self.root_origin;
        ctx.save();
        ctx.translate(-o.x, -o.y);
        self.paint(ctx, font, font_size, viewport);
        ctx.restore();
    }
}

/// `event` with its pointer position moved by `o`.
fn shift_event(event: &Event, o: Point) -> Event {
    let shift = |p: &Point| Point::new(p.x + o.x, p.y + o.y);
    match event {
        Event::MouseMove { pos } => Event::MouseMove { pos: shift(pos) },
        Event::MouseDown {
            pos,
            button,
            modifiers,
        } => Event::MouseDown {
            pos: shift(pos),
            button: *button,
            modifiers: *modifiers,
        },
        Event::MouseUp {
            pos,
            button,
            modifiers,
        } => Event::MouseUp {
            pos: shift(pos),
            button: *button,
            modifiers: *modifiers,
        },
        Event::MouseWheel {
            pos,
            delta_y,
            delta_x,
            modifiers,
        } => Event::MouseWheel {
            pos: shift(pos),
            delta_y: *delta_y,
            delta_x: *delta_x,
            modifiers: *modifiers,
        },
        other => other.clone(),
    }
}
