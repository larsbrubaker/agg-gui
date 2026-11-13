//! A remembered folder that lists slowly: while the memory finds it, `FileBrowser` and
//! `BrowserPage` show its breadcrumbs (`BrowserMemory::last_location_trail`) over a loading line,
//! never the root's rows. Crumb clicks and Back move along the trail being found, the user's own
//! navigation ends the restore, `retry_restore` runs it again, and a browser made `remembering`
//! remembers each folder the user moves to; the root's header markdown stays hidden meanwhile.
//! Each answer tells the restore-finished listeners whether it landed, failed, was abandoned by
//! the user or superseded by a retry; a landed restore is not written back to the memory, nor is
//! a superseded one's root, but one the user shortened with a crumb is. A restore landing on the
//! trail it showed keeps the same crumb widgets, so a click pressed on one as it lands goes
//! through; a new trail or a new look builds them again.
//! Driven through agg-gui's `App` with real clicks.

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};

use agg_gui::text::Font;
use agg_gui::{App, Event, Modifiers, MouseButton, Point, Rect, Size, Widget};
use agg_gui_file_browser::{
    run_ready_local_tasks, BrowserConfig, BrowserLayout, BrowserLook, BrowserMemory, BrowserNames,
    BrowserOptions, BrowserPage, BrowserState, BrowserStyle, FileBrowser, LocationFuture,
    OpenFolder, RestoreOutcome, ViewMode,
};
use agg_gui_files::{DynamicFolder, Folder, MemoryFolder};

const WIDTH: f64 = 700.0;
const HEIGHT: f64 = 400.0;
const KEY: &str = "main";

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(agg_gui::fonts::NOTO_SANS_REGULAR).expect("bundled font loads"))
}

/// The answer to one search for a remembered folder, given when the test says.
#[derive(Default)]
struct Answer {
    folders: Option<Option<Vec<Box<dyn Folder>>>>,
    waker: Option<Waker>,
}

/// A future waiting for its [`Answer`].
struct Waiting(Rc<RefCell<Answer>>);

impl Future for Waiting {
    type Output = Option<Vec<Box<dyn Folder>>>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut answer = self.0.borrow_mut();
        match answer.folders.take() {
            Some(folders) => Poll::Ready(folders),
            None => {
                answer.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

/// Home holding Projects (holding Brackets), and a memory remembering Home > Projects > Brackets
/// whose search answers only when the test gives it the folders.
struct Slow {
    home: MemoryFolder,
    /// Projects and Brackets, which the search finds.
    found: [MemoryFolder; 2],
    answers: RefCell<Vec<Rc<RefCell<Answer>>>>,
    remembered: RefCell<Vec<Vec<String>>>,
}

impl Slow {
    fn new() -> Rc<Self> {
        let home = MemoryFolder::new("Home");
        let projects = home.add_folder("Projects").expect("free");
        let brackets = projects.add_folder("Brackets").expect("free");
        home.add_file("gear.stl", b"g".to_vec());
        Rc::new(Self {
            home,
            found: [projects, brackets],
            answers: RefCell::default(),
            remembered: RefCell::default(),
        })
    }

    /// Answers the latest search with Projects and Brackets, and runs the restore's task.
    fn answer(&self) {
        let [projects, brackets] = self.found.clone();
        let folders: Vec<Box<dyn Folder>> = vec![Box::new(projects), Box::new(brackets)];
        self.give(Some(folders));
    }

    /// Answers the latest search with `folders` (`None`: unreachable), and runs the task.
    fn give(&self, folders: Option<Vec<Box<dyn Folder>>>) {
        let answer = Rc::clone(self.answers.borrow().last().expect("a search ran"));
        answer.borrow_mut().folders = Some(folders);
        if let Some(waker) = answer.borrow_mut().waker.take() {
            waker.wake();
        }
        run_ready_local_tasks();
    }
}

impl BrowserMemory for Slow {
    fn last_location(&self, _key: &str) -> Option<LocationFuture> {
        let answer = Rc::new(RefCell::new(Answer::default()));
        self.answers.borrow_mut().push(Rc::clone(&answer));
        Some(Box::pin(Waiting(answer)))
    }
    fn last_location_trail(&self, _key: &str) -> Option<Vec<String>> {
        Some(vec!["Home".into(), "Projects".into(), "Brackets".into()])
    }
    fn remember_location(&self, _key: &str, path: &[OpenFolder]) {
        let names = path.iter().map(|f| f.borrow().name().to_string()).collect();
        self.remembered.borrow_mut().push(names);
    }
}

fn state(memory: &Rc<Slow>) -> Rc<RefCell<BrowserState>> {
    agg_gui::set_device_scale(1.0);
    let config = BrowserConfig::builder()
        .memory(Rc::clone(memory) as Rc<dyn BrowserMemory>)
        .build();
    let state = BrowserState::new(Box::new(memory.home.clone()), config);
    state.borrow_mut().set_view_mode(ViewMode::Rows);
    state
}

fn layout(app: &mut App) {
    run_ready_local_tasks();
    app.layout(Size::new(WIDTH, HEIGHT));
}

fn find<'a>(widget: &'a dyn Widget, id: &str, origin: Point) -> Option<(&'a dyn Widget, Rect)> {
    let b = widget.bounds();
    let here = Point::new(origin.x + b.x, origin.y + b.y);
    if widget.id() == Some(id) {
        return Some((widget, Rect::new(here.x, here.y, b.width, b.height)));
    }
    widget
        .children()
        .iter()
        .find_map(|child| find(child.as_ref(), id, here))
}

fn click(app: &mut App, id: &str) {
    let r = find(app.root(), id, Point::new(0.0, 0.0))
        .unwrap_or_else(|| panic!("no widget named {id}"))
        .1;
    let at = Point::new(r.x + r.width / 2.0, r.y + r.height / 2.0);
    let y = HEIGHT - at.y;
    app.on_mouse_move(at.x, y);
    app.on_mouse_down(at.x, y, MouseButton::Left, Modifiers::default());
    app.on_mouse_up(at.x, y, MouseButton::Left, Modifiers::default());
    layout(app);
}

fn crumb(state: &Rc<RefCell<BrowserState>>, name: &str) -> String {
    state
        .borrow()
        .config()
        .look
        .names()
        .bread_crumb_button_for(name)
}

fn property(app: &App, key: &str) -> String {
    app.root()
        .properties()
        .into_iter()
        .find(|(k, _)| *k == key)
        .unwrap_or_else(|| panic!("no property {key}"))
        .1
}

#[test]
fn the_browser_shows_the_remembered_trail_over_a_loading_line_until_found() {
    let memory = Slow::new();
    let state = state(&memory);
    let browser = FileBrowser::new(&state, font()).remembering(KEY);
    let mut app = App::new(Box::new(browser));
    layout(&mut app);
    assert_eq!(property(&app, "restoring"), "true");
    assert_eq!(
        property(&app, "bread_crumb_trail"),
        "Home/Projects/Brackets"
    );
    assert!(find(app.root(), &crumb(&state, "Brackets"), Point::default()).is_some());
    let message = state.borrow().listing_message().expect("a loading line");
    assert!(message.loading, "{message:?}");
    assert!(
        !state.borrow().lists_row(&*memory.home.items()[0]),
        "no root rows"
    );
    memory.answer();
    layout(&mut app);
    assert_eq!(property(&app, "restoring"), "false");
    assert_eq!(
        state.borrow().path_names(),
        ["Home", "Projects", "Brackets"]
    );
    click(&mut app, &crumb(&state, "Projects"));
    assert_eq!(
        memory.remembered.borrow().last().cloned(),
        Some(vec!["Home".to_string(), "Projects".to_string()]),
        "the user's move is remembered as it happens"
    );
}

#[test]
fn a_folder_crumb_clicked_meanwhile_shortens_the_restore_is_remembered_and_home_ends_it() {
    let memory = Slow::new();
    let state = state(&memory);
    let mut app = App::new(Box::new(FileBrowser::new(&state, font()).remembering(KEY)));
    layout(&mut app);
    let heard: Heard = Rc::default();
    let sink = Rc::clone(&heard);
    state
        .borrow_mut()
        .add_restore_finished(Rc::new(move |outcome| sink.borrow_mut().push((outcome, 0))));
    click(&mut app, &crumb(&state, "Projects"));
    assert_eq!(property(&app, "bread_crumb_trail"), "Home/Projects");
    memory.answer();
    layout(&mut app);
    layout(&mut app);
    assert_eq!(state.borrow().path_names(), ["Home", "Projects"]);
    assert_eq!(*heard.borrow(), [(RestoreOutcome::LandedShortened, 0)]);
    // The folder the user chose on the way is where they left the browser.
    assert_eq!(
        memory.remembered.borrow().last().cloned(),
        Some(vec!["Home".to_string(), "Projects".to_string()]),
        "the shortened landing is remembered"
    );

    let memory = Slow::new();
    let state = self::state(&memory);
    let mut app = App::new(Box::new(FileBrowser::new(&state, font()).remembering(KEY)));
    layout(&mut app);
    click(&mut app, &crumb(&state, "Home"));
    assert_eq!(property(&app, "restoring"), "false");
    assert!(
        state.borrow().listing_message().is_none(),
        "the root's rows show"
    );
    memory.answer();
    layout(&mut app);
    assert_eq!(state.borrow().depth(), 1, "the late answer opens nothing");
}

#[test]
fn back_walks_up_the_trail_and_a_retry_looks_again() {
    let memory = Slow::new();
    let state = state(&memory);
    let mut app = App::new(Box::new(BrowserPage::new(&state, font()).remembering(KEY)));
    layout(&mut app);
    assert!(find(app.root(), &crumb(&state, "Brackets"), Point::default()).is_some());
    let back = Event::MouseDown {
        pos: Point::new(10.0, 10.0),
        button: MouseButton::Other(agg_gui_file_browser::BACK_MOUSE_BUTTON),
        modifiers: Modifiers::default(),
    };
    app.root_mut().on_event(&back);
    layout(&mut app);
    assert_eq!(state.borrow().shown_trail(), ["Home", "Projects"]);
    assert!(find(app.root(), &crumb(&state, "Brackets"), Point::default()).is_none());
    app.root_mut().on_event(&back);
    layout(&mut app);
    assert!(
        !state.borrow().is_restoring(),
        "Back at the trail's top ends it"
    );
    BrowserState::retry_restore(&state);
    layout(&mut app);
    assert_eq!(
        state.borrow().shown_trail(),
        ["Home", "Projects", "Brackets"]
    );
    memory.answer();
    layout(&mut app);
    assert_eq!(
        state.borrow().path_names(),
        ["Home", "Projects", "Brackets"]
    );
}

#[test]
fn no_header_markdown_shows_while_the_remembered_folder_is_found() {
    let memory = Slow::new();
    agg_gui::set_device_scale(1.0);
    let config = BrowserConfig::builder()
        .memory(Rc::clone(&memory) as Rc<dyn BrowserMemory>)
        .build();
    let root = DynamicFolder::new("Home").with_header_markdown("Home's own words");
    let state = BrowserState::new(Box::new(root), config);
    state.borrow_mut().set_view_mode(ViewMode::Rows);
    let mut app = App::new(Box::new(FileBrowser::new(&state, font()).remembering(KEY)));
    layout(&mut app);
    assert_eq!(property(&app, "restoring"), "true");
    let header = state.borrow().config().look.names().header_markdown.clone();
    assert!(
        find(app.root(), &header, Point::default()).is_none(),
        "no root header over the loading line"
    );
    click(&mut app, &crumb(&state, "Home"));
    assert_eq!(property(&app, "restoring"), "false");
    assert!(
        find(app.root(), &header, Point::default()).is_some(),
        "the root's header once the root is what shows"
    );
}

/// The restore-finished listener's notes: each outcome, with the depth it read from inside.
type Heard = Rc<RefCell<Vec<(RestoreOutcome, usize)>>>;

/// A browser restoring under `memory` whose restore-finished listener takes notes.
fn heard(memory: &Rc<Slow>) -> (App, Rc<RefCell<BrowserState>>, Heard) {
    let state = state(memory);
    let heard: Heard = Rc::default();
    let (sink, weak) = (Rc::clone(&heard), Rc::downgrade(&state));
    state
        .borrow_mut()
        .add_restore_finished(Rc::new(move |outcome| {
            let depth = weak.upgrade().map_or(0, |s| s.borrow().depth());
            sink.borrow_mut().push((outcome, depth));
        }));
    let mut app = App::new(Box::new(FileBrowser::new(&state, font()).remembering(KEY)));
    layout(&mut app);
    (app, state, heard)
}

#[test]
fn the_restore_finished_listeners_hear_whether_it_landed_failed_was_abandoned_or_superseded() {
    let memory = Slow::new();
    let (mut app, _state, heard) = self::heard(&memory);
    assert!(heard.borrow().is_empty(), "nothing while it is found");
    memory.answer();
    layout(&mut app);
    assert_eq!(
        *heard.borrow(),
        [(RestoreOutcome::Landed, 3)],
        "read from the listener"
    );

    let memory = Slow::new();
    let (mut app, state, heard) = self::heard(&memory);
    memory.give(None);
    layout(&mut app);
    assert_eq!(*heard.borrow(), [(RestoreOutcome::Failed, 1)]);
    assert!(!state.borrow().is_restoring());

    let memory = Slow::new();
    let (mut app, state, heard) = self::heard(&memory);
    click(&mut app, &crumb(&state, "Home"));
    assert!(heard.borrow().is_empty(), "told when the answer comes");
    memory.answer();
    layout(&mut app);
    assert_eq!(*heard.borrow(), [(RestoreOutcome::Abandoned, 1)]);

    // A retry's answer is told too, and the earlier search answering late is superseded.
    let memory = Slow::new();
    let (mut app, state, heard) = self::heard(&memory);
    BrowserState::retry_restore(&state);
    memory.answer();
    layout(&mut app);
    assert_eq!(*heard.borrow(), [(RestoreOutcome::Landed, 3)]);
    let first = Rc::clone(&memory.answers.borrow()[0]);
    first.borrow_mut().folders = Some(None);
    let waker = first.borrow_mut().waker.take();
    if let Some(waker) = waker {
        waker.wake();
    }
    layout(&mut app);
    assert_eq!(
        *heard.borrow(),
        [(RestoreOutcome::Landed, 3), (RestoreOutcome::Superseded, 3)]
    );
}

#[test]
fn home_clicked_while_finding_is_remembered_once_the_late_answer_comes() {
    let memory = Slow::new();
    let state = state(&memory);
    let mut app = App::new(Box::new(FileBrowser::new(&state, font()).remembering(KEY)));
    layout(&mut app);
    click(&mut app, &crumb(&state, "Home"));
    memory.answer();
    layout(&mut app);
    assert_eq!(
        memory.remembered.borrow().last().cloned(),
        Some(vec!["Home".to_string()]),
        "where the user went is where the browser reopens"
    );
}

#[test]
fn a_restore_that_lands_is_not_written_back_and_the_users_next_move_is() {
    // The memory finds the nearest folder that is there (Projects, Brackets being gone): the
    // browser opens it, but the memory keeps Brackets for when it is back.
    let memory = Slow::new();
    let state = state(&memory);
    let mut app = App::new(Box::new(FileBrowser::new(&state, font()).remembering(KEY)));
    layout(&mut app);
    let [projects, _] = memory.found.clone();
    memory.give(Some(vec![Box::new(projects) as Box<dyn Folder>]));
    layout(&mut app);
    layout(&mut app);
    assert_eq!(state.borrow().path_names(), ["Home", "Projects"]);
    assert!(
        memory.remembered.borrow().is_empty(),
        "the place a restore landed is not written back: {:?}",
        memory.remembered.borrow()
    );

    // Where the user goes from there is remembered.
    click(&mut app, &crumb(&state, "Home"));
    layout(&mut app);
    assert_eq!(
        memory.remembered.borrow().last().cloned(),
        Some(vec!["Home".to_string()])
    );
}

#[test]
fn a_restore_a_retry_superseded_writes_nothing_when_its_late_answer_comes() {
    let memory = Slow::new();
    let (mut app, state, heard) = self::heard(&memory);
    BrowserState::retry_restore(&state);
    // The first search answers while the retry's is still finding.
    let first = Rc::clone(&memory.answers.borrow()[0]);
    first.borrow_mut().folders = Some(None);
    let waker = first.borrow_mut().waker.take();
    if let Some(waker) = waker {
        waker.wake();
    }
    layout(&mut app);
    assert!(state.borrow().is_restoring(), "the retry still finds");
    assert!(
        memory.remembered.borrow().is_empty(),
        "the root is not written over the remembered folder: {:?}",
        memory.remembered.borrow()
    );
    memory.answer();
    layout(&mut app);
    assert_eq!(
        *heard.borrow(),
        [(RestoreOutcome::Superseded, 1), (RestoreOutcome::Landed, 3)]
    );
    assert!(memory.remembered.borrow().is_empty());
}

/// The crumb named `id`'s address (which widget it is) and its window rectangle.
fn crumb_at(app: &App, id: &str) -> (*const (), Rect) {
    let (widget, rect) =
        find(app.root(), id, Point::default()).unwrap_or_else(|| panic!("no widget named {id}"));
    ((widget as *const dyn Widget).cast::<()>(), rect)
}

/// The breadcrumb trail widget's pieces, each as its type name and properties.
fn crumb_pieces(
    app: &App,
    state: &Rc<RefCell<BrowserState>>,
) -> Vec<(String, Vec<(String, String)>)> {
    let id = state.borrow().config().look.names().bread_crumb.clone();
    let (trail, _) = find(app.root(), &id, Point::default()).expect("a breadcrumb trail");
    trail
        .children()
        .iter()
        .map(|piece| {
            let properties = piece.properties().into_iter();
            let properties = properties.map(|(k, v)| (k.to_string(), v)).collect();
            (piece.type_name().to_string(), properties)
        })
        .collect()
}

/// The texts of the trail's folder links, the root first.
fn crumb_texts(app: &App, state: &Rc<RefCell<BrowserState>>) -> Vec<String> {
    crumb_pieces(app, state)
        .into_iter()
        .filter(|(kind, _)| kind == "LinkLabel")
        .filter_map(|(_, properties)| properties.into_iter().find(|(k, _)| k == "text"))
        .map(|(_, text)| text)
        .collect()
}

#[test]
fn a_crumb_pressed_as_the_restore_lands_on_its_trail_is_kept_and_its_click_goes_through() {
    for layout in [BrowserLayout::TreeAndList, BrowserLayout::ListOnly] {
        pressed_as_it_lands(layout);
    }
}

/// A press on Home while the restore is found, the restore landing, then the release.
fn pressed_as_it_lands(form: BrowserLayout) {
    let memory = Slow::new();
    agg_gui::set_device_scale(1.0);
    let config = BrowserConfig::builder()
        .memory(Rc::clone(&memory) as Rc<dyn BrowserMemory>)
        .options(BrowserOptions {
            layout: form,
            ..BrowserOptions::default()
        })
        .build();
    let state = BrowserState::new(Box::new(memory.home.clone()), config);
    state.borrow_mut().set_view_mode(ViewMode::Rows);
    let mut app = App::new(Box::new(FileBrowser::new(&state, font()).remembering(KEY)));
    layout(&mut app);
    let home = crumb(&state, "Home");
    let (before, r) = crumb_at(&app, &home);
    let (x, y) = (r.x + r.width / 2.0, HEIGHT - (r.y + r.height / 2.0));
    app.on_mouse_move(x, y);
    app.on_mouse_down(x, y, MouseButton::Left, Modifiers::default());
    // The folder lands on the trail it showed while the button is down.
    memory.answer();
    layout(&mut app);
    assert_eq!(property(&app, "restoring"), "false");
    assert_eq!(
        state.borrow().path_names(),
        ["Home", "Projects", "Brackets"]
    );
    let (after, _) = crumb_at(&app, &home);
    assert_eq!(
        before, after,
        "{form:?}: the crumb under the pointer is the same widget"
    );
    app.on_mouse_up(x, y, MouseButton::Left, Modifiers::default());
    layout(&mut app);
    assert_eq!(
        state.borrow().path_names(),
        ["Home"],
        "{form:?}: the click on Home goes through"
    );
}

#[test]
fn a_new_trail_builds_new_crumbs_whose_up_button_climbs_from_there() {
    let memory = Slow::new();
    let state = state(&memory);
    let mut app = App::new(Box::new(FileBrowser::new(&state, font()).remembering(KEY)));
    layout(&mut app);
    memory.answer();
    layout(&mut app);
    assert_eq!(crumb_texts(&app, &state), ["Home", "Projects", "Brackets"]);
    click(&mut app, &crumb(&state, "Projects"));
    assert_eq!(state.borrow().path_names(), ["Home", "Projects"]);
    assert_eq!(crumb_texts(&app, &state), ["Home", "Projects"]);
    let up = state.borrow().config().look.names().up_button.clone();
    click(&mut app, &up);
    assert_eq!(state.borrow().path_names(), ["Home"], "up from Projects");
    assert_eq!(crumb_texts(&app, &state), ["Home"]);
}

/// The default look's style at a text size the test changes.
struct SizedLook {
    names: BrowserNames,
    font_size: Cell<f64>,
}

impl BrowserLook for SizedLook {
    fn style(&self) -> BrowserStyle {
        BrowserStyle {
            font_size: self.font_size.get(),
            ..BrowserStyle::default()
        }
    }
    fn names(&self) -> &BrowserNames {
        &self.names
    }
}

#[test]
fn a_look_changed_while_finding_shows_in_the_crumbs_once_the_restore_lands() {
    let memory = Slow::new();
    agg_gui::set_device_scale(1.0);
    let look = Rc::new(SizedLook {
        names: BrowserNames::default(),
        font_size: Cell::new(14.0),
    });
    let config = BrowserConfig::builder()
        .memory(Rc::clone(&memory) as Rc<dyn BrowserMemory>)
        .look(Rc::clone(&look) as Rc<dyn BrowserLook>)
        .build();
    let state = BrowserState::new(Box::new(memory.home.clone()), config);
    state.borrow_mut().set_view_mode(ViewMode::Rows);
    let mut app = App::new(Box::new(FileBrowser::new(&state, font()).remembering(KEY)));
    layout(&mut app);
    // The separators ("/", the plain labels other than an elision's "...") are one unit larger
    // than the links.
    let separator_sizes = |app: &App| -> Vec<String> {
        crumb_pieces(app, &state)
            .into_iter()
            .filter(|(kind, properties)| {
                kind == "Label" && properties.iter().any(|(k, v)| k == "text" && v == "/")
            })
            .filter_map(|(_, properties)| {
                let size = properties.into_iter().find(|(k, _)| k == "font_size");
                size.map(|(_, v)| v)
            })
            .collect()
    };
    assert_eq!(separator_sizes(&app), ["15.0", "15.0"]);
    look.font_size.set(20.0);
    memory.answer();
    layout(&mut app);
    assert_eq!(
        state.borrow().path_names(),
        ["Home", "Projects", "Brackets"]
    );
    // The larger trail may elide a crumb and its separator: every one shown is the new look's.
    let sizes = separator_sizes(&app);
    assert!(
        !sizes.is_empty() && sizes.iter().all(|s| s == "21.0"),
        "the new look's crumbs: {sizes:?}"
    );
}
