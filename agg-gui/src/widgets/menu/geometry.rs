//! Y-up menu geometry and hit testing.
//!
//! Popup menus are global overlays, so geometry is expressed in the owning
//! widget's local coordinate space but clamped against the on-screen part of
//! the viewport, [`visible_root_rect`](crate::widget::visible_root_rect) —
//! `(0, −lift, w, h)` while the on-screen keyboard lifts the tree. The clamp
//! is exact for menus kept in root logical (unlifted) coordinates: a
//! [`PopupMenu`](super::PopupMenu) hosted through its root origin
//! (`widget/popup_local.rs`), or a host sitting at the root origin.
//! Popup widths come from [`stack_layout_with_width`]'s width callback;
//! [`stack_layout`] uses the fixed (touch-grown) `menu_w`.

use crate::geometry::{Point, Rect, Size};

use super::model::{MenuEntry, MenuItem};
use super::overflow::{
    current_scroll_gutter, place_hanging, place_vertical, PopupScroll, StackPlacement,
};
use super::state::MenuAnchorKind;

pub const ROW_H: f64 = 24.0;
pub const SEP_H: f64 = 7.0;
pub const MENU_W: f64 = 224.0;
pub const BAR_H: f64 = 26.0;
/// Row height for a [`super::MenuOrientation::Vertical`] bar (a tall,
/// narrow sidebar of top-level buttons).  Lives here with the other menu
/// metric constants so [`effective_metrics`] can grow it on touch.
pub const VERTICAL_ROW_H: f64 = 36.0;
/// Default popup / bar text size before any touch growth or explicit
/// [`super::MenuBar::with_font_size`] override.
pub const DEFAULT_FONT_SIZE: f64 = 14.0;
/// Minimum *painted* size (in physical CSS px at `ux_scale == 1.0`) for any
/// interactive menu target on a touch device.  44 px is the long-standing
/// Apple HIG / Material touch-target floor.
pub const TOUCH_MIN: f64 = 44.0;
/// Default gap every popup keeps from the window edge (agg-sharp sits
/// flush); a [`super::MenuStyle::window_margin`] replaces it.
pub(crate) const MARGIN: f64 = 4.0;

/// The concrete menu metrics for the current frame.  Desktop values are the
/// bare constants; sized for touch they grow so every interactive target
/// clears [`TOUCH_MIN`] painted px.  Hit-testing and painting BOTH derive
/// from this single struct (via [`stack_layout`] / the bar layout), so a
/// grown popup can never have paint and hit rects disagree.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MenuMetrics {
    pub row_h: f64,
    pub sep_h: f64,
    pub bar_h: f64,
    pub vertical_row_h: f64,
    pub menu_w: f64,
    pub default_font_size: f64,
    /// Gap every popup keeps from the window edge ([`MARGIN`] unless a
    /// [`super::MenuStyle::window_margin`] replaces it; `0.0` is flush).
    pub window_margin: f64,
    /// The height (logical px) every item row is floored at: the touch
    /// minimum when these metrics are sized for a finger, `0.0` (no floor)
    /// when sized for the mouse.  Rows with their own height
    /// ([`MenuItem::row_height`]) are floored at it too.
    pub touch_row_min: f64,
}

/// Effective menu metrics for the current frame, for the chrome that stays
/// on screen (the menu bar's height, its titles' text size): sized for touch
/// once [`crate::input_profile::touch_ui_active`] holds (a phone or tablet
/// profile, or any touch this session), since the next press on the bar may
/// be a finger's.  A popup sizes itself by the press that opened it instead
/// ([`popup_touch_sized`], [`metrics_for`]).
pub fn effective_metrics() -> MenuMetrics {
    metrics_for(crate::input_profile::touch_ui_active())
}

/// Whether a popup menu opening now is sized for a finger: always under a
/// phone or tablet input profile, otherwise when the press that opened it
/// was a finger's (a tap or a long press,
/// [`crate::touch_emulation::last_press_was_touch`]).  A menu the mouse
/// opens gets the style's own rows every time, even on a touchscreen laptop
/// that was touched a moment ago.  [`super::PopupMenuState`] asks once, as it
/// opens, and its submenus keep that answer.
pub fn popup_touch_sized() -> bool {
    crate::input_profile::is_mobile_touch() || crate::touch_emulation::last_press_was_touch()
}

/// Menu metrics sized for the mouse (`touch == false`) or for a finger.
///
/// Sized for the mouse these equal the desktop constants EXACTLY.  Sized
/// for a finger, interactive heights are floored at `TOUCH_MIN` *painted*
/// px.
///
/// The paint transform multiplies logical px by
/// `effective_scale = device_scale * ux_scale`, and `device_scale` maps
/// logical→physical so that at `ux_scale == 1.0` one logical px is one CSS
/// px.  To guarantee `>= TOUCH_MIN` painted px the requirement in logical
/// units is `TOUCH_MIN / ux_scale`.  This self-normalises against `ux_scale`:
/// an app that already set `ux_scale >= TOUCH_MIN / BAR_H` (≈1.7) gets
/// `TOUCH_MIN / 1.7 ≈ 25.9 < 26`, so `max()` is a no-op and nothing is
/// double-scaled.  Width and default font size grow in lock-step with the
/// row so the popup keeps its desktop proportions, just larger; separators
/// stay thin because they are not tappable.
pub fn metrics_for(touch: bool) -> MenuMetrics {
    let base = MenuMetrics {
        row_h: ROW_H,
        sep_h: SEP_H,
        bar_h: BAR_H,
        vertical_row_h: VERTICAL_ROW_H,
        menu_w: MENU_W,
        default_font_size: crate::font_settings::default_font_size_or(DEFAULT_FONT_SIZE),
        window_margin: MARGIN,
        touch_row_min: 0.0,
    };
    if !touch {
        return base;
    }
    // `set_ux_scale` only debug-asserts positivity, so a release-mode
    // caller can leave `ux_scale()` at `0.0`. Dividing by that would yield
    // `+inf` metrics on this hot layout path (an infinitely tall menu bar).
    // Clamp the divisor so a misbehaving shell degrades to large-but-finite
    // geometry instead of poisoning layout.
    let min_logical = TOUCH_MIN / crate::ux_scale::ux_scale().max(0.01);
    let row_h = ROW_H.max(min_logical);
    let bar_h = BAR_H.max(min_logical);
    let vertical_row_h = VERTICAL_ROW_H.max(min_logical);
    let grow = row_h / ROW_H;
    MenuMetrics {
        row_h,
        sep_h: SEP_H,
        bar_h,
        vertical_row_h,
        menu_w: MENU_W * grow,
        default_font_size: base.default_font_size * grow,
        window_margin: MARGIN,
        touch_row_min: min_logical,
    }
}

/// Current effective menu-bar height (logical px).  Host chrome that
/// reserves vertical space for a top bar should call this **per layout**,
/// not cache it at startup: the touch latch inside
/// [`crate::input_profile::touch_ui_active`] can flip at runtime after the
/// first touch, growing the bar from [`BAR_H`] to the touch minimum.
pub fn menu_bar_height() -> f64 {
    effective_metrics().bar_h
}

/// Current effective row height for a vertical (sidebar) menu bar (logical
/// px).  Query per layout for the same runtime-latch reason as
/// [`menu_bar_height`].
pub fn menu_vertical_row_height() -> f64 {
    effective_metrics().vertical_row_h
}

#[derive(Clone, Debug)]
pub struct PopupLayout {
    pub rect: Rect,
    pub rows: Vec<RowLayout>,
    pub path_prefix: Vec<usize>,
    /// Set when the panel is too tall for its room and scrolls; see
    /// [`super::overflow`].  Its rows then extend past `rect` and are
    /// clipped to it.
    pub scroll: Option<PopupScroll>,
}

#[derive(Clone, Debug)]
pub struct RowLayout {
    pub rect: Rect,
    pub item_index: Option<usize>,
}

#[derive(Clone, Debug)]
pub enum MenuHit {
    Item(Vec<usize>),
    Panel,
}

pub fn stack_layout(
    root_items: &[MenuEntry],
    anchor: Point,
    anchor_kind: MenuAnchorKind,
    open_path: &[usize],
    viewport: Size,
) -> Vec<PopupLayout> {
    stack_layout_with_width(
        root_items,
        anchor,
        anchor_kind,
        open_path,
        viewport,
        &|_, m: &MenuMetrics| m.menu_w,
    )
}

/// [`stack_layout`] with a caller-chosen width per popup level: `width` gets
/// the level's entries and this frame's metrics and returns that panel's
/// width (see [`super::fit_width`] for the content-fitted policy).
pub fn stack_layout_with_width(
    root_items: &[MenuEntry],
    anchor: Point,
    anchor_kind: MenuAnchorKind,
    open_path: &[usize],
    viewport: Size,
    width: &dyn Fn(&[MenuEntry], &MenuMetrics) -> f64,
) -> Vec<PopupLayout> {
    stack_layout_with_metrics(
        root_items,
        anchor,
        anchor_kind,
        open_path,
        viewport,
        width,
        effective_metrics(),
    )
}

/// The metrics a popup opening now gets ([`popup_touch_sized`]) with the
/// item row height replaced by `row_h`; see [`metrics_with_row_h_for`].
pub fn metrics_with_row_h(row_h: f64) -> MenuMetrics {
    metrics_with_row_h_for(popup_touch_sized(), row_h)
}

/// [`metrics_for`] `touch` with the item row height replaced by `row_h`
/// (desktop logical px, a [`super::MenuStyle::row_h`]).  Sized for touch the
/// row is still floored at the touch minimum.  `row_h == ROW_H` returns
/// [`metrics_for`] unchanged.
pub fn metrics_with_row_h_for(touch: bool, row_h: f64) -> MenuMetrics {
    let mut m = metrics_for(touch);
    if row_h == ROW_H || !row_h.is_finite() || row_h <= 0.0 {
        return m;
    }
    m.row_h = row_h.max(m.touch_row_min);
    m
}

/// The metrics a popup opening now gets ([`popup_touch_sized`]) under a
/// style's row and separator heights and window margin; see
/// [`metrics_with_style_for`].
pub fn metrics_with_style(row_h: f64, sep_h: f64, window_margin: f64) -> MenuMetrics {
    metrics_with_style_for(popup_touch_sized(), row_h, sep_h, window_margin)
}

/// [`metrics_with_row_h_for`] with the separator row height replaced by
/// `sep_h` and the window margin by `window_margin` (a [`super::MenuStyle`]'s
/// [`super::MenuSeparatorStyle::row_height`] and
/// [`super::MenuStyle::window_margin`]).  Separators are not tappable, so
/// `sep_h` is not touch-floored.  Non-finite or negative values keep the
/// defaults.
pub fn metrics_with_style_for(
    touch: bool,
    row_h: f64,
    sep_h: f64,
    window_margin: f64,
) -> MenuMetrics {
    let mut m = metrics_with_row_h_for(touch, row_h);
    if sep_h.is_finite() && sep_h >= 0.0 {
        m.sep_h = sep_h;
    }
    if window_margin.is_finite() && window_margin >= 0.0 {
        m.window_margin = window_margin;
    }
    m
}

/// [`stack_layout_with_width`] with caller-supplied metrics `m` (see
/// [`metrics_with_row_h`]) instead of this frame's [`effective_metrics`].
pub fn stack_layout_with_metrics(
    root_items: &[MenuEntry],
    anchor: Point,
    anchor_kind: MenuAnchorKind,
    open_path: &[usize],
    viewport: Size,
    width: &dyn Fn(&[MenuEntry], &MenuMetrics) -> f64,
    m: MenuMetrics,
) -> Vec<PopupLayout> {
    stack_layout_placed(
        root_items,
        anchor,
        anchor_kind,
        open_path,
        viewport,
        width,
        m,
        &StackPlacement::unscrolled(),
    )
}

/// [`stack_layout_with_metrics`] with the opener's span, the menu space's
/// root origin and each level's scroll offset (see [`StackPlacement`]).
#[allow(clippy::too_many_arguments)]
pub(crate) fn stack_layout_placed(
    root_items: &[MenuEntry],
    anchor: Point,
    anchor_kind: MenuAnchorKind,
    open_path: &[usize],
    viewport: Size,
    width: &dyn Fn(&[MenuEntry], &MenuMetrics) -> f64,
    m: MenuMetrics,
    placement: &StackPlacement<'_>,
) -> Vec<PopupLayout> {
    // One source of truth for every popup dimension this frame — paint and
    // hit-test both flow through here, so they can never diverge.
    let mut layouts = Vec::new();
    let mut items = root_items;
    let root_vis = crate::widget::visible_root_rect(viewport);
    // The window in the menu's own space, when that space's root origin is
    // known (a `MenuBar` runs its popup bar-local).
    let vis = match placement.space_origin {
        Some(o) => Rect::new(
            root_vis.x - o.x,
            root_vis.y - o.y,
            root_vis.width,
            root_vis.height,
        ),
        None => root_vis,
    };
    let bar_legacy = placement.space_origin.is_none();
    let mut x = anchor.x;
    let mut y_top = anchor.y;
    let mut prefix = Vec::new();
    // Height of the row this level hangs from, when it fits beside its
    // parent on neither side (see the end of the loop).
    let mut hang_span: Option<f64> = None;

    loop {
        let w = width(items, &m);
        let content_h = popup_height(items, &m);
        let (y, h) = match hang_span {
            // The legacy bar placement has no window in the menu's space to
            // place against; it keeps hanging unclamped.
            Some(span) if !(bar_legacy && anchor_kind == MenuAnchorKind::Bar) => {
                place_hanging(content_h, y_top, anchor_kind, vis, span, m.window_margin)
            }
            _ => place_vertical(
                content_h,
                y_top,
                anchor_kind,
                vis,
                prefix.is_empty(),
                placement.anchor_span,
                bar_legacy,
                m.window_margin,
            ),
        };
        let scrolls = h < content_h - 1e-9;
        let gutter = if scrolls {
            current_scroll_gutter()
        } else {
            0.0
        };
        let margin = m.window_margin;
        // No panel is wider than the window less its margins, scroll bar
        // strip included: a touch-grown fixed width (about 357 logical px)
        // overran a phone's 358 px window.  The rows narrow with it and
        // their labels elide before the shortcut / chevron column
        // (`widget/labels.rs`).
        let w = w.min(max_panel_w(vis, margin) - gutter).max(0.0);
        let left = vis.x + margin;
        let panel_w = w + gutter;
        let px = x.clamp(left, (vis.x + vis.width - panel_w - margin).max(left));
        let rect = Rect::new(px, y, panel_w, h);
        let scroll = scrolls.then(|| {
            let mut s = PopupScroll {
                content_height: content_h,
                offset: 0.0,
                bar: Rect::new(px + w, y, gutter, h),
            };
            s.offset = (placement.scroll)(&prefix).clamp(0.0, s.max_offset(h));
            s
        });
        let offset = scroll.map_or(0.0, |s| s.offset);
        let rows = row_layouts(items, Rect::new(px, y + offset, w, h), &m);
        layouts.push(PopupLayout {
            rect,
            rows,
            path_prefix: prefix.clone(),
            scroll,
        });

        let Some(next_idx) = open_path.get(prefix.len()).copied() else {
            break;
        };
        let Some(item) = item_at(items, next_idx) else {
            break;
        };
        if item.submenu.is_empty() {
            break;
        }
        let Some(row) = layouts.last().and_then(|layout| {
            layout
                .rows
                .iter()
                .find(|row| row.item_index == Some(next_idx))
                .cloned()
        }) else {
            break;
        };
        prefix.push(next_idx);
        items = &item.submenu;
        let sub_w = width(items, &m).min(max_panel_w(vis, margin));
        let cascade_x = rect.x + rect.width - 2.0;
        let fits_right = cascade_x + sub_w <= vis.x + vis.width - margin;
        let fits_left = rect.x - sub_w >= vis.x + margin;
        // agg-sharp `PopupMenu` (its `AltMate`): a submenu with no room
        // to the right of its parent opens to its left, overlapping the
        // parent's left edge by the same 2 px the right cascade does,
        // rather than being clamped back over the parent.
        x = if !fits_right && fits_left {
            rect.x - sub_w + 2.0
        } else {
            cascade_x.min(vis.x + vis.width - sub_w - margin)
        };
        // A row scrolled partly out of its panel cascades from the part
        // that shows.
        let row_rect = clip_to(row.rect, rect);
        // For top/context popups (open downward) the cascade
        // anchors at the row's TOP so the submenu's top edge
        // aligns with the parent row's top. For BottomBar popups
        // (open upward) the cascade anchors at the row's BOTTOM
        // so the submenu's bottom edge aligns with the parent
        // row's bottom — visually symmetric.
        y_top = match anchor_kind {
            MenuAnchorKind::BottomBar => row_rect.y,
            _ => row_rect.y + row_rect.height,
        };
        // On a phone-width viewport the submenu fits on neither side of
        // its parent, and clamping it back on screen would lay it right
        // over the row that opened it (agg-sharp
        // `NarrowSubMenuPlacementTests`). Hang it from the row instead:
        // below it when the menu opens downward, above it when upward, and
        // flipped or scrolled rather than slid back over the row when it
        // does not fit there (`overflow.rs` `place_hanging`): a tap on the
        // row is how a finger closes the submenu again.
        hang_span = None;
        if !fits_right && !fits_left {
            y_top = match anchor_kind {
                MenuAnchorKind::BottomBar => row_rect.y + row_rect.height,
                _ => row_rect.y,
            };
            hang_span = Some(row_rect.height);
        }
    }

    layouts
}

/// The widest a popup panel may be in the window `vis`: its width less a
/// `margin` on each side.  Unbounded while the window is not known yet (a
/// zero or non-finite viewport), so a panel laid out before the first
/// frame keeps its natural width instead of collapsing.
fn max_panel_w(vis: Rect, margin: f64) -> f64 {
    if vis.width > 0.0 && vis.width.is_finite() {
        (vis.width - 2.0 * margin).max(0.0)
    } else {
        f64::INFINITY
    }
}

/// `r` cut vertically to `panel` (unchanged when it lies inside).
fn clip_to(r: Rect, panel: Rect) -> Rect {
    let bottom = r.y.max(panel.y);
    let top = (r.y + r.height).min(panel.y + panel.height).max(bottom);
    Rect::new(r.x, bottom, r.width, top - bottom)
}

pub fn hit_test(layouts: &[PopupLayout], pos: Point) -> Option<MenuHit> {
    for layout in layouts.iter().rev() {
        if !contains(layout.rect, pos) {
            continue;
        }
        for row in &layout.rows {
            if contains(row.rect, pos) {
                if let Some(idx) = row.item_index {
                    let mut path = layout.path_prefix.clone();
                    path.push(idx);
                    return Some(MenuHit::Item(path));
                }
                return Some(MenuHit::Panel);
            }
        }
        return Some(MenuHit::Panel);
    }
    None
}

pub fn item_at_path<'a>(items: &'a [MenuEntry], path: &[usize]) -> Option<&'a MenuItem> {
    let mut current = items;
    let mut item = None;
    for &idx in path {
        item = item_at(current, idx);
        current = &item?.submenu;
    }
    item
}

pub fn item_at(items: &[MenuEntry], idx: usize) -> Option<&MenuItem> {
    match items.get(idx)? {
        MenuEntry::Item(item) => Some(item),
        MenuEntry::Separator => None,
    }
}

/// Height of one entry's row: separators are thin, widget rows take their
/// own height, an item with a [`MenuItem::row_height`] takes that (floored
/// at the metrics' [`MenuMetrics::touch_row_min`], as the style's row is),
/// every other item takes the (possibly touch-grown) row height.
pub fn entry_height(entry: &MenuEntry, m: &MenuMetrics) -> f64 {
    match entry {
        MenuEntry::Item(item) => match (item.widget_row, item.row_height) {
            (Some(w), _) => w.height,
            (None, Some(h)) if h.is_finite() && h > 0.0 => h.max(m.touch_row_min),
            _ => m.row_h,
        },
        MenuEntry::Separator => m.sep_h,
    }
}

pub fn popup_height(items: &[MenuEntry], m: &MenuMetrics) -> f64 {
    items
        .iter()
        .map(|entry| entry_height(entry, m))
        .sum::<f64>()
        .max(m.row_h)
}

pub fn contains(rect: Rect, pos: Point) -> bool {
    pos.x >= rect.x
        && pos.x <= rect.x + rect.width
        && pos.y >= rect.y
        && pos.y <= rect.y + rect.height
}

fn row_layouts(items: &[MenuEntry], rect: Rect, m: &MenuMetrics) -> Vec<RowLayout> {
    let mut y = rect.y + rect.height;
    let mut rows = Vec::with_capacity(items.len());
    for (idx, entry) in items.iter().enumerate() {
        let h = entry_height(entry, m);
        y -= h;
        rows.push(RowLayout {
            rect: Rect::new(rect.x, y, rect.width, h),
            item_index: matches!(entry, MenuEntry::Item(_)).then_some(idx),
        });
    }
    rows
}

#[cfg(test)]
#[path = "geometry_narrow_tests.rs"]
mod narrow_tests;

#[cfg(test)]
mod metrics_tests {
    use super::*;
    use crate::input_profile::{set_input_profile, touch_ui_active, InputProfile};

    /// Return every global this module reads to a known desktop baseline.
    /// The input profile is a process-wide atomic other test files write,
    /// so pin it explicitly; the touch latch and `ux_scale` are thread-local.
    fn desktop() {
        set_input_profile(InputProfile::Desktop);
        crate::touch_state::clear_last_touch_event_for_testing();
        crate::ux_scale::set_ux_scale(1.0);
    }

    /// Activate touch sizing purely via the thread-local latch (a real
    /// touch event) with the profile LEFT desktop — proving the runtime
    /// fallback works even when the shell never called `set_input_profile`.
    fn touch_via_latch(ux: f64) {
        set_input_profile(InputProfile::Desktop);
        crate::touch_state::clear_last_touch_event_for_testing();
        crate::touch_state::note_touch_event();
        crate::ux_scale::set_ux_scale(ux);
        assert!(touch_ui_active(), "latch must activate touch sizing");
    }

    #[test]
    fn desktop_metrics_are_the_bare_constants() {
        let _guard = crate::input_profile::profile_test_lock();
        desktop();
        let m = effective_metrics();
        assert_eq!(m.row_h, ROW_H);
        assert_eq!(m.sep_h, SEP_H);
        assert_eq!(m.bar_h, BAR_H);
        assert_eq!(m.vertical_row_h, VERTICAL_ROW_H);
        assert_eq!(m.menu_w, MENU_W);
        assert_eq!(m.default_font_size, DEFAULT_FONT_SIZE);
        assert_eq!(menu_bar_height(), BAR_H);
        assert_eq!(menu_vertical_row_height(), VERTICAL_ROW_H);
        desktop();
    }

    #[test]
    fn touch_at_ux_1_floors_interactive_targets_at_44() {
        let _guard = crate::input_profile::profile_test_lock();
        touch_via_latch(1.0);
        let m = effective_metrics();
        // At ux 1.0 logical px == painted px, so every interactive metric
        // must equal the 44 px touch minimum.
        assert_eq!(m.row_h, TOUCH_MIN);
        assert_eq!(m.bar_h, TOUCH_MIN);
        assert_eq!(m.vertical_row_h, TOUCH_MIN);
        assert_eq!(menu_bar_height(), TOUCH_MIN);
        assert_eq!(menu_vertical_row_height(), TOUCH_MIN);
        // Width + font grow in lock-step with the row; separators stay thin.
        let grow = TOUCH_MIN / ROW_H;
        assert!((m.menu_w - MENU_W * grow).abs() < 1e-9);
        assert!((m.default_font_size - DEFAULT_FONT_SIZE * grow).abs() < 1e-9);
        assert_eq!(m.sep_h, SEP_H);
        desktop();
    }

    #[test]
    fn popup_row_hit_rects_are_44_tall_on_touch() {
        let _guard = crate::input_profile::profile_test_lock();
        // The row rects `stack_layout` produces feed BOTH hit-testing and
        // painting, so verifying the interactive rows are 44 tall here
        // proves the touch target the user presses is the one drawn.
        touch_via_latch(1.0);
        let items = vec![
            MenuEntry::Item(MenuItem::action("A", "a")),
            MenuEntry::Separator,
            MenuEntry::Item(MenuItem::action("B", "b")),
        ];
        let layouts = stack_layout(
            &items,
            Point::new(20.0, 400.0),
            MenuAnchorKind::Context,
            &[],
            Size::new(500.0, 500.0),
        );
        let rows = &layouts[0].rows;
        assert_eq!(rows[0].rect.height, TOUCH_MIN, "item row must be 44 tall");
        assert_eq!(rows[1].rect.height, SEP_H, "separator stays thin");
        assert_eq!(rows[2].rect.height, TOUCH_MIN, "item row must be 44 tall");
        assert_eq!(layouts[0].rect.width, MENU_W * (TOUCH_MIN / ROW_H));
        desktop();
    }

    #[test]
    fn touch_self_normalizes_against_ux_scale() {
        let _guard = crate::input_profile::profile_test_lock();
        // An app that already scaled the whole UI past the point where the
        // base metric paints >= 44 px must NOT be grown further (no double
        // scaling).  At ux 1.7 the 26 px bar paints 44.2 px, so `max()` is a
        // no-op for the bar; the 36 px vertical row likewise.  The 24 px row
        // paints only 40.8 px at ux 1.7, so it is floored to 44/1.7 (painting
        // exactly 44) — still the floor, never base*ux.
        touch_via_latch(1.7);
        let m = effective_metrics();
        assert_eq!(m.bar_h, BAR_H, "26px bar already clears 44 painted at 1.7");
        assert_eq!(m.vertical_row_h, VERTICAL_ROW_H);
        assert!((m.row_h - TOUCH_MIN / 1.7).abs() < 1e-9);
        assert!(
            (m.row_h * 1.7 - TOUCH_MIN).abs() < 1e-9,
            "floored row paints exactly the touch minimum"
        );

        // At ux large enough that every base already clears 44 painted
        // (>= 44/24 ≈ 1.833), the whole struct is the desktop constants —
        // the self-normalization no-op in full.
        touch_via_latch(2.0);
        let m = effective_metrics();
        assert_eq!(m.row_h, ROW_H);
        assert_eq!(m.bar_h, BAR_H);
        assert_eq!(m.vertical_row_h, VERTICAL_ROW_H);
        assert_eq!(m.menu_w, MENU_W);
        assert_eq!(m.default_font_size, DEFAULT_FONT_SIZE);
        desktop();
    }

    #[test]
    fn effective_metrics_stay_finite_when_ux_scale_is_zero() {
        // `set_ux_scale` only debug-asserts positivity, so a release-mode
        // shell can leave `ux_scale()` at 0.0.  Force that degenerate value
        // past the assert via the raw test setter and confirm the divisor
        // clamp keeps every metric large-but-finite rather than +inf.
        let _guard = crate::input_profile::profile_test_lock();
        touch_via_latch(1.0); // latch touch sizing so the divisor path runs
        crate::ux_scale::set_ux_scale_raw_for_test(0.0);
        let m = effective_metrics();
        assert!(
            m.row_h.is_finite(),
            "row_h must stay finite, got {}",
            m.row_h
        );
        assert!(
            m.bar_h.is_finite(),
            "bar_h must stay finite, got {}",
            m.bar_h
        );
        assert!(
            m.vertical_row_h.is_finite(),
            "vertical_row_h must stay finite, got {}",
            m.vertical_row_h
        );
        assert!(
            m.menu_w.is_finite(),
            "menu_w must stay finite, got {}",
            m.menu_w
        );
        assert!(
            m.default_font_size.is_finite(),
            "default_font_size must stay finite, got {}",
            m.default_font_size
        );
        desktop();
    }
}
