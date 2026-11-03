//! What an app's own dialog page builds on: a restore whose memory answers at once opens the
//! folder inside `restore_location` even while the spawner is busy and runs nothing; a
//! `FileBrowser` made `remembering` restores as it is made, and restore-finished listeners the
//! app adds after making it hear even an answer that landed then (outcomes are told at the
//! spawner's next turn, never inside the call); a browser already off its root shows no
//! remembered trail; an answer that
//! yields (wakes itself to be polled at the next turn) still gets that turn from a spawner that
//! polls each new task at once, never landing inside the call; the look's
//! `folder_message` stands in the list's place for a folder whose listing is only a message; and
//! a `BrowserPage` is found again by its type and scrolls by the host's offset cell.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::text::Font;
use agg_gui::widget::find_widget_by_id;
use agg_gui::widgets::Named;
use agg_gui::{Size, SizedBox, Widget};
use agg_gui_file_browser::{
    BrowserConfig, BrowserList, BrowserLook, BrowserMemory, BrowserNames, BrowserPage,
    BrowserState, BrowserStyle, FileBrowser, ListingMessage, LocationFuture, OpenFolder,
    RestoreOutcome,
};
use agg_gui_files::{Folder, FolderLink, Item, LocalTask, MemoryFolder, MessageItem, Spawner};

const OWN_MESSAGE: &str = "App Folder Message";

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(agg_gui::fonts::NOTO_SANS_REGULAR).expect("bundled font loads"))
}

/// A spawner busy with other work: it holds every task and runs none.
#[derive(Default)]
struct Busy(RefCell<Vec<LocalTask>>);

impl Spawner for Busy {
    fn spawn_task(&self, task: LocalTask) {
        self.0.borrow_mut().push(task);
    }
    fn report_fault(&self, _error: Arc<dyn std::error::Error + Send + Sync>) {}
}

/// A memory that finds Projects there and then.
struct AtOnce(MemoryFolder);

impl BrowserMemory for AtOnce {
    fn last_location(&self, _key: &str) -> Option<LocationFuture> {
        let found: Vec<Box<dyn Folder>> = vec![Box::new(self.0.clone())];
        Some(Box::pin(std::future::ready(Some(found))))
    }
    fn last_location_trail(&self, _key: &str) -> Option<Vec<String>> {
        Some(vec!["Home".into(), "Projects".into()])
    }
    fn remember_location(&self, _key: &str, _path: &[OpenFolder]) {}
}

#[test]
fn a_ready_answer_opens_inside_the_call_while_the_spawner_is_busy() {
    let home = MemoryFolder::new("Home");
    let projects = home.add_folder("Projects").expect("free");
    let busy = Rc::new(Busy::default());
    let config = BrowserConfig::builder()
        .spawner(Rc::clone(&busy) as Rc<dyn Spawner>)
        .memory(Rc::new(AtOnce(projects)) as Rc<dyn BrowserMemory>)
        .build();
    let state = BrowserState::new(Box::new(home), config);
    BrowserState::restore_location(&state, "key");
    assert_eq!(state.borrow().path_names(), ["Home", "Projects"]);
    assert!(!state.borrow().is_restoring());
}

#[test]
fn a_remembering_browser_restores_as_it_is_made_and_late_listeners_hear_it() {
    agg_gui::set_device_scale(1.0);
    let home = MemoryFolder::new("Home");
    let projects = home.add_folder("Projects").expect("free");
    let inline = Rc::new(Inline::default());
    let config = BrowserConfig::builder()
        .spawner(Rc::clone(&inline) as Rc<dyn Spawner>)
        .memory(Rc::new(AtOnce(projects)) as Rc<dyn BrowserMemory>)
        .build();
    let state = BrowserState::new(Box::new(home), config);
    let _browser = FileBrowser::new(&state, font()).remembering("key");
    assert_eq!(
        state.borrow().path_names(),
        ["Home", "Projects"],
        "the restore never waits for the browser to be shown"
    );
    let heard = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&heard);
    state
        .borrow_mut()
        .add_restore_finished(Rc::new(move |outcome| sink.borrow_mut().push(outcome)));
    assert!(heard.borrow().is_empty(), "never told inside the call");
    inline.turn();
    assert_eq!(*heard.borrow(), [RestoreOutcome::Landed]);
}

/// A memory naming a trail whose answer never comes.
struct Never;

impl BrowserMemory for Never {
    fn last_location(&self, _key: &str) -> Option<LocationFuture> {
        Some(Box::pin(std::future::pending()))
    }
    fn last_location_trail(&self, _key: &str) -> Option<Vec<String>> {
        Some(vec!["Home".into(), "Old".into()])
    }
    fn remember_location(&self, _key: &str, _path: &[OpenFolder]) {}
}

#[test]
fn a_browser_already_off_its_root_shows_no_remembered_trail() {
    let home = MemoryFolder::new("Home");
    let projects = home.add_folder("Projects").expect("free");
    let config = BrowserConfig::builder()
        .spawner(Rc::new(Busy::default()) as Rc<dyn Spawner>)
        .memory(Rc::new(Never) as Rc<dyn BrowserMemory>)
        .build();
    let state = BrowserState::new(Box::new(home), config);
    state.borrow_mut().push_container(Box::new(projects));
    BrowserState::restore_location(&state, "key");
    assert!(!state.borrow().is_restoring());
    assert_eq!(state.borrow().shown_trail(), ["Home", "Projects"]);
}

/// A spawner that polls each task as it is given it, and again at each `turn`.
#[derive(Default)]
struct Inline(RefCell<Vec<LocalTask>>);

impl Inline {
    fn poll(mut task: LocalTask) -> Option<LocalTask> {
        let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
        match task.as_mut().poll(&mut cx) {
            std::task::Poll::Ready(()) => None,
            std::task::Poll::Pending => Some(task),
        }
    }

    /// The spawner's next turn: every waiting task polled again.
    fn turn(&self) {
        let tasks = std::mem::take(&mut *self.0.borrow_mut());
        let left: Vec<LocalTask> = tasks.into_iter().filter_map(Self::poll).collect();
        self.0.borrow_mut().extend(left);
    }
}

impl Spawner for Inline {
    fn spawn_task(&self, task: LocalTask) {
        if let Some(task) = Self::poll(task) {
            self.0.borrow_mut().push(task);
        }
    }
    fn report_fault(&self, _error: Arc<dyn std::error::Error + Send + Sync>) {}
}

/// Waits one turn, waking itself.
struct Yield(bool);

impl std::future::Future for Yield {
    type Output = ();
    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<()> {
        if self.0 {
            return std::task::Poll::Ready(());
        }
        self.0 = true;
        cx.waker().wake_by_ref();
        std::task::Poll::Pending
    }
}

/// A memory whose answer yields once before finding Projects.
struct AfterAYield(MemoryFolder);

impl BrowserMemory for AfterAYield {
    fn last_location(&self, _key: &str) -> Option<LocationFuture> {
        let found: Vec<Box<dyn Folder>> = vec![Box::new(self.0.clone())];
        Some(Box::pin(async move {
            Yield(false).await;
            Some(found)
        }))
    }
    fn remember_location(&self, _key: &str, _path: &[OpenFolder]) {}
}

#[test]
fn an_answer_that_yields_gets_its_turn_from_a_spawner_that_polls_at_once() {
    let home = MemoryFolder::new("Home");
    let projects = home.add_folder("Projects").expect("free");
    let inline = Rc::new(Inline::default());
    let config = BrowserConfig::builder()
        .spawner(Rc::clone(&inline) as Rc<dyn Spawner>)
        .memory(Rc::new(AfterAYield(projects)) as Rc<dyn BrowserMemory>)
        .build();
    let state = BrowserState::new(Box::new(home), config);
    BrowserState::restore_location(&state, "key");
    assert_eq!(
        state.borrow().path_names(),
        ["Home"],
        "the answer asked to wait a turn: it never lands inside the call"
    );
    inline.turn();
    assert_eq!(state.borrow().path_names(), ["Home", "Projects"]);
}

/// The default look, answering its own widget for a folder's message.
struct OwnMessages(BrowserNames, Rc<Cell<usize>>);

impl BrowserLook for OwnMessages {
    fn style(&self) -> BrowserStyle {
        BrowserStyle::default()
    }
    fn names(&self) -> &BrowserNames {
        &self.0
    }
    fn folder_message(
        &self,
        _state: &Rc<RefCell<BrowserState>>,
        message: &ListingMessage,
        _font: &Arc<Font>,
    ) -> Option<Box<dyn Widget>> {
        assert!(message.loading, "the folder is loading");
        self.1.set(self.1.get() + 1);
        Some(Box::new(Named::new(OWN_MESSAGE, Box::new(SizedBox::new()))))
    }
}

/// A folder still loading: its listing is the loading message alone.
struct Loading;

impl Folder for Loading {
    fn id(&self) -> &str {
        "loading"
    }
    fn name(&self) -> &str {
        "Home"
    }
    fn is_protected(&self) -> bool {
        true
    }
    fn child_containers(&self) -> Vec<Rc<dyn FolderLink>> {
        Vec::new()
    }
    fn items(&self) -> Vec<Rc<dyn Item>> {
        vec![Rc::new(MessageItem::loading())]
    }
    fn load(&mut self) {}
}

#[test]
fn the_looks_folder_message_stands_in_the_lists_place() {
    agg_gui::set_device_scale(1.0);
    let home = Loading;
    let asked = Rc::new(Cell::new(0));
    let look = OwnMessages(BrowserNames::default(), Rc::clone(&asked));
    let config = BrowserConfig::builder().look(Rc::new(look)).build();
    let state = BrowserState::new(Box::new(home), config);
    let mut list = BrowserList::new(&state, font());
    list.layout(Size::new(400.0, 300.0));
    assert_eq!(asked.get(), 1);
    assert!(find_widget_by_id(&list, OWN_MESSAGE).is_some());
}

#[test]
fn a_page_is_found_by_its_type_and_scrolls_by_the_hosts_cell() {
    agg_gui::set_device_scale(1.0);
    let home = MemoryFolder::new("Home");
    for n in 0..60 {
        home.add_file(&format!("part {n:02}.stl"), b"p".to_vec());
    }
    let state = BrowserState::new(Box::new(home), BrowserConfig::builder().build());
    let offset = Rc::new(Cell::new(0.0));
    let mut page = BrowserPage::new(&state, font()).with_offset_cell(Rc::clone(&offset));
    assert!(page.as_any().is_some_and(|a| a.is::<BrowserPage>()));
    page.layout(Size::new(400.0, 200.0));
    offset.set(120.0);
    page.layout(Size::new(400.0, 200.0));
    assert_eq!(offset.get(), 120.0, "the host's place in the list is kept");
}
