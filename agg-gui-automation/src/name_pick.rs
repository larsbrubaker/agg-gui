//! Which of several same-named things a click by name goes to, and where on
//! it the click lands — the choice agg-sharp's `GetWidgetByName` makes
//! (`GuiAutomation/AutomationRunner.cs`, with `GuiAutomation/PointerReach.cs`)
//! as pure functions of a widget tree and the window's size, so the runner
//! ([`crate::runner`]) and an application's own harness share one rule
//! without a runner, and a wait's predicate can ask it without waiting.
//!
//! The rule:
//! 1. Only what is on show counts: the widget and every ancestor visible and
//!    some of it left by the clips painting puts it under
//!    ([`crate::paint_geometry`]); a named target ([`NamedHit::target`]) by
//!    its own rectangle inside the window.
//! 2. Those a press can reach come first (C# `PreferReachable`): the click
//!    point shows (no clip hides it) and no ancestor refuses a press there
//!    ([`pointer_reach::can_reach`]). When none can be reached, all count.
//! 3. Of those, the largest visible area wins, the first in tree order on a
//!    tie. Areas within a relative [`AREA_TOLERANCE`] tie: two same-sized
//!    widgets at different offsets come out a few ulps apart once moved to
//!    window space, and C#'s strict comparison let that noise pick the
//!    second of equals.
//! 4. The click lands at the hit's centre (C# `MoveMouseToWidget`), unless a
//!    clip hides the centre (a row half scrolled out of its list): then at
//!    the centre of the part that shows. C# pressed whatever lay over the
//!    hidden centre, although its lookup had just required the widget to
//!    show; a click by name should press the widget it found.
//!
//! Rules 3's tolerance and 4 differ from C# for every consumer, not just
//! one: both make a lookup do what its own contract says (the first of
//! equals; a found widget is a clickable one).

use agg_gui::{Point, Rect, Size, Widget};

use crate::paint_geometry::intersect;
use crate::pointer_reach;
use crate::tree_query::{self, NamedHit};

/// The relative difference below which two visible areas count as equal.
pub const AREA_TOLERANCE: f64 = 1e-9;

/// Where a [`NamedHit`] sits in the window and what of it shows (logical,
/// Y-up).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HitPlacement {
    /// The widget's rectangle, or the target's for a target.
    pub screen_rect: Rect,
    /// The rectangle of the hit's widget (the one reporting a target);
    /// [`NamedHit::offset_hint`] is measured from its lower-left corner.
    pub widget_rect: Rect,
    /// `screen_rect` cut by what hides it: a widget's by every clip
    /// painting puts it under and the window, a target's by the window
    /// (drop-down and menu rows paint in the overlay pass, outside their
    /// widget's clips). Zero-sized when nothing shows.
    pub visible_rect: Rect,
    /// Every widget from the root down to the hit's widget is visible.
    pub visible_chain: bool,
}

impl HitPlacement {
    /// C# `ActuallyVisibleOnScreen`: visible all the way up, and some of it
    /// shows.
    pub fn shows(&self) -> bool {
        self.visible_chain && self.visible_rect.width > 0.0 && self.visible_rect.height > 0.0
    }

    /// The area left on show (C# `ClippedOnScreenBounds` area).
    pub fn visible_area(&self) -> f64 {
        self.visible_rect.width * self.visible_rect.height
    }

    /// Whether `point` (window, Y-up) lies in the part that shows, edges
    /// included.
    pub fn shows_point(&self, point: Point) -> bool {
        let v = self.visible_rect;
        point.x >= v.left() && point.x <= v.right() && point.y >= v.bottom() && point.y <= v.top()
    }

    /// The window point `offset_hint` aims at: offset from the lower-left
    /// corner of `widget_rect`, as the runner's clicks apply it.
    pub fn point_at(&self, offset_hint: Point) -> Point {
        Point::new(
            self.widget_rect.x + offset_hint.x,
            self.widget_rect.y + offset_hint.y,
        )
    }
}

/// Where `hit` sits and what of it shows in a `viewport`-sized window;
/// `None` once its widget is detached.
pub fn hit_placement(root: &dyn Widget, hit: &NamedHit, viewport: Size) -> Option<HitPlacement> {
    let placement = tree_query::placement(root, &hit.handle, viewport)?;
    match &hit.target {
        None => Some(HitPlacement {
            screen_rect: placement.screen_rect,
            widget_rect: placement.screen_rect,
            visible_rect: placement.clipped_rect,
            visible_chain: placement.visible_chain,
        }),
        Some(target) => {
            let rect = tree_query::local_rect_to_screen(root, &hit.handle, target.rect_local)?;
            let window = Rect::new(0.0, 0.0, viewport.width, viewport.height);
            Some(HitPlacement {
                screen_rect: rect,
                widget_rect: placement.screen_rect,
                visible_rect: intersect(window, rect),
                visible_chain: placement.visible_chain,
            })
        }
    }
}

/// Rule 2: the hit's click point shows and no ancestor refuses a press
/// there. False once its widget is detached.
pub fn can_reach_hit(root: &dyn Widget, hit: &NamedHit, viewport: Size) -> bool {
    hit_placement(root, hit, viewport).is_some_and(|p| p.shows_point(p.point_at(hit.offset_hint)))
        && pointer_reach::can_reach(root, &hit.handle)
}

/// Rule 4: `hit` with its [`offset_hint`](NamedHit::offset_hint) moved to
/// the centre of its visible part when a clip hides the point it aims at.
/// Unchanged when that point shows, when nothing shows, or once detached.
pub fn aimed(root: &dyn Widget, mut hit: NamedHit, viewport: Size) -> NamedHit {
    if let Some(p) = hit_placement(root, &hit, viewport) {
        if p.shows() && !p.shows_point(p.point_at(hit.offset_hint)) {
            let v = p.visible_rect;
            hit.offset_hint = Point::new(
                v.x + v.width / 2.0 - p.widget_rect.x,
                v.y + v.height / 2.0 - p.widget_rect.y,
            );
        }
    }
    hit
}

/// The window point (logical, Y-up) a click on `hit` lands at, after
/// [`aimed`]; `None` once its widget is detached.
pub fn aim_point(root: &dyn Widget, hit: &NamedHit, viewport: Size) -> Option<Point> {
    let hit = aimed(root, hit.clone(), viewport);
    hit_placement(root, &hit, viewport).map(|p| p.point_at(hit.offset_hint))
}

/// Rules 2 to 4 over `hits` (already limited to what shows, in tree order):
/// the one a click goes to, aimed. `None` when `hits` is empty.
pub fn pick_named_hit(root: &dyn Widget, hits: Vec<NamedHit>, viewport: Size) -> Option<NamedHit> {
    let reachable: Vec<NamedHit> = hits
        .iter()
        .filter(|hit| can_reach_hit(root, hit, viewport))
        .cloned()
        .collect();
    let candidates = if reachable.is_empty() {
        hits
    } else {
        reachable
    };
    let mut best = candidates.first()?.clone();
    if candidates.len() > 1 {
        let mut best_area = 0.0;
        for candidate in &candidates {
            let area = hit_placement(root, candidate, viewport).map_or(0.0, |p| p.visible_area());
            if area > best_area * (1.0 + AREA_TOLERANCE) {
                best_area = area;
                best = candidate.clone();
            }
        }
    }
    Some(aimed(root, best, viewport))
}

/// Every hit named `name` that shows (rule 1), in tree order.
pub fn hits_on_show(root: &dyn Widget, name: &str, viewport: Size) -> Vec<NamedHit> {
    tree_query::find_named_hits(root, name)
        .into_iter()
        .filter(|hit| hit_placement(root, hit, viewport).is_some_and(|p| p.shows()))
        .collect()
}

/// The thing named `name` a click by name goes to right now, aimed: rules 1
/// to 4 in one look, with no waiting. `None` when nothing named `name`
/// shows.
pub fn find_on_screen(root: &dyn Widget, name: &str, viewport: Size) -> Option<NamedHit> {
    pick_named_hit(root, hits_on_show(root, name, viewport), viewport)
}
