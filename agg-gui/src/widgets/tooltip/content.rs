//! Host-supplied tooltip content — agg-sharp's `ToolTipManager.CreateToolTip`.
//!
//! By default the central [`controller`](super::controller) paints a tip as
//! plain text lines in the lightweight queue ([`super::render`]). A host that
//! wants richer tips (MatterCAD shows tooltip text in a markdown view, so
//! `"Fillet\n\n*At least 1 part must be selected*"` renders emphasis) installs
//! a content factory with [`set_content_factory`]. While one is installed the
//! controller asks it for a widget built from the tip text, lays that widget
//! out at the viewport size, sizes the tip panel from the layout, and the
//! tooltip drain ([`paint_pending`], called from
//! [`paint_global_tooltips`](super::render::paint_global_tooltips)) paints it
//! above every clip, menu and modal layer. Placement, timing, autopop and the
//! hide rules stay the controller's: only what the tip *draws* changes.
//!
//! The factory's widget draws the whole tip — background, border, padding —
//! because C#'s `CreateToolTip` result is added to the window bare. When the
//! hovered widget's tip text changes while the tip shows, the content is
//! rebuilt from the new text (C#'s `changeWidgetText` callback); every new
//! show builds fresh content, as C# creates a new widget per show.
//!
//! This applies to the central controller (`with_tooltip` widgets and popup
//! menu rows) only; the legacy [`Tooltip`](super::Tooltip) wrapper keeps its
//! own line styling. State is thread-local, like the rest of the tooltip
//! subsystem (the GUI is single threaded).

use std::cell::RefCell;
use std::rc::Rc;

use crate::draw_ctx::DrawCtx;
use crate::geometry::{Rect, Size};
use crate::widget::{paint_subtree, Widget};

/// Builds a tooltip's content widget from its text. See [`set_content_factory`].
pub type TooltipContentFactory = dyn Fn(&str) -> Box<dyn Widget>;

/// Content built for the visible tip.
struct Active {
    /// Text the widget was built from; a different text rebuilds it.
    text: String,
    widget: Box<dyn Widget>,
    /// Viewport the widget was last laid out at; a resize lays it out again.
    laid_out_at: Size,
    /// The widget's laid-out size: the tip panel's size.
    size: Size,
}

thread_local! {
    static FACTORY: RefCell<Option<Rc<TooltipContentFactory>>> = const { RefCell::new(None) };
    static ACTIVE: RefCell<Option<Active>> = const { RefCell::new(None) };
    /// Panel rect (root logical coords) the visible content paints at this
    /// frame; set by [`submit`], taken by [`paint_pending`].
    static PENDING: RefCell<Option<Rect>> = const { RefCell::new(None) };
}

/// Install (`Some`) or remove (`None`) the app's tooltip content factory,
/// agg-sharp's `ToolTipManager.CreateToolTip`. With none installed tips paint
/// as plain text lines, exactly as before. The factory gets the tip's full
/// text and returns the widget that draws the whole tip; the tip panel takes
/// the size the widget's `layout` returns when offered the viewport. Swapping
/// factories rebuilds a showing tip on the next frame.
pub fn set_content_factory(factory: Option<Box<TooltipContentFactory>>) {
    FACTORY.with(|f| *f.borrow_mut() = factory.map(Rc::from));
    clear();
}

/// Whether a content factory is installed.
pub fn has_content_factory() -> bool {
    FACTORY.with(|f| f.borrow().is_some())
}

/// Build (or rebuild, on a text change) and lay out the content for `text`
/// at `viewport`, returning the tip panel size. `None` when no factory is
/// installed — the controller then paints its default text lines.
pub(super) fn prepare(text: &str, viewport: Size) -> Option<Size> {
    let factory = FACTORY.with(|f| f.borrow().clone())?;
    let stale = ACTIVE.with(|a| a.borrow().as_ref().map_or(true, |a| a.text != text));
    if stale {
        // Build outside the borrow: the factory may itself touch tooltip state.
        let widget = factory(text);
        ACTIVE.with(|a| {
            *a.borrow_mut() = Some(Active {
                text: text.to_string(),
                widget,
                laid_out_at: Size::new(-1.0, -1.0),
                size: Size::ZERO,
            })
        });
    }
    ACTIVE.with(|a| {
        let mut a = a.borrow_mut();
        let active = a.as_mut()?;
        if active.laid_out_at != viewport {
            let s = active.widget.layout(viewport);
            active
                .widget
                .set_bounds(Rect::new(0.0, 0.0, s.width, s.height));
            active.size = s;
            active.laid_out_at = viewport;
        }
        Some(active.size)
    })
}

/// Queue the prepared content to paint at `rect` (root logical coords) in
/// this frame's tooltip drain.
pub(super) fn submit(rect: Rect) {
    PENDING.with(|p| *p.borrow_mut() = Some(rect));
}

/// Drop the built content and any queued paint (the tip hid).
pub(super) fn clear() {
    ACTIVE.with(|a| *a.borrow_mut() = None);
    PENDING.with(|p| *p.borrow_mut() = None);
}

/// Start-of-frame reset: nothing paints unless the controller submits again.
pub(super) fn begin_frame() {
    PENDING.with(|p| *p.borrow_mut() = None);
}

/// Paint the queued content, if any, translated to its panel rect. The widget
/// is taken out of its cell while it paints so a paint that reaches back into
/// the tooltip subsystem cannot hit a held borrow.
pub(super) fn paint_pending(ctx: &mut dyn DrawCtx) {
    let Some(rect) = PENDING.with(|p| p.borrow_mut().take()) else {
        return;
    };
    let Some(mut active) = ACTIVE.with(|a| a.borrow_mut().take()) else {
        return;
    };
    ctx.save();
    ctx.translate(rect.x, rect.y);
    paint_subtree(active.widget.as_mut(), ctx);
    ctx.restore();
    ACTIVE.with(|a| {
        let mut a = a.borrow_mut();
        if a.is_none() {
            *a = Some(active);
        }
    });
}

/// Text the current content was built from, for tests.
#[cfg(test)]
pub(super) fn active_text() -> Option<String> {
    ACTIVE.with(|a| a.borrow().as_ref().map(|a| a.text.clone()))
}

#[cfg(test)]
#[path = "content_tests.rs"]
mod tests;
