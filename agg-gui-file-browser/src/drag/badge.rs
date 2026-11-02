//! How a drop target shows itself: a folder row, tile or tree row records itself as a target as
//! it paints ([`register_painted`], its window rectangle from the paint's transform), and while it
//! is the highlighted one fills with the accent at alpha 40 (the name stays readable) and, when
//! the drop would copy, carries a "+" badge at its right end ([`paint_plus_badge`]: a disc in the
//! accent with a white plus), so a copy reads differently from a move before the button is let
//! go.

use std::cell::RefCell;
use std::rc::Rc;

use agg_gui::{Color, DrawCtx, Rect};
use agg_gui_files::FolderLink;

use super::DropKey;
use crate::state::BrowserState;

/// The highlight's opacity over a target.
const HIGHLIGHT_ALPHA: f32 = 40.0 / 255.0;

/// The window rectangle (logical units, Y up) of a `width` by `height` box at the paint's origin.
pub fn painted_rect(ctx: &dyn DrawCtx, width: f64, height: f64) -> Rect {
    let (mut x, mut y) = (0.0, 0.0);
    agg_gui::logical_root_transform(ctx).transform(&mut x, &mut y);
    Rect::new(x, y, width, height)
}

/// Records `link` as the drop target `key` at the box `width` by `height` being painted, then,
/// when it is the highlighted target, fills `highlight` (target-local) with the accent and puts
/// the copy badge on it. Answers whether it is highlighted.
#[allow(clippy::too_many_arguments)]
pub fn register_painted(
    ctx: &mut dyn DrawCtx,
    state: &Rc<RefCell<BrowserState>>,
    link: Rc<dyn FolderLink>,
    key: DropKey,
    size: (f64, f64),
    highlight: Rect,
    corner_radius: f64,
    accent: Color,
) -> bool {
    let rect = painted_rect(ctx, size.0, size.1);
    let (highlighted, copies) = {
        let mut s = state.borrow_mut();
        s.drop.register_target(rect, link, key);
        (
            s.drop.highlighted() == Some(key),
            s.drop.copy_target() == Some(key),
        )
    };
    if !highlighted {
        return false;
    }
    ctx.set_fill_color(accent.with_alpha(HIGHLIGHT_ALPHA));
    ctx.begin_path();
    ctx.rounded_rect(
        highlight.x,
        highlight.y,
        highlight.width,
        highlight.height,
        corner_radius,
    );
    ctx.fill();
    if copies {
        let radius = (highlight.height * 0.3).clamp(5.0, 9.0);
        let cx = highlight.x + highlight.width - radius - 4.0;
        paint_plus_badge(
            ctx,
            cx,
            highlight.y + highlight.height / 2.0,
            radius,
            accent,
        );
    }
    true
}

/// The "+" badge: a disc of `radius` centred on `(cx, cy)` in `accent`, with a white plus.
pub fn paint_plus_badge(ctx: &mut dyn DrawCtx, cx: f64, cy: f64, radius: f64, accent: Color) {
    ctx.set_fill_color(Color::rgba(accent.r, accent.g, accent.b, 1.0));
    ctx.begin_path();
    ctx.circle(cx, cy, radius);
    ctx.fill();
    let (arm, bar) = (radius * 0.6, (radius * 0.25).max(1.5));
    ctx.set_fill_color(Color::rgba(1.0, 1.0, 1.0, 1.0));
    ctx.begin_path();
    ctx.rect(cx - arm, cy - bar / 2.0, arm * 2.0, bar);
    ctx.rect(cx - bar / 2.0, cy - arm, bar, arm * 2.0);
    ctx.fill();
}

/// A listed row's or tile's part as a drop target: a folder row records itself (key
/// `Content`/`index`) over its whole `size` and lights `highlight` while it is the target.
pub(crate) fn paint_entry_target(
    ctx: &mut dyn DrawCtx,
    state: &Rc<RefCell<BrowserState>>,
    entry: &crate::state::BrowserEntry,
    index: usize,
    size: (f64, f64),
    highlight: Rect,
    style: &crate::look::BrowserStyle,
) {
    if let crate::state::BrowserEntry::Container(link) = entry {
        let key = DropKey {
            kind: super::DropTargetKind::Content,
            index,
        };
        let link = Rc::clone(link);
        let (radius, accent) = (style.corner_radius, style.accent);
        register_painted(ctx, state, link, key, size, highlight, radius, accent);
    }
}

/// A listed row's or tile's drag properties: `drop_label` (what a drop on it does while it is
/// the target) and `lifted` (whether the drag carries it).
pub(crate) fn entry_drop_properties(
    state: &Rc<RefCell<BrowserState>>,
    entry: &crate::state::BrowserEntry,
    index: usize,
) -> [(&'static str, String); 2] {
    let s = state.borrow();
    let key = DropKey {
        kind: super::DropTargetKind::Content,
        index,
    };
    [
        ("drop_label", s.drop.label_for(key)),
        ("lifted", s.drop.carries(&entry.item()).to_string()),
    ]
}
