//! A look that draws the navigation bars' pieces as its app's own widgets
//! (`BrowserLook::menu_button`, `overflow_button`, `search_field`, `clear_search_button`): the
//! composed browser places them where its own would stand, under the look's names, and hands
//! them what its own show and do: the view and sorting menus' rows (marked as the state is when
//! asked, a pick changing the state), the overflow menu's rows as the browser's own overflow menu
//! shows them, and the search box's clear, which ends the search.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::text::Font;
use agg_gui::{
    App, DrawCtx, Event, EventResult, Framebuffer, GfxCtx, Rect, Size, TextField, Widget,
};
use agg_gui_file_browser::{
    menu_actions, run_ready_local_tasks, BrowserAction, BrowserConfig, BrowserIcon, BrowserLayout,
    BrowserLook, BrowserNames, BrowserOptions, BrowserState, BrowserStyle, FileBrowser, MenuScope,
    NavMenuRow, NavMenuRows, OverflowRows, ViewMode,
};
use agg_gui_files::MemoryFolder;

const WIDTH: f64 = 700.0;
const HEIGHT: f64 = 400.0;

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(agg_gui::fonts::NOTO_SANS_REGULAR).expect("bundled font loads"))
}

/// Home holding Projects, gear.stl and bolt.stl.
fn home() -> MemoryFolder {
    let home = MemoryFolder::new("Home");
    home.add_folder("Projects").expect("free");
    home.add_file("gear.stl", b"solid gear".to_vec());
    home.add_file("bolt.stl", b"solid bolt".to_vec());
    home
}

/// A stand-in for an app's button: a named 32-unit square.
struct OwnButton {
    name: String,
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl OwnButton {
    fn boxed(name: &str) -> Box<dyn Widget> {
        Box::new(Self {
            name: name.to_string(),
            bounds: Rect::default(),
            children: Vec::new(),
        })
    }
}

impl Widget for OwnButton {
    fn type_name(&self) -> &'static str {
        "OwnButton"
    }
    fn id(&self) -> Option<&str> {
        Some(&self.name)
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
    fn layout(&mut self, _available: Size) -> Size {
        Size::new(32.0, 32.0)
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// A look drawing every bar piece its own way, keeping what it was handed.
#[derive(Default)]
struct OwnBar {
    names: BrowserNames,
    menus: RefCell<Vec<(BrowserIcon, String, String, NavMenuRows)>>,
    overflow: RefCell<Option<OverflowRows>>,
    clear: RefCell<Option<Rc<dyn Fn()>>>,
    fields: Cell<u32>,
}

impl BrowserLook for OwnBar {
    fn style(&self) -> BrowserStyle {
        BrowserStyle::default()
    }
    fn menu_button(
        &self,
        which: BrowserIcon,
        name: &str,
        tooltip: &str,
        rows: NavMenuRows,
        _font: &Arc<Font>,
    ) -> Option<Box<dyn Widget>> {
        let entry = (which, name.to_string(), tooltip.to_string(), rows);
        self.menus.borrow_mut().push(entry);
        Some(OwnButton::boxed(name))
    }
    fn overflow_button(
        &self,
        name: &str,
        rows: OverflowRows,
        _font: &Arc<Font>,
    ) -> Option<Box<dyn Widget>> {
        *self.overflow.borrow_mut() = Some(rows);
        Some(OwnButton::boxed(name))
    }
    fn search_field(&self, font: &Arc<Font>) -> Option<TextField> {
        self.fields.set(self.fields.get() + 1);
        Some(TextField::new(Arc::clone(font)))
    }
    fn clear_search_button(
        &self,
        name: &str,
        _tooltip: &str,
        clear: Rc<dyn Fn()>,
        _font: &Arc<Font>,
    ) -> Option<Box<dyn Widget>> {
        *self.clear.borrow_mut() = Some(clear);
        Some(OwnButton::boxed(name))
    }
    fn names(&self) -> &BrowserNames {
        &self.names
    }
}

struct Browser {
    app: App,
    state: Rc<RefCell<BrowserState>>,
    look: Rc<OwnBar>,
}

/// The browser at Home in `layout`, drawn in the [`OwnBar`] look, laid out and painted once.
fn browser(layout: BrowserLayout) -> Browser {
    agg_gui::set_device_scale(1.0);
    let look = Rc::new(OwnBar::default());
    let config = BrowserConfig::builder()
        .look(Rc::clone(&look) as Rc<dyn BrowserLook>)
        .options(BrowserOptions {
            layout,
            ..BrowserOptions::default()
        })
        .build();
    let state = BrowserState::new(Box::new(home()), config);
    state.borrow_mut().set_view_mode(ViewMode::Rows);
    let app = App::new(Box::new(FileBrowser::new(&state, font())));
    let mut b = Browser { app, state, look };
    frame(&mut b.app);
    b
}

fn frame(app: &mut App) {
    run_ready_local_tasks();
    app.layout(Size::new(WIDTH, HEIGHT));
    let mut fb = Framebuffer::new(WIDTH as u32, HEIGHT as u32);
    let mut ctx = GfxCtx::new(&mut fb);
    app.paint(&mut ctx);
}

/// The type of the widget named `id`.
fn type_of(widget: &dyn Widget, id: &str) -> Option<&'static str> {
    if widget.id() == Some(id) {
        return Some(widget.type_name());
    }
    widget
        .children()
        .iter()
        .find_map(|c| type_of(c.as_ref(), id))
}

/// A menu's rows as text: titles, marked ones with a "*", separators as "-".
fn menu_text(rows: &NavMenuRows) -> Vec<String> {
    rows()
        .into_iter()
        .map(|row| match row {
            NavMenuRow::Separator => "-".to_string(),
            NavMenuRow::Check { title, checked, .. } => mark(title, checked),
            NavMenuRow::Radio {
                title, selected, ..
            } => mark(title, selected),
        })
        .collect()
}

fn mark(title: String, marked: bool) -> String {
    match marked {
        true => format!("{title}*"),
        false => title,
    }
}

/// Runs the row titled `title`.
fn pick(rows: &NavMenuRows, title: &str) {
    let run = rows().into_iter().find_map(|row| match row {
        NavMenuRow::Check { title: t, run, .. } | NavMenuRow::Radio { title: t, run, .. }
            if t == title =>
        {
            Some(run)
        }
        _ => None,
    });
    run.unwrap_or_else(|| panic!("no row {title}"))();
}

/// The titles of a browser menu's rows, separators as "-".
fn action_titles(rows: &[BrowserAction]) -> Vec<String> {
    rows.iter()
        .map(|row| match row {
            BrowserAction::Command(command) => command.title().to_string(),
            _ => "-".to_string(),
        })
        .collect()
}

#[test]
fn the_tab_bar_places_the_looks_own_buttons_under_the_looks_names() {
    let b = browser(BrowserLayout::TreeAndList);
    let names = BrowserNames::default();
    let root = b.app.root();
    for id in [
        &names.view_options_button,
        &names.sort_button,
        &names.overflow_menu,
        &names.close_search,
    ] {
        assert_eq!(type_of(root, id), Some("OwnButton"), "{id}");
    }
    assert_eq!(
        b.look.fields.get(),
        1,
        "the search box asks for its field once"
    );
    let menus = b.look.menus.borrow();
    let asked: Vec<(BrowserIcon, &str, &str)> = menus
        .iter()
        .map(|(which, name, tip, _)| (*which, name.as_str(), tip.as_str()))
        .collect();
    assert_eq!(
        asked,
        vec![
            (
                BrowserIcon::ViewOptions,
                names.view_options_button.as_str(),
                "View Settings"
            ),
            (BrowserIcon::Sort, names.sort_button.as_str(), "Sorting"),
        ]
    );
}

#[test]
fn the_menus_rows_are_marked_as_the_state_is_and_a_pick_changes_it() {
    let b = browser(BrowserLayout::TreeAndList);
    let menus = b.look.menus.borrow();
    let view = &menus[0].3;
    assert_eq!(
        menu_text(view),
        [
            "Show Folders*",
            "-",
            "View List*",
            "View Icons",
            "View Large Icons"
        ]
    );
    pick(view, "View Icons");
    assert_eq!(b.state.borrow().view_mode(), ViewMode::Icons);
    assert_eq!(
        menu_text(view)[2..],
        ["View List", "View Icons*", "View Large Icons"],
        "asked again, the rows show the state as it is now"
    );
    pick(view, "Show Folders");
    assert!(!b.state.borrow().show_containers());
    let sort = &menus[1].3;
    let sort_rows = menu_text(sort);
    assert_eq!(
        sort_rows.len(),
        6,
        "three keys, a separator and two directions"
    );
    pick(sort, "Descending");
    assert!(!b.state.borrow().sort().ascending);
    assert!(menu_text(sort).contains(&"Descending*".to_string()));
}

#[test]
fn the_overflow_rows_are_the_browsers_own_overflow_menu() {
    let b = browser(BrowserLayout::TreeAndList);
    let rows = b
        .look
        .overflow
        .borrow()
        .clone()
        .expect("the look was asked");
    let own = menu_actions(&b.state, MenuScope::Overflow);
    assert!(!own.is_empty());
    assert_eq!(action_titles(&rows()), action_titles(&own));
}

#[test]
fn the_looks_clear_button_ends_the_search() {
    let mut b = browser(BrowserLayout::ListOnly);
    let names = BrowserNames::default();
    assert_eq!(
        type_of(b.app.root(), &names.close_search),
        Some("OwnButton")
    );
    // The list-only form's view and sorting buttons are the look's too.
    let asked = b.look.menus.borrow().len();
    assert_eq!(asked, 2);
    let search = {
        let root = b.app.root();
        let browser = root
            .as_any()
            .and_then(|a| a.downcast_ref::<FileBrowser>())
            .expect("the root is the browser");
        Rc::clone(browser.search())
    };
    search.set_query("gear");
    search.flush();
    frame(&mut b.app);
    assert!(search.is_search_active());
    let clear = b.look.clear.borrow().clone().expect("the look was asked");
    clear();
    frame(&mut b.app);
    assert!(!search.is_search_active());
    assert_eq!(b.state.borrow().path_names(), ["Home"]);
}
