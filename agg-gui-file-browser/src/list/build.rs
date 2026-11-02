//! What every row and tile is built from ([`ListCx`]): the browser's state, its look, style and
//! font, its pictures hook and what the hook is handed, gathered once per build of the list
//! (`list/mod.rs`); and the parts of one row ([`RowParts`]), read from the state and then from
//! the hooks with the state let go (the crate header's rule).

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::text::Font;
use agg_gui::{Size, Widget};
use agg_gui_files::{Item, Spawner};

use crate::look::{BrowserIcon, BrowserLook, BrowserStyle};
use crate::pictures::{ItemPictures, OverlayFrame, PictureCx};
use crate::state::{BrowserEntry, BrowserState};

use super::menu_button::MENU_GLYPH_SIZE;
use super::tile_art::{tile_art, Art, TileArt};

/// What a list's rows and tiles are built with.
#[derive(Clone)]
pub struct ListCx {
    /// The browser.
    pub state: Rc<RefCell<BrowserState>>,
    /// Its look.
    pub look: Rc<dyn BrowserLook>,
    /// The look's style, read once per build.
    pub style: BrowserStyle,
    /// The font names are drawn in.
    pub font: Arc<Font>,
    /// The pictures hook.
    pub pictures: Rc<dyn ItemPictures>,
    /// Where the hook's work runs.
    pub spawner: Rc<dyn Spawner>,
    /// What the hook calls when a picture arrives: the list builds again.
    pub arrived: Rc<dyn Fn()>,
    /// The overflow glyph a hovered row draws.
    pub overflow: Art,
    /// The open folder's path, which every entry handed to a hook carries.
    pub path: Rc<[String]>,
}

/// One row's or tile's parts.
pub struct RowParts {
    /// The listed row.
    pub entry: BrowserEntry,
    /// Its widget name.
    pub id: String,
    /// Whether it is selected.
    pub selected: bool,
    /// Whether the pointer is over it (kept across rebuilds by the menu button state).
    pub hovered: bool,
    /// What it shows where its picture goes.
    pub art: TileArt,
    /// What the app lays over its picture.
    pub overlay: Option<Box<dyn Widget>>,
}

impl ListCx {
    /// The context for building `state`'s list in `look` with `font`, a picture's arrival
    /// calling `arrived`.
    pub fn new(
        state: &Rc<RefCell<BrowserState>>,
        look: Rc<dyn BrowserLook>,
        font: Arc<Font>,
        arrived: Rc<dyn Fn()>,
    ) -> Self {
        let (pictures, spawner, path) = {
            let s = state.borrow();
            let config = s.config();
            (
                Rc::clone(&config.pictures),
                Rc::clone(&config.spawner),
                Rc::<[String]>::from(s.path_names()),
            )
        };
        let style = look.style();
        let glyph = Size::new(MENU_GLYPH_SIZE, MENU_GLYPH_SIZE);
        let overflow = match look.tinted_icon(BrowserIcon::Overflow, glyph, style.text_color) {
            Some(image) => Art::Image(image),
            None => Art::Glyph {
                icon: BrowserIcon::Overflow,
                color: style.text_color,
                side: MENU_GLYPH_SIZE * 0.6,
            },
        };
        Self {
            state: Rc::clone(state),
            look,
            style,
            font,
            pictures,
            spawner,
            arrived,
            overflow,
            path,
        }
    }

    /// The parts of listed row `index` with its picture `side` units square, an overlay framed
    /// as `frame`; `None` past the listing's end.
    pub fn parts(&self, index: usize, side: i32, frame: OverlayFrame) -> Option<RowParts> {
        let (entry, selected, hovered) = {
            let s = self.state.borrow();
            let entry = s.listed_entry(index)?;
            (
                entry,
                s.is_selected(index),
                s.menu_button().is_hovered(index),
            )
        };
        let id = entry.row_name(self.look.names());
        let hook_entry = entry.to_entry(Rc::clone(&self.path));
        let state = Rc::clone(&self.state);
        let folder_picture = move |item: &Rc<dyn Item>, w: i32, h: i32| {
            state
                .try_borrow()
                .ok()
                .and_then(|s| s.container_thumbnail(item, w, h))
        };
        let cx = PictureCx::new(&self.spawner, &self.arrived).with_folder_picture(&folder_picture);
        let art = tile_art(
            self.look.as_ref(),
            &self.style,
            self.pictures.as_ref(),
            &cx,
            &hook_entry,
            side,
        );
        let overlay = self.pictures.tile_overlay(&hook_entry, frame);
        Some(RowParts {
            entry,
            id,
            selected,
            hovered,
            art,
            overlay,
        })
    }
}
