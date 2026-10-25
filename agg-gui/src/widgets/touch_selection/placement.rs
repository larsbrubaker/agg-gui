//! Where touch selection's handles and toolbar are ([`super`]): which
//! handles show (only those whose caret is in the host's text area and
//! inside the clip of its ancestors), which one a point grabs, where the
//! toolbar floats and what it offers, and the overlay paint and hit test
//! the host forwards from `paint_global_overlay` / `hit_test_global_overlay`.

use crate::draw_ctx::DrawCtx;
use crate::geometry::{Point, Rect};

use super::handles::{self, CaretLine, HandleKind, HandleStyle};
use super::{toolbar, TouchSelectable};

fn handle_offset<W: TouchSelectable>(w: &W, kind: HandleKind) -> usize {
    let (a, c) = w.ts_anchor_cursor();
    match kind {
        HandleKind::Start => a.min(c),
        HandleKind::End => a.max(c),
        HandleKind::Caret => c,
    }
}

/// Whether the caret `line` shows: inside the host's text area and inside
/// its ancestors' clip (a ScrollView scrolled it away, say).
fn caret_shows<W: TouchSelectable>(w: &W, line: CaretLine) -> bool {
    let shown = w.ts_text_rect();
    let in_text = line.x >= shown.x - 1.0
        && line.x <= shown.x + shown.width + 1.0
        && line.top > shown.y
        && line.bottom < shown.y + shown.height;
    let mid = line.middle();
    in_text
        && w.touch_selection().clip.get().is_none_or(|c| {
            mid.x >= c.x - 1.0
                && mid.x <= c.x + c.width + 1.0
                && mid.y >= c.y
                && mid.y <= c.y + c.height
        })
}

/// The handles on screen now, with the caret line each hangs from.
pub(crate) fn visible_handles<W: TouchSelectable>(w: &W) -> Vec<(HandleKind, CaretLine)> {
    let ts = w.touch_selection();
    let (a, c) = w.ts_anchor_cursor();
    let kinds: &[HandleKind] = if ts.selection_handles && a != c {
        &[HandleKind::Start, HandleKind::End]
    } else if ts.caret_handle_shown() && a == c {
        &[HandleKind::Caret]
    } else {
        &[]
    };
    kinds
        .iter()
        .map(|k| (*k, w.ts_caret_line(handle_offset(w, *k))))
        .filter(|(_, line)| caret_shows(w, *line))
        .collect()
}

/// The handle whose grab area holds `pos`, the nearest when they overlap.
pub(crate) fn handle_at<W: TouchSelectable>(w: &W, pos: Point) -> Option<HandleKind> {
    let style = HandleStyle::current();
    visible_handles(w)
        .into_iter()
        .filter(|(k, line)| toolbar::contains(handles::hit_rect(style, *k, *line), pos))
        .map(|(k, line)| {
            let c = handles::body_center(style, k, line);
            (k, (c.x - pos.x).powi(2) + (c.y - pos.y).powi(2))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(k, _)| k)
}

/// The window the toolbar must stay in, in root logical space: the visible
/// viewport above the on-screen keyboard.
fn toolbar_room() -> Rect {
    let viewport = crate::widget::current_viewport();
    let visible = crate::widget::visible_root_rect(viewport);
    let keyboard_top = crate::widgets::on_screen_keyboard::occluded_height(viewport.height)
        - crate::widget::keyboard_scroll::current_lift();
    let bottom = visible.y.max(keyboard_top);
    Rect::new(
        visible.x,
        bottom,
        visible.width,
        (visible.y + visible.height - bottom).max(0.0),
    )
}

/// The toolbar's layout while it shows: over the selection (or caret) when
/// either end of it shows, offering what applies.
pub(crate) fn toolbar_layout<W: TouchSelectable>(w: &W) -> Option<toolbar::ToolbarLayout> {
    let ts = w.touch_selection();
    if !ts.toolbar_shown() || !w.ts_toolbar_allowed() {
        return None;
    }
    let (a, c) = w.ts_anchor_cursor();
    let (lo, hi) = (a.min(c), a.max(c));
    let start = w.ts_caret_line(lo);
    let end = w.ts_caret_line(hi);
    if !caret_shows(w, start) && !caret_shows(w, end) {
        return None;
    }
    let len = w.ts_text().len();
    let copyable = lo != hi && !w.ts_secret();
    let actions = toolbar::actions(
        copyable,
        w.ts_editable(),
        ts.paste_available,
        len > 0,
        lo == 0 && hi == len,
    );
    let shown = w.ts_text_rect();
    let center_x = if (start.top - end.top).abs() < 0.5 {
        (start.x + end.x) * 0.5
    } else {
        shown.x + shown.width * 0.5
    };
    let anchor = toolbar::Anchor {
        center_x,
        top: start.top.min(shown.y + shown.height),
        bottom: end.bottom.max(shown.y),
    };
    toolbar::layout(
        &actions,
        HandleStyle::current(),
        &w.ts_font(),
        anchor,
        ts.origin.get(),
        toolbar_room(),
    )
}

/// Whether `pos` (host-local) is on a handle or the toolbar, for the host's
/// `hit_test_global_overlay`.
pub(crate) fn hit_test<W: TouchSelectable>(w: &W, pos: Point) -> bool {
    handle_at(w, pos).is_some() || toolbar_layout(w).is_some_and(|l| l.contains(pos))
}

/// Draw the handles and the toolbar (from the host's `paint_global_overlay`).
pub(crate) fn paint<W: TouchSelectable>(w: &W, ctx: &mut dyn DrawCtx) {
    let style = HandleStyle::current();
    let accent = ctx.visuals().accent;
    for (kind, line) in visible_handles(w) {
        handles::paint(ctx, style, kind, line, accent);
    }
    if let Some(layout) = toolbar_layout(w) {
        toolbar::paint(ctx, &layout, style, &w.ts_font());
    }
}
