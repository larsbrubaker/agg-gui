//! Where a list's pictures and icons come from without an app's pictures hook: an item's own
//! picture (`OwnPictures`, the default) arrives on the spawner and replaces the item icon; and
//! the look's art is asked for again when it says its art changed (`icon_generation`), by the
//! list and by the icon button alike.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::icon_image::IconImage;
use agg_gui::text::Font;
use agg_gui::{App, Color, Size, Widget};
use agg_gui_file_browser::{
    run_ready_local_tasks, BrowserConfig, BrowserIcon, BrowserIconButton, BrowserList, BrowserLook,
    BrowserNames, BrowserState, BrowserStyle, ViewMode,
};
use agg_gui_files::{Folder, FolderLink, Item, Picture, PictureFuture, Thumbnail};

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(agg_gui::fonts::NOTO_SANS_REGULAR).expect("bundled font loads"))
}

/// An item whose own picture is a solid 4 x 4 green square, or which has none.
struct Drawing {
    name: String,
    has_picture: bool,
}

impl Item for Drawing {
    fn id(&self) -> &str {
        &self.name
    }
    fn name(&self) -> &str {
        &self.name
    }
    fn is_protected(&self) -> bool {
        false
    }
    fn is_visible(&self) -> bool {
        true
    }
    fn as_thumbnail(&self) -> Option<&dyn Thumbnail> {
        self.has_picture.then_some(self as &dyn Thumbnail)
    }
}

impl Thumbnail for Drawing {
    fn get_thumbnail(&self, _width: i32, _height: i32) -> PictureFuture {
        let green = [0u8, 255, 0, 255].repeat(16);
        Box::pin(async move { Ok(Picture::new(4, 4, green)) })
    }
}

/// A folder of two drawings, one with a picture of its own.
struct Drawings;

impl Folder for Drawings {
    fn id(&self) -> &str {
        "drawings"
    }
    fn name(&self) -> &str {
        "Drawings"
    }
    fn is_protected(&self) -> bool {
        true
    }
    fn child_containers(&self) -> Vec<Rc<dyn FolderLink>> {
        Vec::new()
    }
    fn items(&self) -> Vec<Rc<dyn Item>> {
        let drawing = |name: &str, has_picture| {
            Rc::new(Drawing {
                name: name.to_string(),
                has_picture,
            }) as Rc<dyn Item>
        };
        vec![drawing("bolt", true), drawing("nut", false)]
    }
    fn load(&mut self) {}
}

fn property(widget: &dyn Widget, id: &str, key: &str) -> Option<String> {
    if widget.id() == Some(id) {
        return widget
            .properties()
            .into_iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v);
    }
    widget
        .children()
        .iter()
        .find_map(|child| property(child.as_ref(), id, key))
}

fn layout(app: &mut App) {
    app.layout(Size::new(400.0, 300.0));
}

#[test]
fn an_items_own_picture_arrives_and_replaces_its_icon() {
    agg_gui::set_device_scale(1.0);
    let state = BrowserState::new(Box::new(Drawings), BrowserConfig::default());
    state.borrow_mut().set_view_mode(ViewMode::Icons);
    let mut app = App::new(Box::new(BrowserList::new(&state, font())));
    layout(&mut app);
    let picture = |app: &App, id| property(app.root(), id, "picture");
    assert_eq!(
        picture(&app, "Row Item bolt").as_deref(),
        Some("item"),
        "while it is made"
    );
    run_ready_local_tasks();
    layout(&mut app);
    assert_eq!(picture(&app, "Row Item bolt").as_deref(), Some("picture"));
    assert_eq!(
        picture(&app, "Row Item nut").as_deref(),
        Some("item"),
        "it has none"
    );
}

/// A look whose item art changes when the test says, counting each ask.
#[derive(Default)]
struct SwappableLook {
    names: BrowserNames,
    generation: Cell<u64>,
    asks: Cell<u32>,
}

impl BrowserLook for SwappableLook {
    fn style(&self) -> BrowserStyle {
        BrowserStyle::default()
    }
    fn icon(&self, _which: BrowserIcon, logical: Size, _enabled: bool) -> Option<IconImage> {
        self.asks.set(self.asks.get() + 1);
        let pixels = Arc::new([255u8, 255, 255, 255].repeat(4));
        IconImage::from_rgba(pixels, 2, 2, logical)
    }
    fn tinted_icon(&self, which: BrowserIcon, logical: Size, _tint: Color) -> Option<IconImage> {
        self.icon(which, logical, true)
    }
    fn icon_generation(&self) -> u64 {
        self.generation.get()
    }
    fn names(&self) -> &BrowserNames {
        &self.names
    }
}

#[test]
fn the_list_asks_for_its_art_again_when_the_looks_art_changes() {
    agg_gui::set_device_scale(1.0);
    let look = Rc::new(SwappableLook::default());
    let config = BrowserConfig::builder().look(look.clone()).build();
    let state = BrowserState::new(Box::new(Drawings), config);
    let mut app = App::new(Box::new(BrowserList::new(&state, font())));
    layout(&mut app);
    let asked = look.asks.get();
    assert!(asked > 0);
    layout(&mut app);
    assert_eq!(look.asks.get(), asked, "nothing changed, nothing asked");
    look.generation.set(1);
    layout(&mut app);
    assert!(look.asks.get() > asked, "the new art is asked for");
}

#[test]
fn the_icon_button_asks_for_its_image_again_when_the_looks_art_changes() {
    agg_gui::set_device_scale(1.0);
    let look = Rc::new(SwappableLook::default());
    let mut button = BrowserIconButton::new(look.clone(), BrowserIcon::Up, 32.0);
    button.layout(Size::new(32.0, 32.0));
    button.set_bounds(agg_gui::Rect::new(0.0, 0.0, 32.0, 32.0));
    let paint = |button: &mut BrowserIconButton| {
        let mut fb = agg_gui::Framebuffer::new(32, 32);
        let mut ctx = agg_gui::GfxCtx::new(&mut fb);
        agg_gui::widget::paint_subtree(button, &mut ctx);
    };
    paint(&mut button);
    paint(&mut button);
    assert_eq!(look.asks.get(), 1, "kept between paints");
    look.generation.set(7);
    paint(&mut button);
    assert_eq!(look.asks.get(), 2);
}
