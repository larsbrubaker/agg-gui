//! Y-up menu geometry and hit testing.
//!
//! Popup menus are global overlays, so geometry is expressed in the owning
//! widget's local coordinate space but clamped against the current viewport.

use crate::geometry::{Point, Rect, Size};

use super::model::{MenuEntry, MenuItem};
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
const MARGIN: f64 = 4.0;

/// The concrete menu metrics for the current frame.  Desktop values are the
/// bare constants; on a touch device they grow so every interactive target
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
}

/// Effective menu metrics for the current frame.
///
/// When [`crate::input_profile::touch_ui_active`] is false these equal the
/// desktop constants EXACTLY (bit-identical — desktop behaviour is
/// unchanged).  When it is true, interactive heights are floored at
/// `TOUCH_MIN` *painted* px.
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
pub fn effective_metrics() -> MenuMetrics {
    let base = MenuMetrics {
        row_h: ROW_H,
        sep_h: SEP_H,
        bar_h: BAR_H,
        vertical_row_h: VERTICAL_ROW_H,
        menu_w: MENU_W,
        default_font_size: DEFAULT_FONT_SIZE,
    };
    if !crate::input_profile::touch_ui_active() {
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
        default_font_size: DEFAULT_FONT_SIZE * grow,
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
    // One source of truth for every popup dimension this frame — paint and
    // hit-test both flow through here, so they can never diverge.
    let m = effective_metrics();
    let mut layouts = Vec::new();
    let mut items = root_items;
    let mut x = anchor.x;
    let mut y_top = anchor.y;
    let mut prefix = Vec::new();

    loop {
        let rect = popup_rect(items, Point::new(x, y_top), anchor_kind, viewport, &m);
        let rows = row_layouts(items, rect, &m);
        layouts.push(PopupLayout {
            rect,
            rows,
            path_prefix: prefix.clone(),
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
        x = (rect.x + rect.width - 2.0).min(viewport.width - m.menu_w - MARGIN);
        // For top/context popups (open downward) the cascade
        // anchors at the row's TOP so the submenu's top edge
        // aligns with the parent row's top. For BottomBar popups
        // (open upward) the cascade anchors at the row's BOTTOM
        // so the submenu's bottom edge aligns with the parent
        // row's bottom — visually symmetric.
        y_top = match anchor_kind {
            MenuAnchorKind::BottomBar => row.rect.y,
            _ => row.rect.y + row.rect.height,
        };
    }

    layouts
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

pub fn popup_height(items: &[MenuEntry], m: &MenuMetrics) -> f64 {
    items
        .iter()
        .map(|entry| match entry {
            MenuEntry::Item(_) => m.row_h,
            MenuEntry::Separator => m.sep_h,
        })
        .sum::<f64>()
        .max(m.row_h)
}

pub fn contains(rect: Rect, pos: Point) -> bool {
    pos.x >= rect.x
        && pos.x <= rect.x + rect.width
        && pos.y >= rect.y
        && pos.y <= rect.y + rect.height
}

fn popup_rect(
    items: &[MenuEntry],
    anchor: Point,
    anchor_kind: MenuAnchorKind,
    viewport: Size,
    m: &MenuMetrics,
) -> Rect {
    let h = popup_height(items, m);
    let x = anchor
        .x
        .clamp(MARGIN, (viewport.width - m.menu_w - MARGIN).max(MARGIN));
    let (min_y, raw_y) = match anchor_kind {
        // Top bar — popup hangs below the anchor (extends toward
        // smaller y in Y-up). `Bar` allows negative-y clamp so a
        // bar pushed flush against the viewport edge still places
        // a popup correctly.
        MenuAnchorKind::Bar => (-viewport.height, anchor.y - h),
        // Bottom bar — popup rises ABOVE the anchor (extends
        // toward larger y in Y-up). Popup rect's bottom = anchor.y.
        MenuAnchorKind::BottomBar => (MARGIN, anchor.y),
        MenuAnchorKind::Context => (MARGIN, anchor.y - h),
    };
    let y = raw_y.clamp(min_y, (viewport.height - h - MARGIN).max(min_y));
    Rect::new(x, y, m.menu_w, h)
}

fn row_layouts(items: &[MenuEntry], rect: Rect, m: &MenuMetrics) -> Vec<RowLayout> {
    let mut y = rect.y + rect.height;
    let mut rows = Vec::with_capacity(items.len());
    for (idx, entry) in items.iter().enumerate() {
        let h = match entry {
            MenuEntry::Item(_) => m.row_h,
            MenuEntry::Separator => m.sep_h,
        };
        y -= h;
        rows.push(RowLayout {
            rect: Rect::new(rect.x, y, rect.width, h),
            item_index: matches!(entry, MenuEntry::Item(_)).then_some(idx),
        });
    }
    rows
}

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
