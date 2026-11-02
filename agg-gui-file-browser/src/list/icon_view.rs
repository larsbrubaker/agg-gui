//! The icon view: [`BrowserTile`], one tile (a picture 18, 70, 100 or 256 units square,
//! `ViewMode::thumb_size`, with a 1-unit border, and under pictures of 75 and up, or any size in
//! a list showing tile names, the name in the style's tile size, cut with an ellipsis to the
//! picture's width less 4), and [`TileStack`], the tiles flowed into as many columns as fit, the
//! space left over spread evenly between and around them, folders filling rows of their own
//! before the files start. A selected tile sits on the selection fill and a selected or hovered
//! one gets an accent border, both with the corners rounded by the style's radius. What the app
//! lays over the picture (`ItemPictures::tile_overlay`) is the tile's child. The tooltip lets the
//! user read what the tile cuts: an item with a detail line shows its name and that line, any
//! other its whole name when the tile cuts it or draws none. Pointer handling is
//! `list/item_event.rs`.

use std::sync::Arc;

use agg_gui::text::{measure_advance, Font};
use agg_gui::{DrawCtx, Event, EventResult, Point, Rect, Size, Widget};

use crate::fitted_text::fit_name;
use crate::state::ListSlot;

use super::build::{ListCx, RowParts};
use super::item_event::item_event;
use super::menu_button::{menu_glyph_rect, shows_menu_button};
use super::tile_art::{paint_art, TileArt};

/// The tile's border width.
const BORDER: f64 = 1.0;
/// The space under each row of tiles.
const ROW_GAP: f64 = 6.0;
/// The name shows under pictures this wide and up (smaller ones are picture only).
pub const NAME_MIN_THUMB: i32 = 75;
/// The space above the name.
const NAME_TOP_MARGIN: f64 = 3.0;
/// The name's inset from the picture's width.
const NAME_INSET: f64 = 4.0;
/// The name's height cap.
const NAME_MAX_HEIGHT: f64 = 20.0;

/// The name's line height at `size`, capped at [`NAME_MAX_HEIGHT`].
fn name_height(font: &Font, size: f64) -> f64 {
    (font.ascender_px(size) + font.descender_px(size)).min(NAME_MAX_HEIGHT)
}

/// A tile's size for a `thumb`-wide picture, with the name's line under it when `named`.
pub fn tile_size(thumb: i32, font: &Font, name_size: f64, named: bool) -> Size {
    let side = f64::from(thumb);
    let name = match named {
        true => NAME_TOP_MARGIN + name_height(font, name_size),
        false => 0.0,
    };
    Size::new(side + 2.0 * BORDER, side + name + 2.0 * BORDER)
}

/// One tile of the icon view (see the module header).
pub struct BrowserTile {
    cx: ListCx,
    parts: RowParts,
    index: usize,
    name: String,
    /// The name as the tile draws it, empty when it draws none.
    shown_name: String,
    /// The name and the detail line, for an item that has one.
    detail_tooltip: Option<String>,
    hovered: bool,
    thumb: i32,
    named: bool,
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl BrowserTile {
    /// Listed row `index` of `cx`'s browser as a `thumb`-wide tile, its name shown when
    /// `named`; `None` past the listing's end.
    pub fn new(cx: &ListCx, index: usize, thumb: i32, named: bool) -> Option<Self> {
        let mut parts = cx.parts(index, thumb)?;
        let name = parts.entry.name().to_string();
        let detail_tooltip = parts
            .entry
            .detail_line()
            .map(|detail| format!("{name}\n{detail}"));
        let shown_name = match named {
            true => fit_name(
                &cx.font,
                &name,
                cx.style.tile_font_size,
                f64::from(thumb) - NAME_INSET,
            ),
            false => String::new(),
        };
        let children = parts.overlay.take().into_iter().collect();
        Some(Self {
            cx: cx.clone(),
            hovered: parts.hovered,
            parts,
            index,
            name,
            shown_name,
            detail_tooltip,
            thumb,
            named,
            bounds: Rect::default(),
            children,
        })
    }

    /// Whether the tile opens a folder.
    pub fn is_container(&self) -> bool {
        self.parts.entry.is_container()
    }

    /// The tile's index in the listing it was built from.
    pub fn index(&self) -> usize {
        self.index
    }

    /// What the tile shows where its picture goes.
    pub fn art(&self) -> &TileArt {
        &self.parts.art
    }

    /// The picture's square in the tile.
    fn picture_rect(&self) -> Rect {
        let side = f64::from(self.thumb);
        Rect::new(BORDER, self.bounds.height - BORDER - side, side, side)
    }
}

impl Widget for BrowserTile {
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
    fn type_name(&self) -> &'static str {
        "BrowserTile"
    }
    fn id(&self) -> Option<&str> {
        Some(&self.parts.id)
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds;
        let picture = self.picture_rect();
        for child in &mut self.children {
            child.layout(Size::new(picture.width, picture.height));
            child.set_bounds(picture);
        }
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }
    fn layout(&mut self, _available: Size) -> Size {
        let style = &self.cx.style;
        tile_size(self.thumb, &self.cx.font, style.tile_font_size, self.named)
    }
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let style = &self.cx.style;
        let (w, h) = (self.bounds.width, self.bounds.height);
        // Everything inside the border is rounded by what is left of the tile's radius there,
        // so the shapes nest without a corner poking out.
        let radius = style.corner_radius;
        let inner_radius = (radius - BORDER).max(0.0);
        if self.parts.selected {
            ctx.set_fill_color(style.selection_fill);
            ctx.begin_path();
            ctx.rounded_rect(0.0, 0.0, w, h, radius);
            ctx.fill();
        }
        let picture = self.picture_rect();
        ctx.set_fill_color(style.thumbnail_background);
        ctx.begin_path();
        ctx.rounded_rect(
            picture.x,
            picture.y,
            picture.width,
            picture.height,
            inner_radius,
        );
        ctx.fill();
        self.parts.art.paint(ctx, picture);
        if self.named {
            let font = &self.cx.font;
            let size = style.tile_font_size;
            let width = measure_advance(font, &self.shown_name, size);
            let line = name_height(font, size);
            let baseline = picture.y - NAME_TOP_MARGIN - line + font.descender_px(size);
            ctx.set_fill_color(style.text_color);
            ctx.set_font(Arc::clone(font));
            ctx.set_font_size(size);
            ctx.fill_text(&self.shown_name, (w - width) / 2.0, baseline);
        }
        if self.parts.selected || self.hovered {
            ctx.set_stroke_color(style.accent);
            ctx.set_line_width(BORDER);
            ctx.begin_path();
            // The stroke is centred on its path, so its outer edge follows the fill's corners.
            ctx.rounded_rect(
                BORDER / 2.0,
                BORDER / 2.0,
                w - BORDER,
                h - BORDER,
                (radius - BORDER / 2.0).max(0.0),
            );
            ctx.stroke();
        }
        if self.hovered && shows_menu_button(w) {
            paint_art(ctx, &self.cx.overflow, menu_glyph_rect(w, h));
        }
    }
    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseEnter => self.hovered = true,
            Event::MouseLeave => self.hovered = false,
            _ => {}
        }
        let size = Size::new(self.bounds.width, self.bounds.height);
        item_event(
            event,
            &self.cx.state,
            self.index,
            Point::new(0.0, 0.0),
            size,
        )
    }
    /// The name and detail line for an item that has one, else the whole name when the tile
    /// cuts it or draws none.
    fn tooltip_text(&self) -> Option<&str> {
        self.detail_tooltip
            .as_deref()
            .or_else(|| (self.shown_name != self.name).then_some(self.name.as_str()))
    }
    fn properties(&self) -> Vec<(&'static str, String)> {
        vec![
            ("name", self.name.clone()),
            ("shown_name", self.shown_name.clone()),
            ("selected", self.parts.selected.to_string()),
            ("is_container", self.is_container().to_string()),
            ("picture", self.parts.art.kind.to_string()),
            ("thumb_size", self.thumb.to_string()),
            ("menu_glyph", self.hovered.to_string()),
        ]
    }
}

/// The icon view: the tiles flowed into columns, as tall as they are so a `ScrollView` knows how
/// far to scroll.
pub struct TileStack {
    thumb: i32,
    item: Size,
    /// Whether each tile opens a folder (folders fill rows of their own).
    folders: Vec<bool>,
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    id: String,
}

impl TileStack {
    /// A `thumb`-wide tile for each listed row of `cx`'s browser, named under pictures of 75
    /// and up, or under every picture when `always_named` (a list whose small tiles must still
    /// say which folder is which).
    pub fn for_state(cx: &ListCx, thumb: i32, always_named: bool) -> Self {
        let named = always_named || thumb >= NAME_MIN_THUMB;
        let slots = cx.state.borrow().list_slots();
        let tiles: Vec<BrowserTile> = slots
            .iter()
            .filter_map(|slot| match slot {
                ListSlot::Entry(index) => BrowserTile::new(cx, *index, thumb, named),
                _ => None,
            })
            .collect();
        Self {
            thumb,
            item: tile_size(thumb, &cx.font, cx.style.tile_font_size, named),
            folders: tiles.iter().map(BrowserTile::is_container).collect(),
            bounds: Rect::default(),
            children: tiles
                .into_iter()
                .map(|t| Box::new(t) as Box<dyn Widget>)
                .collect(),
            id: cx.look.names().rows.clone(),
        }
    }

    /// The column count and the margin each side of every tile, counted in device pixels so the
    /// tiles land on whole pixels: the picture is its width times the device scale, truncated,
    /// the 1-pixel margin each side unscaled, and the spare space divided in whole device pixels
    /// before it is turned back into logical units.
    fn flow(&self, width: f64) -> (usize, f64) {
        let scale = agg_gui::device_scale();
        // Truncates toward zero; widths are never negative.
        let item_width = (f64::from(self.thumb) * scale) as i32 + 2;
        let width_px = width * scale;
        let mut columns = (width_px / f64::from(item_width)).floor() as i32;
        let mut remaining = width_px as i32 - columns * item_width;
        if columns < 1 {
            columns = 1;
            remaining = 0;
        }
        // Integer division: the spare space per gap is whole device pixels.
        let margin = match remaining > 0 {
            true => f64::from(remaining / (columns + 1)) / 2.0,
            false => 0.0,
        };
        // `columns` is at least 1 here.
        (columns as usize, (margin / scale).floor())
    }
}

impl Widget for TileStack {
    fn type_name(&self) -> &'static str {
        "TileStack"
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
        let (columns, margin) = self.flow(available.width);
        let item = self.item;
        let row_height = item.height + ROW_GAP;
        let cell_width = item.width + 2.0 * margin;
        // Placed top down as (distance from the top, x); a row of folders ends before the
        // first file.
        let mut placed = Vec::with_capacity(self.children.len());
        let (mut y, mut column) = (0.0, 0usize);
        let (mut row_open, mut row_of_folders) = (false, false);
        for (child, &is_folder) in self.children.iter_mut().zip(&self.folders) {
            if row_open && row_of_folders && !is_folder {
                y += row_height;
                column = 0;
                row_open = false;
            }
            if !row_open {
                row_of_folders = is_folder;
                row_open = true;
            }
            child.layout(item);
            placed.push((y, margin + column as f64 * cell_width + margin));
            column += 1;
            if column >= columns {
                y += row_height;
                column = 0;
                row_open = false;
            }
        }
        let height = if row_open { y + row_height } else { y };
        for (child, (top, x)) in self.children.iter_mut().zip(placed) {
            // Y up: the first row at the top, each row's gap below its tiles.
            child.set_bounds(Rect::new(
                x,
                height - top - item.height,
                item.width,
                item.height,
            ));
        }
        Size::new(available.width, height)
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}
