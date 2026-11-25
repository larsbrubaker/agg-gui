//! `BarTab` — a split bar that carries a pull tab, which collapses one side
//! of the split and brings it back.
//!
//! The bar and the tab are one object: the tab is a 48-unit bulge on the
//! bar's side facing the pane that stays, filled the bar's colour as one
//! continuous shape (concave fillets where it leaves the bar, rounded outer
//! corners, no outline), with a chevron across tab and bar pointing the way
//! the collapsible side will move. Collapsed, the host keeps the bar at the
//! edge with its tab, so open and collapsed look like the same thing.
//!
//! Gestures (see [`BarTab::on_event`]):
//! - a click on the tab toggles the side;
//! - a finger dragged across the tab toward the pane that stays opens the
//!   side, toward the edge closes it, once per press (a swipe, for touch,
//!   where the tab is a small target to tap);
//! - a mouse drag on the tab past the click allowance is a resize drag,
//!   like one on the bar: someone who grabs the tab with a mouse expects it
//!   to pull the bar along;
//! - a double click on the bar toggles the side; on the tab, its second
//!   press (which lands where the tab was before the first click moved it)
//!   is swallowed, so the pane now under it never sees a double click;
//! - a press on the bar is otherwise the host's resize drag
//!   ([`BarTabResponse::BeginResize`]), from collapsed too: the host's
//!   drag reopens the side once the pointer reaches where the open bar can
//!   sit, and the bar follows the pointer for the rest of the press. No
//!   drag collapses the side.
//!
//! A toggle answers [`BarTabResponse::Toggled`]: the bar has moved, so the
//! host re-places it and calls [`BarTab::update_hover`] with the pointer,
//! or hover and tooltip would describe where the bar used to be.
//!
//! The piece is not a widget: [`Splitter::with_collapse_tab`] hosts one, and
//! a host whose collapsible side is not one of a `Splitter`'s two panes (a
//! side column laid out by its page) hosts one the same way: place it with
//! [`BarTab::place`], route events through [`BarTab::on_event`], paint it
//! from `paint_overlay` (the tab overlaps the neighbouring pane), claim the
//! pointer where [`BarTab::claims`] says, and answer `tooltip_text` from
//! [`BarTab::tooltip`]. The state lives in a [`SplitterCollapsed`] cell the
//! host shares, so it can save the choice and other views can follow it.
//!
//! [`Splitter::with_collapse_tab`]: crate::widgets::Splitter::with_collapse_tab

use std::sync::{Arc, Mutex};

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::event::{current_click_count, Event, MouseButton, DOUBLE_CLICK_WINDOW};
use crate::geometry::{Point, Rect};
use web_time::Instant;

/// The tab's length along the bar.
pub const TAB_LENGTH: f64 = 48.0;
/// How far the tab reaches past the bar.
pub const TAB_BULGE: f64 = 8.0;
/// The concave fillets where the tab leaves the bar.
const TAB_FILLET: f64 = 4.0;
/// The tab's rounded outer corners.
const TAB_RADIUS: f64 = 5.0;
/// The chevron's arm and stroke.
const CHEVRON_ARM: f64 = 4.0;
const CHEVRON_WIDTH: f64 = 1.75;
/// The chevron's ink: the text colour at this alpha (the accent while the
/// tab is hovered).
const CHEVRON_ALPHA: f32 = 0.6;
/// The black film laid over bar and tab while hovered: one step darker,
/// whether the bar colour is opaque or translucent.
const HOVER_DARKEN: f32 = 0.14;
/// Travel across the tab that opens or closes the side, once per press.
pub const DRAG_TO_TOGGLE: f64 = 24.0;
/// Travel a press on the tab may make and still be a click.
const TAP_SLOP: f64 = 10.0;
/// Slack round the bar a press or hover still counts as on it, so a thin
/// bar stays easy to grab (the same as `Splitter`'s plain bar).
const BAR_SLACK: f64 = 2.0;

/// Shared, clonable collapsed state — the seam between a [`BarTab`] buried
/// in a widget tree and a host that saves it or makes other views follow.
///
/// Two-way like [`SplitterRatio`](crate::widgets::SplitterRatio): the tab
/// writes it on every toggle, and the host's writes land on the next layout
/// (a write requests one). Poisoning is swallowed: the payload is one
/// `bool`.
#[derive(Clone, Debug)]
pub struct SplitterCollapsed(Arc<Mutex<bool>>);

impl SplitterCollapsed {
    /// New cell, collapsed when `collapsed`.
    pub fn new(collapsed: bool) -> Self {
        Self(Arc::new(Mutex::new(collapsed)))
    }

    /// Whether the collapsible side is collapsed.
    pub fn get(&self) -> bool {
        *self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Collapse or open the side; takes effect on the next layout.
    pub fn set(&self, collapsed: bool) {
        let mut value = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if *value != collapsed {
            *value = collapsed;
            crate::animation::request_layout();
            crate::animation::request_draw();
        }
    }

    /// Flip the state.
    pub fn toggle(&self) {
        self.set(!self.get());
    }
}

/// Which pane of the split collapses: `First` is the left (or top) pane,
/// `Second` the right (or bottom). The tab faces the other pane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CollapseSide {
    First,
    Second,
}

/// What [`BarTab::on_event`] did with an event, for its host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarTabResponse {
    /// Not the tab's: the host handles it (or lets it pass).
    Ignored,
    /// The tab handled it.
    Consumed,
    /// A press on the bar that is not a double click: the host's resize drag
    /// starts, from collapsed too (the host reopens the side, through
    /// [`BarTab::state`], once the drag reaches where the open bar can sit).
    BeginResize,
    /// The side was collapsed or opened by this event. The bar moves at the
    /// host's next layout: re-place it now and call
    /// [`BarTab::update_hover`] with the pointer.
    Toggled,
}

/// A press in progress on the tab or the bar.
#[derive(Clone, Copy, Debug)]
enum Press {
    /// On the tab: where it went down, whether a finger's swipe has already
    /// toggled, and whether the press is a finger's.
    Tab {
        down: Point,
        toggled: bool,
        touch: bool,
    },
    /// On the bar (or a tab dragged by the mouse), given to the host's
    /// resize drag, or spent.
    Bar,
    /// A double click's second press where the tab was before its first
    /// click moved it: swallowed, release included.
    Swallowed,
}

/// The split bar's pull tab: placement, paint and gestures. See the module
/// docs for how a host uses it.
pub struct BarTab {
    state: SplitterCollapsed,
    side: CollapseSide,
    /// The bar (host-local, Y up) and whether the split is top/bottom.
    bar: Rect,
    vertical: bool,
    /// Where the tab is centred along the bar (host-local).
    center: f64,
    bar_color: Option<Color>,
    hide_label: String,
    show_label: String,
    bar_hovered: bool,
    tab_hovered: bool,
    press: Option<Press>,
    /// Where the tab was when a click on it toggled the side, and when: a
    /// second press of a double click there is swallowed.
    last_toggle: Option<(Rect, Instant)>,
}

impl BarTab {
    /// A tab that collapses `side`, its state in `state`.
    pub fn new(state: SplitterCollapsed, side: CollapseSide) -> Self {
        Self {
            state,
            side,
            bar: Rect::default(),
            vertical: false,
            center: 0.0,
            bar_color: None,
            hide_label: String::new(),
            show_label: String::new(),
            bar_hovered: false,
            tab_hovered: false,
            press: None,
            last_toggle: None,
        }
    }

    /// The tab's tooltip: `hide` while the side is open, `show` while it is
    /// collapsed (the host localizes both). Empty text shows no tooltip.
    pub fn set_tooltips(&mut self, hide: impl Into<String>, show: impl Into<String>) {
        self.hide_label = hide.into();
        self.show_label = show.into();
    }

    /// The bar's colour; `None` (the default) uses the visuals' text colour
    /// at the plain `Splitter` bar's alpha.
    pub fn set_bar_color(&mut self, color: Option<Color>) {
        self.bar_color = color;
    }

    /// The shared state cell.
    pub fn state(&self) -> &SplitterCollapsed {
        &self.state
    }

    /// Which side collapses.
    pub fn side(&self) -> CollapseSide {
        self.side
    }

    /// Whether the side is collapsed.
    pub fn collapsed(&self) -> bool {
        self.state.get()
    }

    /// Place the bar at `bar` (host-local, Y up) in a left/right split, or a
    /// top/bottom one when `vertical`, with the tab centred at `center` along
    /// the bar (Y for a left/right split, X for top/bottom).
    pub fn place(&mut self, bar: Rect, vertical: bool, center: f64) {
        self.bar = bar;
        self.vertical = vertical;
        self.center = center;
    }

    /// Whether the bar or the tab is hovered (bar and tab darken together).
    pub fn hovered(&self) -> bool {
        self.bar_hovered || self.tab_hovered
    }

    /// Whether the tab itself is hovered.
    pub fn tab_hovered(&self) -> bool {
        self.tab_hovered
    }

    /// Which way, along the split axis, the pane that stays lies from the
    /// bar: the tab bulges this way, and a drag this way opens the side.
    fn toward_view(&self) -> f64 {
        // X grows rightward, so the first (left) pane is at -1; Y grows
        // upward and the first pane is the top, so it is at +1.
        match (self.side, self.vertical) {
            (CollapseSide::Second, false) => -1.0,
            (CollapseSide::First, false) => 1.0,
            (CollapseSide::Second, true) => 1.0,
            (CollapseSide::First, true) => -1.0,
        }
    }

    /// The bar's thickness and its edge facing the pane that stays, along
    /// the split axis.
    fn bar_thickness_and_edge(&self) -> (f64, f64) {
        let (start, thickness) = if self.vertical {
            (self.bar.y, self.bar.height)
        } else {
            (self.bar.x, self.bar.width)
        };
        let edge = if self.toward_view() > 0.0 {
            start + thickness
        } else {
            start
        };
        (thickness, edge)
    }

    /// Host-local point for `s` along the split axis (out from the bar's
    /// view-facing edge, toward the view) and `t` along the bar.
    fn to_local(&self, s: f64, t: f64) -> Point {
        let (_, edge) = self.bar_thickness_and_edge();
        let across = edge + self.toward_view() * s;
        if self.vertical {
            Point::new(t, across)
        } else {
            Point::new(across, t)
        }
    }

    /// `p` (host-local) as (`s`, `t`), the inverse of [`Self::to_local`].
    fn to_bar_frame(&self, p: Point) -> (f64, f64) {
        let (_, edge) = self.bar_thickness_and_edge();
        let (across, along) = if self.vertical {
            (p.y, p.x)
        } else {
            (p.x, p.y)
        };
        ((across - edge) * self.toward_view(), along)
    }

    /// The tab's rectangle (host-local), the bulge only, fillets excluded.
    pub fn tab_rect(&self) -> Rect {
        let a = self.to_local(0.0, self.center - TAB_LENGTH / 2.0);
        let b = self.to_local(TAB_BULGE, self.center + TAB_LENGTH / 2.0);
        Rect::new(
            a.x.min(b.x),
            a.y.min(b.y),
            (a.x - b.x).abs(),
            (a.y - b.y).abs(),
        )
    }

    /// Whether `p` (host-local) is on the tab.
    pub fn on_tab(&self, p: Point) -> bool {
        let (s, t) = self.to_bar_frame(p);
        s > 0.0 && s <= TAB_BULGE && (t - self.center).abs() <= TAB_LENGTH / 2.0
    }

    /// Whether `p` (host-local) is on the bar (with a little slack).
    pub fn on_bar(&self, p: Point) -> bool {
        let (thickness, _) = self.bar_thickness_and_edge();
        let (s, t) = self.to_bar_frame(p);
        let length = if self.vertical {
            (self.bar.x, self.bar.x + self.bar.width)
        } else {
            (self.bar.y, self.bar.y + self.bar.height)
        };
        s >= -thickness - BAR_SLACK && s <= BAR_SLACK && t >= length.0 && t <= length.1
    }

    /// Whether the host should claim the pointer at `p` ahead of its
    /// children: on the tab or the bar, or while a press is in progress.
    pub fn claims(&self, p: Point) -> bool {
        self.press.is_some() || self.on_tab(p) || self.on_bar(p) || self.swallows(p)
    }

    /// Whether a press at `p` is the second of a double click on the tab,
    /// landing where the tab was before its first click moved it.
    fn swallows(&self, p: Point) -> bool {
        self.last_toggle.is_some_and(|(r, at)| {
            current_click_count() >= 2
                && crate::clock::now().saturating_duration_since(at) < DOUBLE_CLICK_WINDOW
                && p.x >= r.x
                && p.x <= r.x + r.width
                && p.y >= r.y
                && p.y <= r.y + r.height
        })
    }

    /// Whether a press is in progress on the tab (the host's resize drag is
    /// not running then).
    pub fn pressing_tab(&self) -> bool {
        matches!(self.press, Some(Press::Tab { .. }))
    }

    /// The tooltip while the tab is hovered.
    pub fn tooltip(&self) -> Option<&str> {
        if !self.tab_hovered {
            return None;
        }
        let label = if self.collapsed() {
            &self.show_label
        } else {
            &self.hide_label
        };
        (!label.is_empty()).then_some(label.as_str())
    }

    /// `base` one step darker: a black film at [`HOVER_DARKEN`] laid over
    /// it, so an opaque colour darkens its RGB and a translucent one (a
    /// black at low alpha, where scaling RGB changes nothing) gains alpha.
    pub fn darken(base: Color) -> Color {
        let k = HOVER_DARKEN;
        let a = k + base.a * (1.0 - k);
        // `a >= k > 0`, so the division is defined.
        let keep = base.a * (1.0 - k) / a;
        Color::rgba(base.r * keep, base.g * keep, base.b * keep, a)
    }

    /// The bar's fill as painted now (darkened while hovered or `active`,
    /// the host's resize drag).
    pub fn fill_color(&self, text_color: Color, active: bool) -> Color {
        let base = self
            .bar_color
            .unwrap_or_else(|| text_color.with_alpha(0.08));
        if self.hovered() || active {
            Self::darken(base)
        } else {
            base
        }
    }

    /// Paint the bar and the tab as one shape, then the chevron. `active`
    /// is the host's resize drag (painted as hovered).
    pub fn paint(&self, ctx: &mut dyn DrawCtx, active: bool) {
        let v = ctx.visuals();
        let fill = self.fill_color(v.text_color, active);
        let ink = if self.tab_hovered {
            v.accent
        } else {
            v.text_color.with_alpha(CHEVRON_ALPHA)
        };
        let (thickness, _) = self.bar_thickness_and_edge();
        let (t0, t1) = if self.vertical {
            (self.bar.x, self.bar.x + self.bar.width)
        } else {
            (self.bar.y, self.bar.y + self.bar.height)
        };
        let (f, r, bulge) = (TAB_FILLET, TAB_RADIUS, TAB_BULGE);
        let (lo, hi) = (
            self.center - TAB_LENGTH / 2.0,
            self.center + TAB_LENGTH / 2.0,
        );
        // The outline in the bar's frame: down the bar's far side, then up
        // its view-facing side with the tab on the way.
        let mut path = PathBuilder { tab: self, ctx };
        path.ctx.set_fill_color(fill);
        path.ctx.begin_path();
        path.move_to(-thickness, t0);
        path.line_to(0.0, t0);
        path.line_to(0.0, lo - f);
        path.quad_to(0.0, lo, f, lo);
        path.line_to(bulge - r, lo);
        path.quad_to(bulge, lo, bulge, lo + r);
        path.line_to(bulge, hi - r);
        path.quad_to(bulge, hi, bulge - r, hi);
        path.line_to(f, hi);
        path.quad_to(0.0, hi, 0.0, hi + f);
        path.line_to(0.0, t1);
        path.line_to(-thickness, t1);
        path.ctx.close_path();
        path.ctx.fill();
        // The chevron across tab and bar together, pointing the way the
        // side moves: toward the edge while open, toward the view collapsed.
        let mid = (bulge - thickness) / 2.0;
        let point = if self.collapsed() {
            CHEVRON_ARM / 2.0
        } else {
            -CHEVRON_ARM / 2.0
        };
        path.ctx.set_stroke_color(ink);
        path.ctx.set_line_width(CHEVRON_WIDTH);
        path.ctx.begin_path();
        path.move_to(mid - point, self.center + CHEVRON_ARM);
        path.line_to(mid + point, self.center);
        path.line_to(mid - point, self.center - CHEVRON_ARM);
        path.ctx.stroke();
    }

    /// Update hover from a pointer at `p` (host-local); `true` when it
    /// changed (the host requests a draw).
    pub fn update_hover(&mut self, p: Option<Point>) -> bool {
        let (tab, bar) = match p {
            Some(p) => {
                let tab = self.on_tab(p);
                (tab, !tab && self.on_bar(p))
            }
            None => (false, false),
        };
        let changed = tab != self.tab_hovered || bar != self.bar_hovered;
        self.tab_hovered = tab;
        self.bar_hovered = bar;
        changed
    }

    /// Handle a pointer event in host-local coordinates. See the module docs
    /// for the gestures; [`BarTabResponse::BeginResize`] hands a bar press
    /// to the host's resize drag.
    pub fn on_event(&mut self, event: &Event) -> BarTabResponse {
        match event {
            Event::MouseDown {
                pos,
                button: MouseButton::Left,
                ..
            } => self.press_at(*pos),
            Event::MouseMove { pos } => self.move_to(*pos),
            Event::MouseUp {
                pos,
                button: MouseButton::Left,
                ..
            } => match self.press.take() {
                Some(Press::Tab {
                    down,
                    toggled: false,
                    ..
                }) => {
                    if (pos.x - down.x).hypot(pos.y - down.y) <= TAP_SLOP {
                        self.last_toggle = Some((self.tab_rect(), crate::clock::now()));
                        self.state.toggle();
                        return BarTabResponse::Toggled;
                    }
                    BarTabResponse::Consumed
                }
                Some(Press::Tab { .. }) | Some(Press::Swallowed) => BarTabResponse::Consumed,
                Some(Press::Bar) | None => BarTabResponse::Ignored,
            },
            // The pointer went onto a pane (the host claims it only over
            // bar and tab) or out of the host: nothing is hovered.
            Event::MouseOut | Event::MouseLeave => {
                if self.update_hover(None) {
                    crate::animation::request_draw();
                }
                BarTabResponse::Ignored
            }
            // A finger drag ends without a release; whatever it toggled
            // stays toggled.
            Event::MouseCaptureLost => {
                self.press = None;
                BarTabResponse::Ignored
            }
            _ => BarTabResponse::Ignored,
        }
    }

    /// A left press at `pos`.
    fn press_at(&mut self, pos: Point) -> BarTabResponse {
        let swallow = self.swallows(pos);
        // Only the press right after a tab click can be its double click.
        self.last_toggle = None;
        if swallow {
            self.press = Some(Press::Swallowed);
            return BarTabResponse::Consumed;
        }
        if self.on_tab(pos) {
            self.press = Some(Press::Tab {
                down: pos,
                toggled: false,
                touch: crate::touch_emulation::last_press_was_touch(),
            });
            return BarTabResponse::Consumed;
        }
        if !self.on_bar(pos) {
            return BarTabResponse::Ignored;
        }
        self.press = Some(Press::Bar);
        if crate::event::is_double_click() {
            self.state.toggle();
            return BarTabResponse::Toggled;
        }
        BarTabResponse::BeginResize
    }

    /// A pointer move to `pos`.
    fn move_to(&mut self, pos: Point) -> BarTabResponse {
        match self.press {
            Some(Press::Tab {
                down,
                toggled: false,
                touch: true,
            }) => {
                let d = if self.vertical {
                    pos.y - down.y
                } else {
                    pos.x - down.x
                };
                let travel = d * self.toward_view();
                if travel.abs() < DRAG_TO_TOGGLE {
                    return BarTabResponse::Consumed;
                }
                // Toward the view opens, toward the edge closes.
                self.state.set(travel < 0.0);
                self.press = Some(Press::Tab {
                    down,
                    toggled: true,
                    touch: true,
                });
                BarTabResponse::Toggled
            }
            Some(Press::Tab {
                down, touch: false, ..
            }) => {
                if (pos.x - down.x).hypot(pos.y - down.y) <= TAP_SLOP {
                    return BarTabResponse::Consumed;
                }
                // A mouse dragging the tab drags the bar.
                self.press = Some(Press::Bar);
                BarTabResponse::BeginResize
            }
            Some(Press::Tab { .. }) | Some(Press::Swallowed) => BarTabResponse::Consumed,
            Some(Press::Bar) | None => {
                if self.update_hover(Some(pos)) {
                    crate::animation::request_draw();
                }
                BarTabResponse::Ignored
            }
        }
    }
}

/// Draws a path given in the bar's frame (`s` across, `t` along).
struct PathBuilder<'a, 'c> {
    tab: &'a BarTab,
    ctx: &'c mut dyn DrawCtx,
}

impl PathBuilder<'_, '_> {
    fn move_to(&mut self, s: f64, t: f64) {
        let p = self.tab.to_local(s, t);
        self.ctx.move_to(p.x, p.y);
    }
    fn line_to(&mut self, s: f64, t: f64) {
        let p = self.tab.to_local(s, t);
        self.ctx.line_to(p.x, p.y);
    }
    fn quad_to(&mut self, cs: f64, ct: f64, s: f64, t: f64) {
        let c = self.tab.to_local(cs, ct);
        let p = self.tab.to_local(s, t);
        self.ctx.quad_to(c.x, c.y, p.x, p.y);
    }
}
