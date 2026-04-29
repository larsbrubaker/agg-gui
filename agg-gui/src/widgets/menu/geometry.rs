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
const MARGIN: f64 = 4.0;

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
    let mut layouts = Vec::new();
    let mut items = root_items;
    let mut x = anchor.x;
    let mut y_top = anchor.y;
    let mut prefix = Vec::new();

    loop {
        let rect = popup_rect(items, Point::new(x, y_top), anchor_kind, viewport);
        let rows = row_layouts(items, rect);
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
        x = (rect.x + rect.width - 2.0).min(viewport.width - MENU_W - MARGIN);
        y_top = row.rect.y + row.rect.height;
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

pub fn popup_height(items: &[MenuEntry]) -> f64 {
    items
        .iter()
        .map(|entry| match entry {
            MenuEntry::Item(_) => ROW_H,
            MenuEntry::Separator => SEP_H,
        })
        .sum::<f64>()
        .max(ROW_H)
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
) -> Rect {
    let h = popup_height(items);
    let x = anchor
        .x
        .clamp(MARGIN, (viewport.width - MENU_W - MARGIN).max(MARGIN));
    let min_y = if anchor_kind == MenuAnchorKind::Bar {
        -viewport.height
    } else {
        MARGIN
    };
    let y = (anchor.y - h).clamp(min_y, (viewport.height - h - MARGIN).max(min_y));
    Rect::new(x, y, MENU_W, h)
}

fn row_layouts(items: &[MenuEntry], rect: Rect) -> Vec<RowLayout> {
    let mut y = rect.y + rect.height;
    let mut rows = Vec::with_capacity(items.len());
    for (idx, entry) in items.iter().enumerate() {
        let h = match entry {
            MenuEntry::Item(_) => ROW_H,
            MenuEntry::Separator => SEP_H,
        };
        y -= h;
        rows.push(RowLayout {
            rect: Rect::new(rect.x, y, rect.width, h),
            item_index: matches!(entry, MenuEntry::Item(_)).then_some(idx),
        });
    }
    rows
}
