//! Widgets hosted in popup-menu rows.
//!
//! A [`super::MenuItem::widget_row`] entry reserves a row of a given height;
//! the widget that fills it is registered on the owning [`super::PopupMenu`]
//! under the row's id and stored here.  The menu geometry
//! ([`super::geometry::stack_layout`]) places the row like any other; this
//! module lays the widget out into that row, paints it, and routes pointer
//! events to it with the framework's standard hit-test + bubble dispatch,
//! including hover-leave and press capture (a drag that starts in a row
//! widget keeps going to it until release).
//!
//! The store is shared behind `Rc<RefCell<…>>` so `PopupMenu` stays `Clone`
//! (clones share the same widgets).

use std::cell::RefCell;
use std::rc::Rc;

use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Point, Rect, Size};
use crate::widget::{dispatch_event_dyn, hit_test_subtree, paint_subtree, Widget};

use super::geometry::{contains, item_at_path, PopupLayout};
use super::model::MenuEntry;

struct RowWidget {
    id: usize,
    widget: Box<dyn Widget>,
    /// Hit path inside the widget from the last pointer event (for
    /// hover-leave delivery).
    hover_path: Option<Vec<usize>>,
}

/// Row id + hit path inside its widget.
type Capture = (usize, Vec<usize>);

/// Row widgets of one popup menu, keyed by [`super::MenuWidgetRow::id`].
#[derive(Clone, Default)]
pub(super) struct RowWidgets {
    rows: Rc<RefCell<Vec<RowWidget>>>,
    /// Row id + hit path of the widget that took the last press.
    captured: Rc<RefCell<Option<Capture>>>,
}

impl RowWidgets {
    pub(super) fn is_empty(&self) -> bool {
        self.rows.borrow().is_empty()
    }

    pub(super) fn set(&self, id: usize, widget: Box<dyn Widget>) {
        let mut rows = self.rows.borrow_mut();
        match rows.iter_mut().find(|r| r.id == id) {
            Some(row) => {
                row.widget = widget;
                row.hover_path = None;
            }
            None => rows.push(RowWidget {
                id,
                widget,
                hover_path: None,
            }),
        }
    }

    pub(super) fn remove(&self, id: usize) -> Option<Box<dyn Widget>> {
        let mut rows = self.rows.borrow_mut();
        let idx = rows.iter().position(|r| r.id == id)?;
        Some(rows.remove(idx).widget)
    }

    pub(super) fn next_free_id(&self) -> usize {
        self.rows
            .borrow()
            .iter()
            .map(|r| r.id + 1)
            .max()
            .unwrap_or(0)
    }

    /// Run `f` on the widget registered under `id`.
    pub(super) fn with<R>(&self, id: usize, f: impl FnOnce(&mut dyn Widget) -> R) -> Option<R> {
        let mut rows = self.rows.borrow_mut();
        let row = rows.iter_mut().find(|r| r.id == id)?;
        Some(f(row.widget.as_mut()))
    }

    /// Forget hover / capture (the menu closed or reopened).
    pub(super) fn reset_interaction(&self) {
        *self.captured.borrow_mut() = None;
        for row in self.rows.borrow_mut().iter_mut() {
            row.hover_path = None;
        }
    }

    /// Lay out and paint every visible row widget of `layout`.
    pub(super) fn paint_level(
        &self,
        ctx: &mut dyn DrawCtx,
        items: &[MenuEntry],
        layout: &PopupLayout,
    ) {
        let mut rows = self.rows.borrow_mut();
        for (id, rect) in widget_rows(items, std::slice::from_ref(layout)) {
            if let Some(row) = rows.iter_mut().find(|r| r.id == id) {
                place(row.widget.as_mut(), rect);
                // `paint_subtree` paints in the widget's local space; the
                // parent's translate-by-bounds is ours to do.
                let b = row.widget.bounds();
                ctx.save();
                ctx.translate(b.x, b.y);
                paint_subtree(row.widget.as_mut(), ctx);
                ctx.restore();
            }
        }
    }

    /// Route a pointer event to the row widgets.  Returns `Some` when the
    /// event belongs to a row widget and the menu must not handle it;
    /// `None` lets the menu handle it as usual (a `MouseMove` is delivered
    /// to the widgets *and* returned as `None` so the menu still updates its
    /// own hover / submenu state).
    pub(super) fn route(
        &self,
        items: &[MenuEntry],
        layouts: &[PopupLayout],
        event: &Event,
    ) -> Option<EventResult> {
        let pos = pointer_pos(event)?;
        let visible = widget_rows(items, layouts);
        let mut rows = self.rows.borrow_mut();

        // A press that started in a row widget owns the pointer until release.
        let captured = self.captured.borrow().clone();
        if let Some((id, path)) = captured {
            if let Some((row, rect)) = find(&mut rows, &visible, id) {
                place(row.widget.as_mut(), rect);
                let result = deliver(row.widget.as_mut(), &path, event, pos);
                if matches!(event, Event::MouseUp { .. }) {
                    *self.captured.borrow_mut() = None;
                    let p = local(row.widget.as_ref(), pos);
                    row.hover_path = hit_test_subtree(row.widget.as_ref(), p);
                }
                return Some(result.max_consumed());
            }
            *self.captured.borrow_mut() = None;
        }

        let under = visible
            .iter()
            .find(|(_, rect)| contains(*rect, pos))
            .copied();

        // Hover-leave: the widget the pointer was last over gets a move at
        // the new (outside) position so it can drop its hover state.
        for row in rows.iter_mut() {
            if under.is_some_and(|(id, _)| id == row.id) {
                continue;
            }
            if let Some(path) = row.hover_path.take() {
                if visible.iter().any(|(id, _)| *id == row.id) {
                    let leave = Event::MouseMove { pos };
                    deliver(row.widget.as_mut(), &path, &leave, pos);
                }
            }
        }

        let (id, rect) = under?;
        let row = rows.iter_mut().find(|r| r.id == id)?;
        place(row.widget.as_mut(), rect);
        let p = local(row.widget.as_ref(), pos);
        let path = hit_test_subtree(row.widget.as_ref(), p).unwrap_or_default();
        let result = deliver(row.widget.as_mut(), &path, event, pos);
        row.hover_path = Some(path.clone());
        match event {
            Event::MouseMove { .. } => None,
            Event::MouseDown { .. } => {
                *self.captured.borrow_mut() = Some((id, path));
                Some(result.max_consumed())
            }
            _ => Some(result.max_consumed()),
        }
    }
}

trait MaxConsumed {
    fn max_consumed(self) -> EventResult;
}

impl MaxConsumed for EventResult {
    /// Inside the menu every pointer event is consumed (the menu is modal);
    /// keep a widget's own consuming result so its redraw request survives.
    fn max_consumed(self) -> EventResult {
        if self.is_consumed() {
            self
        } else {
            EventResult::Consumed
        }
    }
}

/// `(row id, row rect)` for every widget row visible in `layouts`.
fn widget_rows(items: &[MenuEntry], layouts: &[PopupLayout]) -> Vec<(usize, Rect)> {
    let mut out = Vec::new();
    for layout in layouts {
        for row in &layout.rows {
            let Some(idx) = row.item_index else {
                continue;
            };
            let mut path = layout.path_prefix.clone();
            path.push(idx);
            if let Some(w) = item_at_path(items, &path).and_then(|item| item.widget_row) {
                out.push((w.id, row.rect));
            }
        }
    }
    out
}

fn find<'a>(
    rows: &'a mut [RowWidget],
    visible: &[(usize, Rect)],
    id: usize,
) -> Option<(&'a mut RowWidget, Rect)> {
    let rect = visible.iter().find(|(v, _)| *v == id)?.1;
    let row = rows.iter_mut().find(|r| r.id == id)?;
    Some((row, rect))
}

/// Lay `widget` out with the row's size and place it at the row's left edge,
/// vertically centred (a stretching widget fills the row).
fn place(widget: &mut dyn Widget, rect: Rect) {
    let size = widget.layout(Size::new(rect.width, rect.height));
    let w = size.width.min(rect.width);
    let h = size.height.min(rect.height);
    widget.set_bounds(Rect::new(rect.x, rect.y + (rect.height - h) * 0.5, w, h));
}

/// `pos` (menu space) in the local coordinates of a placed row widget.
fn local(widget: &dyn Widget, pos: Point) -> Point {
    let b = widget.bounds();
    Point::new(pos.x - b.x, pos.y - b.y)
}

/// Dispatch `event` (menu-space `pos`) along `path` inside the row widget.
fn deliver(widget: &mut dyn Widget, path: &[usize], event: &Event, pos: Point) -> EventResult {
    let p = local(widget, pos);
    dispatch_event_dyn(widget, path, &with_pos(event, p), p)
}

fn pointer_pos(event: &Event) -> Option<Point> {
    match event {
        Event::MouseMove { pos }
        | Event::MouseDown { pos, .. }
        | Event::MouseUp { pos, .. }
        | Event::MouseWheel { pos, .. } => Some(*pos),
        _ => None,
    }
}

fn with_pos(event: &Event, pos: Point) -> Event {
    match event {
        Event::MouseMove { .. } => Event::MouseMove { pos },
        Event::MouseDown {
            button, modifiers, ..
        } => Event::MouseDown {
            pos,
            button: *button,
            modifiers: *modifiers,
        },
        Event::MouseUp {
            button, modifiers, ..
        } => Event::MouseUp {
            pos,
            button: *button,
            modifiers: *modifiers,
        },
        Event::MouseWheel {
            delta_y,
            delta_x,
            modifiers,
            ..
        } => Event::MouseWheel {
            pos,
            delta_y: *delta_y,
            delta_x: *delta_x,
            modifiers: *modifiers,
        },
        other => other.clone(),
    }
}
