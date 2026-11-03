//! What a row or tile shows where its picture goes ([`TileArt`]), worked out when the row is
//! built (without the app's pictures, `ListCx::placeholder_art`) and again once it comes near the
//! visible part (`list/visible_art.rs`): a folder row shows the look's folder tile (`BrowserLook::folder_tile`, which sees the
//! link and so may draw its own tile image its own way), else the link's own tile image
//! (`FolderLink::tile_icon`, resolved by `BrowserLook::icon_for_key`), else the folder icon in the accent with the link's micro icon (`FolderLink::micro_icon`) over it;
//! an item shows what the app's `ItemPictures` answers: its picture, the generating icon while it
//! is made, the item icon in the text colour, or the app's own image standing for one of those
//! (`PictureState::Shown`). An icon the look has no image for is drawn as
//! its Font Awesome glyph in the same colour. The list builds its rows again when a picture
//! arrives or the look's art changes (`list/mod.rs`), so what is worked out here stays current.

use std::sync::Arc;

use agg_gui::icon_image::IconImage;
use agg_gui::{measure_text_metrics, Color, DrawCtx, Rect, Size};

use crate::entry::Entry;
use crate::look::{glyph_face, BrowserIcon, BrowserLook, BrowserStyle};
use crate::pictures::{ItemPictures, PictureCx, PictureState};

/// The largest an item's icon draws (a picture fills its tile; the icon stays this size).
pub const ITEM_ICON_SIZE: f64 = 48.0;
/// The share of a folder tile the micro icon over it takes.
const MICRO_ICON_SHARE: f64 = 0.4;
/// The share of its box a Font Awesome glyph fills.
const GLYPH_SHARE: f64 = 0.75;

/// One image or glyph, drawn centred in a box.
#[derive(Clone)]
pub enum Art {
    /// The look's or the app's image, drawn at its own size (shrunk to fit a smaller box).
    Image(IconImage),
    /// A Font Awesome glyph in a colour, `side` units across.
    Glyph {
        icon: BrowserIcon,
        color: Color,
        side: f64,
    },
}

/// What a row or tile shows where its picture goes.
#[derive(Clone)]
pub struct TileArt {
    /// The picture or icon.
    pub art: Art,
    /// A folder's micro icon, drawn smaller over the folder icon.
    pub badge: Option<IconImage>,
    /// What it is, reported as the row's `picture` property: `folder`, `picture`,
    /// `generating` or `item`.
    pub kind: &'static str,
    /// Which art it is, reported as the row's `thumbnail` property: the look's name for its icon
    /// (`BrowserLook::icon_name`), the key a folder link named, or `picture` for an entry's own.
    pub name: String,
}

/// `which` from the look in `tint`, `side` units, else its glyph.
fn icon_art(look: &dyn BrowserLook, which: BrowserIcon, side: f64, tint: Color) -> Art {
    match look.tinted_icon(which, Size::new(side, side), tint) {
        Some(image) => Art::Image(image),
        None => Art::Glyph {
            icon: which,
            color: tint,
            side,
        },
    }
}

/// What `entry` shows in a `side`-unit square (see the module header). The pictures hook is
/// called here, so the caller holds no borrow of the browser's state.
pub fn tile_art(
    look: &dyn BrowserLook,
    style: &BrowserStyle,
    pictures: &dyn ItemPictures,
    cx: &PictureCx<'_>,
    entry: &Entry,
    side: i32,
) -> TileArt {
    let square = f64::from(side);
    let size = Size::new(square, square);
    if let Some(link) = entry.link() {
        // The look names its tile; else a link's own tile image is reported by its key, however
        // it is drawn, and any other folder by the folder icon's name.
        let folder = look.folder_tile_name(link).unwrap_or_else(|| {
            link.tile_icon()
                .map_or_else(|| look.icon_name(BrowserIcon::Folder), str::to_string)
        });
        if let Some(image) = look.folder_tile(link, square) {
            return TileArt::folder(Art::Image(image), None, folder);
        }
        let own = link
            .tile_icon()
            .and_then(|key| look.icon_for_key(key, size, style.accent));
        if let Some(image) = own {
            return TileArt::folder(Art::Image(image), None, folder);
        }
        let art = icon_art(look, BrowserIcon::Folder, square, style.accent);
        let micro = square * MICRO_ICON_SHARE;
        let badge = link
            .micro_icon()
            .and_then(|key| look.icon_for_key(key, Size::new(micro, micro), style.background));
        return TileArt::folder(art, badge, folder);
    }
    let icon_side = ITEM_ICON_SIZE.min(square);
    let icon = |which: BrowserIcon, kind| {
        let art = icon_art(look, which, icon_side, style.text_color);
        (art, kind, look.icon_name(which))
    };
    let (art, kind, name) = match pictures.picture(cx, entry, side) {
        PictureState::Ready(image) => (Art::Image(image), "picture", "picture".to_string()),
        PictureState::Generating => icon(BrowserIcon::Generating, "generating"),
        PictureState::Default => icon(BrowserIcon::Item, "item"),
        PictureState::Shown { image, kind, name } => (Art::Image(image), kind.as_str(), name),
    };
    TileArt {
        art,
        badge: None,
        kind,
        name,
    }
}

impl TileArt {
    fn folder(art: Art, badge: Option<IconImage>, name: String) -> Self {
        Self {
            art,
            badge,
            kind: "folder",
            name,
        }
    }

    /// The image's centre pixel as "r,g,b,a" at the device scale, for tests that check a tile's
    /// colour; empty for a glyph, which has no pixels of its own.
    pub fn center_pixel_text(&self) -> String {
        let Art::Image(image) = &self.art else {
            return String::new();
        };
        let Some((rgba, w, h)) = image.raster_at_scale(agg_gui::device_scale()) else {
            return String::new();
        };
        let at = ((h / 2) as usize * w as usize + (w / 2) as usize) * 4;
        match rgba.get(at..at + 4) {
            Some([r, g, b, a]) => format!("{r},{g},{b},{a}"),
            _ => String::new(),
        }
    }

    /// Draws the art centred in `rect`, an image on whole device pixels.
    pub fn paint(&self, ctx: &mut dyn DrawCtx, rect: Rect) {
        paint_art(ctx, &self.art, rect);
        if let Some(badge) = &self.badge {
            paint_art(ctx, &Art::Image(badge.clone()), rect);
        }
    }
}

/// Draws `art` centred in `rect`.
pub fn paint_art(ctx: &mut dyn DrawCtx, art: &Art, rect: Rect) {
    match art {
        Art::Image(image) => {
            let size = image.size();
            let fit = (rect.width / size.width.max(1.0))
                .min(rect.height / size.height.max(1.0))
                .min(1.0);
            let (w, h) = (size.width * fit, size.height * fit);
            let (x, y) = snap_to_device_pixel(
                ctx,
                rect.x + (rect.width - w) / 2.0,
                rect.y + (rect.height - h) / 2.0,
            );
            image.draw_in(ctx, Rect::new(x, y, w, h));
        }
        Art::Glyph { icon, color, side } => {
            let font = glyph_face();
            let size = side.min(rect.width).min(rect.height) * GLYPH_SHARE;
            let text = icon.glyph().to_string();
            let m = measure_text_metrics(&font, &text, size);
            ctx.set_font(Arc::clone(&font));
            ctx.set_font_size(size);
            ctx.set_fill_color(*color);
            let x = rect.x + (rect.width - m.width) / 2.0;
            let y = rect.y + (rect.height - (m.ascent - m.descent)) / 2.0;
            ctx.fill_text(&text, x, y);
        }
    }
}

/// `(x, y)` moved onto the nearest whole device pixel under the current transform (unchanged
/// under a turned or mirrored one, which has no pixel grid).
fn snap_to_device_pixel(ctx: &dyn DrawCtx, x: f64, y: f64) -> (f64, f64) {
    let t = ctx.transform();
    if !(t.shx == 0.0 && t.shy == 0.0 && t.sx > 0.0 && t.sy > 0.0) {
        return (x, y);
    }
    let snap = |v: f64, scale: f64, offset: f64| ((v * scale + offset).round() - offset) / scale;
    (snap(x, t.sx, t.tx), snap(y, t.sy, t.ty))
}
