//! The Save and Move dialogs' controllers under an app's own pages: minimal pages built in this
//! test (a field, buttons named as an app names them, agg-gui's browser page for the list) over
//! `SaveController` and `MoveController`, driven through agg-gui's `App` with real pointer and
//! key events, as an app with its own dialog chrome uses them.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::text::Font;
use agg_gui::{
    App, Button, DrawCtx, Event, EventResult, Modifiers, MouseButton, Point, Rect, Size, Stack,
    TextField, Widget,
};
use agg_gui_file_browser::{
    dialog_options, run_ready_local_tasks, BrowserConfig, BrowserDialogs, BrowserPage,
    BrowserState, MoveAnswers, MoveController, MovePick, MoveRequest, NameRequest, ReplaceAnswer,
    ReplaceQuestion, SaveAnswers, SaveController, SaveMode, SavePick, SaveRequest,
};
use agg_gui_files::{DynamicFolder, DynamicFolderLink, Folder, MemoryFolder};

const WIDTH: f64 = 800.0;
const HEIGHT: f64 = 600.0;
const FIELD: &str = "Design Name Edit Field";
const ACCEPT: &str = "Accept Button";
const CANCEL: &str = "Cancel Button";
const CREATE: &str = "Create Folder Button";
const PAGE: &str = "App Dialog Page";

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(agg_gui::fonts::NOTO_SANS_REGULAR).expect("bundled font loads"))
}

/// The top: Designs (holding report.txt and gear.stl), Drafts and Cloud, all writable.
#[derive(Clone)]
struct Sample {
    designs: MemoryFolder,
    drafts: MemoryFolder,
    cloud: MemoryFolder,
}

impl Sample {
    fn new() -> Self {
        let designs = MemoryFolder::new("Designs");
        designs.add_file("report.txt", b"r".to_vec());
        designs.add_file("gear.stl", b"g".to_vec());
        Self {
            designs,
            drafts: MemoryFolder::new("Drafts"),
            cloud: MemoryFolder::new("Cloud"),
        }
    }

    fn root(&self) -> Box<dyn Folder> {
        let mut root = DynamicFolder::new("Top");
        root.data.child_containers = [&self.designs, &self.drafts, &self.cloud]
            .into_iter()
            .map(|folder| {
                let folder = folder.clone();
                let name = folder.name().to_string();
                Rc::new(DynamicFolderLink::new(
                    name,
                    Rc::new(move || Box::new(folder.clone()) as Box<dyn Folder>),
                )) as Rc<dyn agg_gui_files::FolderLink>
            })
            .collect();
        Box::new(root)
    }

    fn browser(&self) -> Rc<RefCell<BrowserState>> {
        agg_gui::set_device_scale(1.0);
        let config = BrowserConfig::builder().options(dialog_options()).build();
        BrowserState::new(self.root(), config)
    }
}

/// An app's own dialog page: its widgets laid out top to bottom (the browser page filling what
/// is left), the controller's `update` run before the browser lays out, and the properties the
/// app reports.
struct AppPage {
    update: Box<dyn Fn()>,
    report: Box<dyn Fn() -> Vec<(&'static str, String)>>,
    bounds: Rect,
    /// The browser page first, then the rest.
    children: Vec<Box<dyn Widget>>,
}

impl Widget for AppPage {
    fn type_name(&self) -> &'static str {
        "AppPage"
    }
    fn id(&self) -> Option<&str> {
        Some(PAGE)
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
        (self.update)();
        let mut top = available.height;
        for child in self.children.iter_mut().skip(1) {
            let size = child.layout(Size::new(available.width, 40.0));
            top -= 40.0;
            child.set_bounds(Rect::new(0.0, top, size.width.max(200.0), 40.0));
        }
        let room = Size::new(available.width, top.max(0.0));
        self.children[0].layout(room);
        self.children[0].set_bounds(Rect::new(0.0, 0.0, room.width, room.height));
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
    fn properties(&self) -> Vec<(&'static str, String)> {
        (self.report)()
    }
}

/// The app's Save As page over `c`: its name field (focus id 77, its own hint), Save, Cancel
/// and Create Folder.
fn save_page(c: &SaveController) -> AppPage {
    let (editing, entering, accepting, enabled, cancelling, creating) = (
        c.clone(),
        c.clone(),
        c.clone(),
        c.clone(),
        c.clone(),
        c.clone(),
    );
    let mut field = TextField::new(font())
        .with_text_cell(c.name_cell())
        .on_change(move |_| editing.edited())
        .on_enter(move |_| entering.accept())
        .with_name(FIELD);
    if let Some(hint) = c.placeholder() {
        field = field.with_placeholder(hint.to_string());
    }
    if let Some(id) = c.request().focus_id {
        field = field.with_focus_id(id);
    }
    let accept = Button::new("Save", font())
        .on_click(move || accepting.accept())
        .with_enabled_fn(move || enabled.accept_enabled())
        .with_name(ACCEPT);
    let cancel = Button::new("Cancel", font())
        .on_click(move || cancelling.cancel())
        .with_name(CANCEL);
    let create = Button::new("Create Folder", font())
        .on_click(move || creating.create_folder())
        .with_name(CREATE);
    let (updating, reporting) = (c.clone(), c.clone());
    AppPage {
        update: Box::new(move || updating.update()),
        report: Box::new(move || {
            vec![
                ("design_name", reporting.name()),
                (
                    "design_name_hint",
                    reporting.placeholder().unwrap_or_default().to_string(),
                ),
                ("accept_enabled", reporting.accept_enabled().to_string()),
            ]
        }),
        bounds: Rect::default(),
        children: vec![
            Box::new(BrowserPage::new(c.state(), font())),
            Box::new(field),
            Box::new(accept),
            Box::new(cancel),
            Box::new(create),
        ],
    }
}

type Picked = Rc<RefCell<Option<Option<SavePick>>>>;

fn save_answers() -> (SaveAnswers, Picked) {
    let picked: Picked = Rc::new(RefCell::new(None));
    let (accepting, cancelling) = (Rc::clone(&picked), Rc::clone(&picked));
    let answers = SaveAnswers {
        accept: Rc::new(move |pick| *accepting.borrow_mut() = Some(Some(pick))),
        cancel: Rc::new(move || *cancelling.borrow_mut() = Some(None)),
    };
    (answers, picked)
}

fn app_over(page: AppPage) -> App {
    let mut app = App::new(Box::new(Stack::new().add(Box::new(page))));
    layout(&mut app);
    for _ in 0..20 {
        run_ready_local_tasks();
    }
    layout(&mut app);
    app
}

fn layout(app: &mut App) {
    app.layout(Size::new(WIDTH, HEIGHT));
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

fn property(app: &App, key: &str) -> String {
    let (page, _) = find(app.root(), PAGE, Point::new(0.0, 0.0)).expect("the page shows");
    page.properties()
        .into_iter()
        .find(|(k, _)| *k == key)
        .map(|(_, v)| v)
        .unwrap_or_default()
}

fn click(app: &mut App, id: &str, clicks: u32) {
    let (_, r) =
        find(app.root(), id, Point::new(0.0, 0.0)).unwrap_or_else(|| panic!("no {id} shows"));
    let at = Point::new(r.x + r.width / 2.0, HEIGHT - (r.y + r.height / 2.0));
    app.on_mouse_move(at.x, at.y);
    for n in 1..=clicks {
        app.on_mouse_down_clicks(at.x, at.y, MouseButton::Left, Modifiers::default(), n);
        app.on_mouse_up(at.x, at.y, MouseButton::Left, Modifiers::default());
        layout(app);
    }
}

fn type_into(app: &mut App, id: &str, text: &str) {
    click(app, id, 1);
    app.on_text_input(text);
    layout(app);
}

fn trail(c: &SaveController) -> String {
    c.state().borrow().path_names().join("/")
}

fn pick_of(picked: &Picked) -> (String, String, bool) {
    let pick = picked.borrow_mut().take().expect("answered");
    let pick = pick.expect("not cancelled");
    (
        pick.folder.name().to_string(),
        pick.file_name,
        pick.replaces,
    )
}

#[test]
fn typing_and_row_clicks_name_the_save_and_save_answers_the_folder_and_name() {
    let sample = Sample::new();
    let (answers, picked) = save_answers();
    let request = SaveRequest::new("").with_extension(".txt");
    let c = SaveController::new(&sample.browser(), request, answers);
    let mut app = app_over(save_page(&c));
    assert_eq!(
        property(&app, "accept_enabled"),
        "false",
        "the top takes no file"
    );
    click(&mut app, "Designs Row Item Collection", 2);
    assert_eq!(trail(&c), "Top/Designs");
    assert_eq!(property(&app, "accept_enabled"), "false", "no name yet");
    type_into(&mut app, FIELD, "notes");
    assert_eq!(property(&app, "design_name"), "notes");
    assert_eq!(property(&app, "accept_enabled"), "true");
    click(&mut app, "Row Item gear.stl", 1);
    assert_eq!(
        property(&app, "design_name"),
        "gear.stl",
        "a clicked file's name, as the request shows it"
    );
    type_into(&mut app, FIELD, "x");
    click(&mut app, "Row Item report.txt", 1);
    assert_eq!(property(&app, "design_name"), "report");
    type_into(&mut app, FIELD, "s");
    click(&mut app, ACCEPT, 1);
    assert_eq!(
        pick_of(&picked),
        ("Designs".into(), "reports.txt".into(), false)
    );
}

#[test]
fn the_apps_folder_rule_disables_save_up_front_with_its_reason() {
    let sample = Sample::new();
    let (answers, picked) = save_answers();
    let c = SaveController::new(&sample.browser(), SaveRequest::new("notes"), answers)
        .with_folder_rule(Rc::new(|folder: &dyn Folder| match folder.name() {
            "Cloud" => Err("Sign in to save here.".to_string()),
            _ => Ok(()),
        }));
    let mut app = app_over(save_page(&c));
    assert_eq!(
        c.refusal().as_deref(),
        Some("Choose a folder you can save in.")
    );
    click(&mut app, "Cloud Row Item Collection", 2);
    assert_eq!(trail(&c), "Top/Cloud");
    assert_eq!(
        property(&app, "accept_enabled"),
        "false",
        "refused up front"
    );
    assert_eq!(c.refusal().as_deref(), Some("Sign in to save here."));
    click(&mut app, ACCEPT, 1);
    click(&mut app, FIELD, 1);
    app.on_key_down(agg_gui::Key::Enter, Modifiers::default());
    layout(&mut app);
    assert!(picked.borrow().is_none(), "nothing saved");
    c.state().borrow_mut().go_to(0);
    layout(&mut app);
    click(&mut app, "Designs Row Item Collection", 2);
    assert_eq!(property(&app, "accept_enabled"), "true");
    assert_eq!(c.refusal(), None);
    click(&mut app, ACCEPT, 1);
    assert_eq!(pick_of(&picked), ("Designs".into(), "notes".into(), false));
}

/// The question the app's host was asked, and the answer it holds until the user picks.
type Asked = Rc<RefCell<Vec<(ReplaceQuestion, Box<dyn FnOnce(bool)>)>>>;

#[test]
fn the_question_host_hears_the_replace_question_and_confirming_saves() {
    let sample = Sample::new();
    let (answers, picked) = save_answers();
    let asked: Asked = Rc::default();
    let host = Rc::clone(&asked);
    let c = SaveController::new(
        &sample.browser(),
        SaveRequest::new("report").with_extension(".txt"),
        answers,
    )
    .with_question_host(Rc::new(move |question, answer| {
        host.borrow_mut().push((question, answer));
    }));
    let mut app = app_over(save_page(&c));
    click(&mut app, "Designs Row Item Collection", 2);
    click(&mut app, ACCEPT, 1);
    let (question, answer) = asked.borrow_mut().pop().expect("the host was asked");
    assert_eq!(question, ReplaceQuestion::replace_existing("report"));
    assert_eq!(
        c.properties()[3],
        ("question", "normal".to_string()),
        "not asked in the page"
    );
    answer(false);
    layout(&mut app);
    assert!(picked.borrow().is_none(), "cancelling stays in the dialog");
    // A double click on the file asks again; confirming saves over it.
    click(&mut app, "Row Item report.txt", 2);
    let (_, answer) = asked.borrow_mut().pop().expect("asked again");
    answer(true);
    assert_eq!(
        pick_of(&picked),
        ("Designs".into(), "report.txt".into(), true)
    );
}

/// An app's dialogs whose input box answers every name question with `answer`, keeping each
/// request asked.
struct InputBox {
    answer: Option<String>,
    asked: RefCell<Vec<NameRequest>>,
}

impl BrowserDialogs for InputBox {
    fn ask_name(&self, request: NameRequest, answer: Box<dyn FnOnce(Option<String>)>) {
        self.asked.borrow_mut().push(request);
        answer(self.answer.clone());
    }
}

#[test]
fn create_folder_asks_through_the_apps_input_box_and_opens_the_folder() {
    let sample = Sample::new();
    let (answers, picked) = save_answers();
    let input = Rc::new(InputBox {
        answer: Some("Brackets".into()),
        asked: RefCell::default(),
    });
    let c = SaveController::new(&sample.browser(), SaveRequest::new("notes"), answers)
        .asking_names_through(Rc::clone(&input) as Rc<dyn BrowserDialogs>);
    let mut app = app_over(save_page(&c));
    click(&mut app, CREATE, 1);
    assert!(input.asked.borrow().is_empty(), "the top takes no folder");
    click(&mut app, "Designs Row Item Collection", 2);
    click(&mut app, CREATE, 1);
    assert_eq!(
        *input.asked.borrow(),
        [NameRequest::new(
            "Create Folder",
            "Folder Name",
            "",
            "Create"
        )]
    );
    let made: Vec<String> = sample
        .designs
        .child_containers()
        .iter()
        .map(|l| l.name().to_string())
        .collect();
    assert_eq!(made, ["Brackets"]);
    assert_eq!(trail(&c), "Top/Designs/Brackets");
    click(&mut app, ACCEPT, 1);
    assert_eq!(pick_of(&picked), ("Brackets".into(), "notes".into(), false));
}

#[test]
fn create_folder_can_keep_the_dialog_in_the_folder_showing_the_new_one_listed() {
    let sample = Sample::new();
    let (answers, picked) = save_answers();
    let input = Rc::new(InputBox {
        answer: Some("Brackets".into()),
        asked: RefCell::default(),
    });
    let c = SaveController::new(&sample.browser(), SaveRequest::new("notes"), answers)
        .asking_names_through(Rc::clone(&input) as Rc<dyn BrowserDialogs>)
        .keeping_folder_on_create();
    let mut app = app_over(save_page(&c));
    click(&mut app, "Designs Row Item Collection", 2);
    click(&mut app, CREATE, 1);
    let made: Vec<String> = sample
        .designs
        .child_containers()
        .iter()
        .map(|l| l.name().to_string())
        .collect();
    assert_eq!(made, ["Brackets"]);
    assert_eq!(trail(&c), "Top/Designs", "the dialog stays where it was");
    let listed = c
        .state()
        .borrow()
        .entries()
        .iter()
        .any(|e| e.name() == "Brackets");
    assert!(listed, "the new folder is listed in the folder showing");
    click(&mut app, ACCEPT, 1);
    assert_eq!(pick_of(&picked), ("Designs".into(), "notes".into(), false));
}

#[test]
fn a_dialog_typing_into_a_kept_cell_starts_with_its_text_and_fills_it() {
    let sample = Sample::new();
    let kept = Rc::new(RefCell::new("typed before".to_string()));
    let (answers, picked) = save_answers();
    let c = SaveController::new(&sample.browser(), SaveRequest::new("ignored"), answers)
        .typing_into(Rc::clone(&kept));
    let mut app = app_over(save_page(&c));
    assert_eq!(property(&app, "design_name"), "typed before");
    click(&mut app, "Designs Row Item Collection", 2);
    click(&mut app, "Row Item gear.stl", 1);
    assert_eq!(
        *kept.borrow(),
        "gear.stl",
        "a row's click fills the kept cell"
    );
    // A page built again over a new controller sharing the cell shows the same name.
    let (again, _) = save_answers();
    let rebuilt = SaveController::new(&sample.browser(), SaveRequest::new(""), again)
        .typing_into(Rc::clone(&kept));
    assert_eq!(rebuilt.name(), "gear.stl");
    // Saving takes the kept cell's name: over gear.stl, once the user agrees to replace it.
    click(&mut app, ACCEPT, 1);
    assert!(matches!(c.mode(), SaveMode::Asking { file_name, .. } if file_name == "gear.stl"));
    c.confirm();
    assert_eq!(
        pick_of(&picked),
        ("Designs".into(), "gear.stl".into(), true)
    );
}

#[test]
fn a_starting_folder_opens_the_dialog_there_with_the_field_focused_and_hinted() {
    let sample = Sample::new();
    let (answers, picked) = save_answers();
    let drafts = sample.drafts.clone();
    let request = SaveRequest::new("")
        .with_placeholder("Enter a Design Name Here")
        .with_focus_id(77);
    let c = SaveController::new(&sample.browser(), request, answers).starting_in(Box::pin(
        async move { Some(vec![Box::new(drafts) as Box<dyn Folder>]) },
    ));
    let mut app = app_over(save_page(&c));
    assert_eq!(trail(&c), "Top/Drafts");
    assert_eq!(
        property(&app, "design_name_hint"),
        "Enter a Design Name Here"
    );
    // Typing straight away names the save: the field took focus as the dialog opened.
    app.on_text_input("plate");
    layout(&mut app);
    assert_eq!(property(&app, "design_name"), "plate");
    app.on_key_down(agg_gui::Key::Enter, Modifiers::default());
    layout(&mut app);
    assert_eq!(pick_of(&picked), ("Drafts".into(), "plate".into(), false));
}

#[test]
fn move_accept_from_an_apps_page_answers_the_picked_folder() {
    let sample = Sample::new();
    let state = sample.browser();
    let gear = sample
        .designs
        .items()
        .into_iter()
        .find(|i| i.name() == "gear.stl")
        .expect("gear.stl");
    let picked: Rc<RefCell<Option<MovePick>>> = Rc::default();
    let accepting = Rc::clone(&picked);
    let answers = MoveAnswers {
        accept: Rc::new(move |pick| *accepting.borrow_mut() = Some(pick)),
        cancel: Rc::new(|| {}),
    };
    let request =
        MoveRequest::new(vec![gear], None).with_source("Designs", None, Rc::new(|_| false));
    let c = MoveController::new(&state, request, answers);
    let (accepting, enabled, updating, reporting) = (c.clone(), c.clone(), c.clone(), c.clone());
    let accept = Button::new("Move", font())
        .on_click(move || accepting.accept())
        .with_enabled_fn(move || enabled.accept_enabled())
        .with_name(ACCEPT);
    let page = AppPage {
        update: Box::new(move || updating.update()),
        report: Box::new(move || reporting.properties()),
        bounds: Rect::default(),
        children: vec![Box::new(BrowserPage::new(&state, font())), Box::new(accept)],
    };
    let mut app = app_over(page);
    assert_eq!(
        property(&app, "accept_enabled"),
        "false",
        "the top takes nothing"
    );
    click(&mut app, ACCEPT, 1);
    assert!(picked.borrow().is_none());
    click(&mut app, "Drafts Row Item Collection", 1);
    assert_eq!(property(&app, "accept_enabled"), "true");
    assert_eq!(property(&app, "accept_text"), "Move");
    assert_eq!(c.summary(), property(&app, "summary"));
    assert!(c.summary().contains("Drafts"), "{}", c.summary());
    click(&mut app, ACCEPT, 1);
    let pick = picked.borrow_mut().take().expect("answered");
    assert_eq!((pick.target.name(), pick.copy), ("Drafts", false));
    assert!(c.is_accepted());
}

#[test]
fn an_app_answering_save_beside_a_name_in_another_case_saves_without_asking() {
    let sample = Sample::new();
    let (answers, picked) = save_answers();
    let heard = Rc::new(RefCell::new(None));
    let hook_heard = Rc::clone(&heard);
    // A volume that tells names apart by case: Report.txt is not report.txt.
    let request = SaveRequest::new("Report")
        .with_extension(".txt")
        .with_replace_answer(Rc::new(move |target| {
            *hook_heard.borrow_mut() = Some(target.file_name.to_string());
            ReplaceAnswer::Save
        }));
    let c = SaveController::new(&sample.browser(), request, answers);
    let mut app = app_over(save_page(&c));
    click(&mut app, "Designs Row Item Collection", 2);
    click(&mut app, ACCEPT, 1);
    assert_eq!(heard.borrow().as_deref(), Some("Report.txt"));
    assert_eq!(c.mode(), SaveMode::Normal, "no question is asked");
    assert_eq!(
        pick_of(&picked),
        ("Designs".into(), "Report.txt".into(), false),
        "the save goes ahead, replacing nothing"
    );
}

#[test]
fn an_app_answering_ask_asks_its_question_even_with_nothing_listed() {
    let sample = Sample::new();
    let (answers, picked) = save_answers();
    let question = ReplaceQuestion::new("Check", "Sure?", "Go");
    let asked = question.clone();
    let request = SaveRequest::new("fresh")
        .with_replace_answer(Rc::new(move |_| ReplaceAnswer::Ask(asked.clone())));
    let c = SaveController::new(&sample.browser(), request, answers);
    let mut app = app_over(save_page(&c));
    click(&mut app, "Designs Row Item Collection", 2);
    click(&mut app, ACCEPT, 1);
    assert_eq!(
        c.mode(),
        SaveMode::Asking {
            question,
            file_name: "fresh".into()
        }
    );
    assert!(picked.borrow().is_none());
    c.confirm();
    assert_eq!(pick_of(&picked), ("Designs".into(), "fresh".into(), false));
}

#[test]
fn a_name_differing_only_in_case_asks_to_replace_and_the_hook_hears_it_exists() {
    let sample = Sample::new();
    let (answers, picked) = save_answers();
    let heard = Rc::new(RefCell::new(None));
    let hook_heard = Rc::clone(&heard);
    let request = SaveRequest::new("Report")
        .with_extension(".txt")
        .with_replace_question(Rc::new(move |target| {
            *hook_heard.borrow_mut() = Some(target.exists);
            None
        }));
    let c = SaveController::new(&sample.browser(), request, answers);
    let mut app = app_over(save_page(&c));
    click(&mut app, "Designs Row Item Collection", 2);
    click(&mut app, ACCEPT, 1);
    assert_eq!(*heard.borrow(), Some(true), "Report.txt is report.txt");
    assert!(
        matches!(c.mode(), SaveMode::Asking { .. }),
        "the replace question is asked"
    );
    assert!(picked.borrow().is_none());
    c.confirm();
    assert_eq!(
        pick_of(&picked),
        ("Designs".into(), "Report.txt".into(), true)
    );
}

#[test]
fn the_replace_question_asked_in_the_page_ends_when_the_folder_changes() {
    let sample = Sample::new();
    sample.drafts.add_file("x.txt", b"x".to_vec());
    let (answers, picked) = save_answers();
    let c = SaveController::new(
        &sample.browser(),
        SaveRequest::new("report").with_extension(".txt"),
        answers,
    );
    let mut app = app_over(save_page(&c));
    click(&mut app, "Designs Row Item Collection", 2);
    click(&mut app, ACCEPT, 1);
    assert!(matches!(c.mode(), SaveMode::Asking { .. }));
    // Navigating away before the next layout: confirming saves nothing in the other folder.
    c.state().borrow_mut().go_to(0);
    let drafts = c.state().borrow().root_child_containers()[1].clone();
    c.state().borrow_mut().open(drafts.as_ref());
    c.confirm();
    assert!(picked.borrow().is_none(), "Drafts was never asked about");
    // And the page's next layout takes the question down.
    layout(&mut app);
    assert_eq!(c.mode(), SaveMode::Normal);
    assert!(picked.borrow().is_none());
}

#[test]
fn the_question_hosts_answer_saves_nothing_once_the_folder_changed() {
    let sample = Sample::new();
    let (answers, picked) = save_answers();
    let asked: Asked = Rc::default();
    let host = Rc::clone(&asked);
    let c = SaveController::new(
        &sample.browser(),
        SaveRequest::new("report").with_extension(".txt"),
        answers,
    )
    .with_question_host(Rc::new(move |question, answer| {
        host.borrow_mut().push((question, answer));
    }));
    let mut app = app_over(save_page(&c));
    click(&mut app, "Designs Row Item Collection", 2);
    click(&mut app, ACCEPT, 1);
    let (_, answer) = asked.borrow_mut().pop().expect("the host was asked");
    c.state().borrow_mut().go_to(0);
    let drafts = c.state().borrow().root_child_containers()[1].clone();
    c.state().borrow_mut().open(drafts.as_ref());
    answer(true);
    assert!(picked.borrow().is_none(), "Drafts was never asked about");
}
