//! A folder whose live relist stays pending, shown by an idle `FileBrowser` through agg-gui's
//! `App` on the virtual clock: each layout asks for no frame at once, only one more
//! `LIVE_REFRESH_WAKE` later, and the folder gets its turn again on that frame.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::animation::{clear_draw_request, wants_draw};
use agg_gui::text::Font;
use agg_gui::{App, Size};
use agg_gui_file_browser::{BrowserConfig, BrowserState, FileBrowser, ViewMode, LIVE_REFRESH_WAKE};
use agg_gui_files::{Folder, FolderLink, Item, MemoryFolder};

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(agg_gui::fonts::NOTO_SANS_REGULAR).expect("bundled font loads"))
}

/// A folder whose relist never finishes, counting the turns it is given.
struct Relisting {
    inner: MemoryFolder,
    turns: Rc<Cell<usize>>,
}

impl Folder for Relisting {
    fn id(&self) -> &str {
        self.inner.id()
    }
    fn name(&self) -> &str {
        self.inner.name()
    }
    fn is_protected(&self) -> bool {
        self.inner.is_protected()
    }
    fn child_containers(&self) -> Vec<Rc<dyn FolderLink>> {
        self.inner.child_containers()
    }
    fn items(&self) -> Vec<Rc<dyn Item>> {
        self.inner.items()
    }
    fn load(&mut self) {
        self.inner.load()
    }
    fn pump_live_refresh(&mut self) -> bool {
        self.turns.set(self.turns.get() + 1);
        true
    }
}

#[test]
fn a_pending_relist_wakes_once_after_the_delay_never_every_frame() {
    let _clock = agg_gui::clock::scoped_virtual(None);
    agg_gui::set_device_scale(1.0);
    let turns = Rc::new(Cell::new(0));
    let inner = MemoryFolder::new("Home");
    inner.add_file("gear.stl", b"g".to_vec());
    let root = Relisting {
        inner,
        turns: Rc::clone(&turns),
    };
    let state = BrowserState::new(Box::new(root), BrowserConfig::default());
    state.borrow_mut().set_view_mode(ViewMode::Rows);
    let mut app = App::new(Box::new(FileBrowser::new(&state, font())));
    let size = Size::new(600.0, 400.0);
    // The first layouts build the browser and ask for what a new browser asks for.
    app.layout(size);
    app.layout(size);
    clear_draw_request();
    agg_gui::clock::advance(LIVE_REFRESH_WAKE);
    clear_draw_request();

    let before = turns.get();
    app.layout(size);
    assert!(turns.get() > before, "the layout gave the folder its turn");
    assert!(!wants_draw(), "no frame asked for at once");
    agg_gui::clock::advance(LIVE_REFRESH_WAKE / 2);
    assert!(!wants_draw(), "nor before the wake");
    agg_gui::clock::advance(LIVE_REFRESH_WAKE / 2);
    assert!(wants_draw(), "one frame after the wake");
}
