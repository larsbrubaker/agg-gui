//! Which rows and tiles ask for their pictures: those within the list's visible part or
//! [`READ_AHEAD`] units of it. Rows are built with placeholder art (`list/build.rs`); after each
//! layout the list (`list/mod.rs`) hands its scrolled content here with where its viewport is, and
//! every row or tile there that has not asked yet asks the pictures hook (`BrowserRow::ask_art`,
//! `BrowserTile::ask_art`). So opening a folder of hundreds of pictures asks for the screenful
//! the user sees and a little more, and scrolling asks for the rest as they come into view; a row
//! that asked keeps its picture when scrolled away.

use agg_gui::Widget;

use super::icon_view::BrowserTile;
use super::row::BrowserRow;

/// How far above and below the visible part rows ask for their pictures, in logical units: a
/// couple of rows, so a short scroll finds them already on their way.
pub const READ_AHEAD: f64 = 120.0;

/// Has every row and tile of `content` (the scrolled rows or tiles, Y up) within [`READ_AHEAD`]
/// of the visible part ask for its picture: `offset` units scrolled down from the content's top,
/// `height` units showing.
pub fn ask_visible_art(content: &mut dyn Widget, offset: f64, height: f64) {
    let content_height = content.bounds().height;
    let (from, to) = (offset - READ_AHEAD, offset + height + READ_AHEAD);
    for child in content.children_mut().iter_mut() {
        let b = child.bounds();
        // The child's top and bottom as distances down from the content's top.
        let top = content_height - (b.y + b.height);
        let bottom = top + b.height;
        if bottom < from || top > to {
            continue;
        }
        let Some(any) = child.as_any_mut() else {
            continue;
        };
        if let Some(row) = any.downcast_mut::<BrowserRow>() {
            row.ask_art();
        } else if let Some(tile) = any.downcast_mut::<BrowserTile>() {
            tile.ask_art();
        }
    }
}
