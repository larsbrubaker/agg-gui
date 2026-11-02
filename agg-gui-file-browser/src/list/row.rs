//! The row view: [`BrowserRow`], one row of the list (a 50-unit picture, then the name, on the
//! style's thumbnail background, a selected row on its selection fill, the box's corners rounded
//! by the style's corner radius), and [`RowStack`], the rows top down. A name too long for the
//! room between the picture and the menu button ends in "..." (a file's keeping its extension,
//! `fit_name`) and shows whole as the row's tooltip; an item with a detail line
//! (`Item::detail_line`) shows it smaller and fainter under the name. A hovered row draws the
//! menu button's glyph at its right end (`list/menu_button.rs`), and whatever the app lays over
//! the picture (`ItemPictures::tile_overlay`) is the row's child, over the picture. Pointer
//! handling is `list/item_event.rs`.

use agg_gui::{DrawCtx, Event, EventResult, Point, Rect, Size, Widget};

use crate::fitted_text::{centered_text_baseline, fill_text_within, fit_name, fit_text};

use super::build::{ListCx, RowParts};
use super::item_event::item_event;
use super::menu_button::{menu_glyph_rect, shows_menu_button, MENU_GLYPH_SIZE};
use super::tile_art::{paint_art, TileArt};

/// The picture's side in logical units.
pub const ROW_THUMB_SIZE: i32 = 50;
/// The gap left and right of a row's box, and above it.
const ROW_MARGIN: f64 = 6.0;
/// A row's height: the picture and the margin above it.
pub const ROW_HEIGHT: f64 = ROW_THUMB_SIZE as f64 + ROW_MARGIN;
/// A detail line's size against the name's, and its opacity: read second, after the name.
const DETAIL_SCALE: f64 = 0.85;
const DETAIL_ALPHA: f32 = 0.7;
/// The name's margin after the picture.
const NAME_MARGIN: f64 = 10.0;

/// The room the name has in a row `width` wide: from its margin after the picture to its margin
/// before the menu glyph, so a long name never runs under it.
fn name_room(width: f64) -> f64 {
    let item_width = (width - 2.0 * ROW_MARGIN).max(0.0);
    item_width - f64::from(ROW_THUMB_SIZE) - NAME_MARGIN * 2.0 - MENU_GLYPH_SIZE
}

/// One row of the list (see the module header).
pub struct BrowserRow {
    cx: ListCx,
    parts: RowParts,
    index: usize,
    name: String,
    /// The name as it fits, fitted when the row is given its width.
    shown_name: String,
    detail: Option<String>,
    shown_detail: String,
    hovered: bool,
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl BrowserRow {
    /// Listed row `index` of `cx`'s browser; `None` past the listing's end.
    pub fn new(cx: &ListCx, index: usize) -> Option<Self> {
        let mut parts = cx.parts(index, ROW_THUMB_SIZE)?;
        let name = parts.entry.name().to_string();
        let detail = parts.entry.detail_line();
        let children = parts.overlay.take().into_iter().collect();
        Some(Self {
            cx: cx.clone(),
            hovered: parts.hovered,
            index,
            shown_name: name.clone(),
            name,
            shown_detail: detail.clone().unwrap_or_default(),
            detail,
            parts,
            bounds: Rect::default(),
            children,
        })
    }

    /// The row's index in the listing it was built from.
    pub fn index(&self) -> usize {
        self.index
    }

    /// What the row shows where its picture goes.
    pub fn art(&self) -> &TileArt {
        &self.parts.art
    }

    /// The row's own box (its margins left out).
    fn item_size(&self) -> Size {
        Size::new(
            (self.bounds.width - 2.0 * ROW_MARGIN).max(0.0),
            f64::from(ROW_THUMB_SIZE),
        )
    }
}

impl Widget for BrowserRow {
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
    fn type_name(&self) -> &'static str {
        "BrowserRow"
    }
    fn id(&self) -> Option<&str> {
        Some(&self.parts.id)
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    /// The name is fitted to the width the row is given.
    fn set_bounds(&mut self, bounds: Rect) {
        let refit = bounds.width != self.bounds.width;
        self.bounds = bounds;
        if !refit {
            return;
        }
        let (font, size) = (&self.cx.font, self.cx.style.font_size);
        self.shown_name = fit_name(font, &self.name, size, name_room(bounds.width));
        if let Some(detail) = &self.detail {
            self.shown_detail =
                fit_text(font, detail, size * DETAIL_SCALE, name_room(bounds.width));
        }
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }
    fn layout(&mut self, available: Size) -> Size {
        let thumb = f64::from(ROW_THUMB_SIZE);
        let picture = Rect::new(ROW_MARGIN, 0.0, thumb, thumb);
        for child in &mut self.children {
            child.layout(Size::new(thumb, thumb));
            child.set_bounds(picture);
        }
        Size::new(available.width, ROW_HEIGHT)
    }
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let style = &self.cx.style;
        let thumb = f64::from(ROW_THUMB_SIZE);
        let item = self.item_size();
        ctx.set_fill_color(match self.parts.selected {
            true => style.selection_fill,
            false => style.thumbnail_background,
        });
        ctx.begin_path();
        ctx.rounded_rect(ROW_MARGIN, 0.0, item.width, thumb, style.corner_radius);
        ctx.fill();
        self.parts
            .art
            .paint(ctx, Rect::new(ROW_MARGIN, 0.0, thumb, thumb));
        let font = &self.cx.font;
        ctx.set_fill_color(style.text_color);
        ctx.set_font(std::sync::Arc::clone(font));
        ctx.set_font_size(style.font_size);
        // The name beside the picture, centred in its height; with a detail line the two are
        // centred together, the detail smaller and fainter under the name.
        let centered = centered_text_baseline(font, style.font_size, thumb);
        let text_x = ROW_MARGIN + thumb + NAME_MARGIN;
        let room = name_room(self.bounds.width);
        let detail_size = style.font_size * DETAIL_SCALE;
        let name_baseline = match self.detail {
            Some(_) => centered + detail_size * 0.6,
            None => centered,
        };
        fill_text_within(ctx, &self.shown_name, text_x, name_baseline, room, thumb);
        if self.detail.is_some() {
            ctx.set_fill_color(style.text_color.with_alpha(DETAIL_ALPHA));
            ctx.set_font_size(detail_size);
            let baseline = centered - style.font_size * 0.7;
            fill_text_within(ctx, &self.shown_detail, text_x, baseline, room, thumb);
        }
        if self.hovered && shows_menu_button(item.width) {
            let r = menu_glyph_rect(item.width, item.height);
            let glyph = Rect::new(ROW_MARGIN + r.x, r.y, r.width, r.height);
            paint_art(ctx, &self.cx.overflow, glyph);
        }
    }
    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseEnter => self.hovered = true,
            Event::MouseLeave => self.hovered = false,
            _ => {}
        }
        let size = self.item_size();
        item_event(
            event,
            &self.cx.state,
            self.index,
            Point::new(ROW_MARGIN, 0.0),
            size,
        )
    }
    /// The whole name while it is cut short, so the user can still read it.
    fn tooltip_text(&self) -> Option<&str> {
        (self.shown_name != self.name).then_some(self.name.as_str())
    }
    fn properties(&self) -> Vec<(&'static str, String)> {
        vec![
            ("name", self.name.clone()),
            ("shown_name", self.shown_name.clone()),
            ("detail", self.detail.clone().unwrap_or_default()),
            ("selected", self.parts.selected.to_string()),
            ("is_container", self.parts.entry.is_container().to_string()),
            ("picture", self.parts.art.kind.to_string()),
            ("menu_glyph", self.hovered.to_string()),
        ]
    }
}

/// The rows top down, as tall as they are so a `ScrollView` knows how far to scroll.
pub struct RowStack {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    id: String,
}

impl RowStack {
    /// A row for each listed row of `cx`'s browser.
    pub fn for_state(cx: &ListCx) -> Self {
        let slots = cx.state.borrow().list_slots();
        let children = slots
            .iter()
            .filter_map(|slot| match slot {
                crate::state::ListSlot::Entry(index) => BrowserRow::new(cx, *index),
                _ => None,
            })
            .map(|row| Box::new(row) as Box<dyn Widget>)
            .collect();
        Self {
            bounds: Rect::default(),
            children,
            id: cx.look.names().rows.clone(),
        }
    }
}

impl Widget for RowStack {
    fn type_name(&self) -> &'static str {
        "RowStack"
    }
    fn id(&self) -> Option<&str> {
        Some(&self.id)
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }
    fn layout(&mut self, available: Size) -> Size {
        let sizes: Vec<Size> = self
            .children
            .iter_mut()
            .map(|row| row.layout(Size::new(available.width, ROW_HEIGHT)))
            .collect();
        let height: f64 = sizes.iter().map(|s| s.height).sum();
        // Y up: the first row at the top, the margin above each row's box.
        let mut top = height;
        for (row, size) in self.children.iter_mut().zip(sizes) {
            top -= size.height;
            row.set_bounds(Rect::new(0.0, top, available.width, size.height));
        }
        Size::new(available.width, height)
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}
