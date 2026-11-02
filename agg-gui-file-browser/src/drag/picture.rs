//! The picture a drag carries under the pointer, so the user sees what they picked up: the first
//! carried item's art (what its row or tile draws: its picture, or the look's icon) floats below
//! and right of the pointer, 64 units square at three-quarter opacity, so the pointer and the
//! folder under it stay in view. More than one item carried puts the count on its top-right corner
//! (a disc in the accent with the number in white); a drop that would copy puts the "+" badge on
//! its bottom-right corner, by the rule the folder's own badge follows
//! (`DropManager::copy_target`). Outside the browser the app's `OutsideDrops::over` may hide it,
//! whatever it answers (`OutsideOver::hides_picture`: a view that shows the items itself, or an
//! app whose drag pictures show only over the browser); without a hook, or where the hook leaves
//! it, the picture follows the pointer everywhere. It comes back inside the browser. The rows and tiles being carried paint at half opacity while the drag lasts
//! ([`lift`], [`finish_lift`]), so they read as picked up.
//!
//! The picture is drawn in agg-gui's global overlay pass, after the whole widget tree, so it lies
//! on top of everything: by the composed browser (`FileBrowser::paint_global_overlay`), once for
//! its tree and list, or by a list standing alone (`BrowserList::paint_global_overlay`). The art is the one a
//! carried row last painted, handed over as it paints: nothing is looked up or made per frame.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::text::Font;
use agg_gui::{Color, DrawCtx, Rect};
use agg_gui_files::Item;

use super::badge::{paint_plus_badge, painted_rect};
use crate::drops::OutsideOver;
use crate::list::tile_art::{paint_art, Art, TileArt};
use crate::look::BrowserStyle;
use crate::state::BrowserState;

/// The picture's side, in logical units.
pub const PICTURE_SIDE: f64 = 64.0;
/// The picture's opacity: solid enough to read, faint enough to see the folder under it.
const PICTURE_ALPHA: f64 = 0.75;
/// A carried row's opacity while the drag lasts.
pub const CARRIED_ALPHA: f64 = 0.5;
/// How far right of and below the pointer the picture starts, clear of the arrow.
const POINTER_GAP: f64 = 16.0;
/// The count badge's radius, and the "+" badge's.
const COUNT_RADIUS: f64 = 10.0;
const PLUS_RADIUS: f64 = 9.0;

/// One frame's picture: its window rectangle (logical units, Y up) and badges.
#[derive(Clone, Copy, Debug)]
struct Shown {
    rect: Rect,
    count: usize,
    copies: bool,
}

/// What the drag's picture shows and where it was last drawn.
#[derive(Clone, Default)]
pub struct DragPicture {
    /// The carried row's art, the name its row reports it by (`thumbnail`), and whether it is
    /// the first carried item's.
    art: Option<(Art, String, bool)>,
    /// The last frame's picture, `None` while hidden.
    shown: Option<Shown>,
}

impl DragPicture {
    /// A carried row at `rank` among the carried items painted `art`. The first item's art always
    /// wins (kept current as its picture arrives); another's stands in only until the first one
    /// paints (it may be scrolled away).
    fn offer(&mut self, rank: usize, art: &TileArt) {
        if rank != 0 && self.art.is_some() {
            return;
        }
        self.art = Some((art.art.clone(), art.name.clone(), rank == 0));
    }
}

/// The start of a row's or tile's paint: a carried one paints at half opacity until
/// [`finish_lift`]. Answers its place among the carried items, for `finish_lift`.
pub fn lift(
    ctx: &mut dyn DrawCtx,
    state: &Rc<RefCell<BrowserState>>,
    item: &Rc<dyn Item>,
) -> Option<usize> {
    let rank = state.borrow().drop.carried_rank(item)?;
    ctx.save();
    ctx.set_global_alpha(CARRIED_ALPHA);
    Some(rank)
}

/// The end of a paint begun with [`lift`]: a carried row hands the drag the art it drew and its
/// opacity comes back.
pub fn finish_lift(
    ctx: &mut dyn DrawCtx,
    state: &Rc<RefCell<BrowserState>>,
    rank: Option<usize>,
    art: &TileArt,
) {
    let Some(rank) = rank else {
        return;
    };
    ctx.restore();
    state.borrow_mut().drop.picture.offer(rank, art);
}

/// The drag's picture by the pointer, on top of everything (a global overlay's paint).
pub fn paint_drag_picture(
    ctx: &mut dyn DrawCtx,
    state: &Rc<RefCell<BrowserState>>,
    style: &BrowserStyle,
    font: &Arc<Font>,
) {
    let origin = painted_rect(ctx, 0.0, 0.0);
    let shown = {
        let s = state.borrow();
        let count = s.drop.dragged_items().len();
        let hidden = s.drop.outside.is_some_and(OutsideOver::hides_picture);
        let at = s
            .item_press()
            .last_pointer()
            .filter(|_| count > 0 && !hidden);
        at.map(|at| Shown {
            rect: Rect::new(
                at.x + POINTER_GAP,
                at.y - POINTER_GAP - PICTURE_SIDE,
                PICTURE_SIDE,
                PICTURE_SIDE,
            ),
            count,
            copies: s.drop.copy_target().is_some(),
        })
    };
    let art = {
        let mut s = state.borrow_mut();
        s.drop.picture.shown = shown;
        s.drop.picture.art.clone()
    };
    let Some(shown) = shown else {
        return;
    };
    // The list's own coordinates.
    let rect = Rect::new(
        shown.rect.x - origin.x,
        shown.rect.y - origin.y,
        shown.rect.width,
        shown.rect.height,
    );
    ctx.save();
    ctx.set_global_alpha(PICTURE_ALPHA);
    ctx.set_fill_color(style.background);
    ctx.begin_path();
    ctx.rounded_rect(rect.x, rect.y, rect.width, rect.height, style.corner_radius);
    ctx.fill();
    if let Some((art, _, _)) = art {
        paint_art(ctx, &art, rect);
    }
    ctx.restore();
    if shown.count > 1 {
        let (cx, cy) = (rect.x + rect.width, rect.y + rect.height);
        paint_count_badge(ctx, style, font, shown.count, cx, cy);
    }
    if shown.copies {
        paint_plus_badge(ctx, rect.x + rect.width, rect.y, PLUS_RADIUS, style.accent);
    }
}

/// How many items the drag carries, in white on an accent disc centred on `(cx, cy)`.
fn paint_count_badge(
    ctx: &mut dyn DrawCtx,
    style: &BrowserStyle,
    font: &Arc<Font>,
    count: usize,
    cx: f64,
    cy: f64,
) {
    let text = count.to_string();
    // A long count widens the disc into a pill rather than spilling out of it.
    let size = style.font_size * 0.85;
    let width = agg_gui::measure_text_metrics(font, &text, size).width;
    let half = (width / 2.0 + 4.0).max(COUNT_RADIUS);
    let accent = style.accent;
    ctx.set_fill_color(Color::rgba(accent.r, accent.g, accent.b, 1.0));
    ctx.begin_path();
    ctx.rounded_rect(
        cx - half,
        cy - COUNT_RADIUS,
        half * 2.0,
        COUNT_RADIUS * 2.0,
        COUNT_RADIUS,
    );
    ctx.fill();
    ctx.set_fill_color(Color::rgba(1.0, 1.0, 1.0, 1.0));
    ctx.set_font(Arc::clone(font));
    ctx.set_font_size(size);
    ctx.fill_text(&text, cx - width / 2.0, cy - size * 0.35);
}

/// What the drag's picture showed in the last frame: its window rectangle ("x,y,width,height",
/// logical units, Y up; empty while hidden), the count it carries and whether the copy badge
/// shows, and the name of the art it shows (the carried row's `thumbnail`; empty while hidden).
/// The list's `drag_picture`, `drag_picture_count`, `drag_picture_copies` and
/// `drag_picture_image` properties.
pub fn picture_properties(state: &BrowserState) -> [(&'static str, String); 4] {
    let shown = state.drop.picture.shown;
    let rect = shown.map_or_else(String::new, |s| {
        format!(
            "{},{},{},{}",
            s.rect.x, s.rect.y, s.rect.width, s.rect.height
        )
    });
    [
        ("drag_picture", rect),
        (
            "drag_picture_count",
            shown.map_or(0, |s| s.count).to_string(),
        ),
        (
            "drag_picture_copies",
            shown.is_some_and(|s| s.copies).to_string(),
        ),
        (
            "drag_picture_image",
            match (shown, &state.drop.picture.art) {
                (Some(_), Some((_, name, _))) => name.clone(),
                _ => String::new(),
            },
        ),
    ]
}
