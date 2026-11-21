//! Drops the app takes or refuses itself (`PlaceRules::app_drop`, `run_app_drop`), through
//! agg-gui's `App` with real pointer events over a folder tree beside the list (`drag_support`):
//! a read-only folder the app accepts the drop on is highlighted with the app's label and runs
//! the app's drop on release, moving and copying nothing; a folder the app refuses is never
//! highlighted, holds its reason while the drag is over it, and reports it on release; the app is
//! asked once per folder per drag; and without the app's report a failed drop stays on the
//! browser's status line after it lists again.

mod drag_support;

use drag_support::*;

use agg_gui_file_browser::AppDrop;

/// Rules that answer `answer` for a drop on Shelf (every other folder left to the move-or-copy
/// rule), noting each question and each drop they run, and failing a run with `fails`.
struct ShelfRules {
    answer: AppDrop,
    fails: Option<String>,
    asked: RefCell<Vec<String>>,
    ran: RefCell<Vec<String>>,
}

impl ShelfRules {
    fn new(answer: AppDrop) -> Rc<Self> {
        Rc::new(Self {
            answer,
            fails: None,
            asked: RefCell::new(Vec::new()),
            ran: RefCell::new(Vec::new()),
        })
    }
}

impl PlaceRules for ShelfRules {
    fn app_drop(&self, link: &dyn FolderLink, _items: &[Rc<dyn Item>]) -> AppDrop {
        self.asked.borrow_mut().push(link.name().to_string());
        match link.name() {
            "Shelf" => self.answer.clone(),
            _ => AppDrop::NotApp,
        }
    }

    fn run_app_drop(&self, link: &dyn FolderLink, items: &[Rc<dyn Item>]) -> Result<(), String> {
        let names: Vec<&str> = items.iter().map(|i| i.name()).collect();
        self.ran
            .borrow_mut()
            .push(format!("{} on {}", names.join(","), link.name()));
        match &self.fails {
            Some(why) => Err(why.clone()),
            None => Ok(()),
        }
    }
}

/// A config with `rules`, its reports gathered in the answer's list.
fn config_with(rules: Rc<ShelfRules>) -> (BrowserConfig, Rc<RefCell<Vec<String>>>) {
    let reported = Rc::new(RefCell::new(Vec::<String>::new()));
    let report = Rc::clone(&reported);
    let config = BrowserConfig::builder()
        .places(rules)
        .report(Rc::new(move |m: &str| {
            report.borrow_mut().push(m.to_string())
        }))
        .build();
    (config, reported)
}

#[test]
fn a_read_only_folder_the_app_accepts_is_labelled_its_way_and_runs_the_apps_drop() {
    let rules = ShelfRules::new(AppDrop::Accepts {
        label: "Add to Shelf".to_string(),
    });
    let (config, reported) = config_with(rules.clone());
    let (mut app, state, desk, shelf) = two_places(config, true);
    let at = drag_over(&mut app, "Row Item gear.stl", "Shelf Tree Node");
    assert_eq!(
        property(&app, "Shelf Tree Node", "drop_highlighted"),
        "true"
    );
    assert_eq!(
        property(&app, "Shelf Tree Node", "drop_label"),
        "Add to Shelf"
    );
    assert_eq!(state.borrow().drop_manager().refusal(), None);
    assert_eq!(
        rules
            .asked
            .borrow()
            .iter()
            .filter(|n| *n == "Shelf")
            .count(),
        1,
        "asked once per folder per drag, however many moves"
    );
    release(&mut app, at, Modifiers::default());
    assert_eq!(*rules.ran.borrow(), ["gear.stl on Shelf"]);
    assert_eq!(names(&desk), ["gear.stl"], "nothing moved");
    assert!(names(&shelf).is_empty(), "nothing copied");
    assert!(reported.borrow().is_empty());
    assert_eq!(state.borrow().history().undo_description(), None);
}

#[test]
fn an_app_drop_that_fails_is_reported() {
    let rules = Rc::new(ShelfRules {
        fails: Some("The shelf is full".to_string()),
        ..Rc::into_inner(ShelfRules::new(AppDrop::Accepts {
            label: "Add to Shelf".to_string(),
        }))
        .expect("one owner")
    });
    let (config, reported) = config_with(rules.clone());
    let (mut app, _state, desk, _shelf) = two_places(config, true);
    let at = drag_over(&mut app, "Row Item gear.stl", "Shelf Tree Node");
    release(&mut app, at, Modifiers::default());
    assert_eq!(*rules.ran.borrow(), ["gear.stl on Shelf"]);
    assert_eq!(*reported.borrow(), ["The shelf is full"]);
    assert_eq!(names(&desk), ["gear.stl"]);
}

/// Rules that take every move and fail it.
struct FailingMoves;

impl PlaceRules for FailingMoves {
    fn take_over_move<'a>(&self, _cx: MoveCx<'a>) -> MoveTakeover<'a> {
        MoveTakeover::Taken(Err("the disk is full".to_string()))
    }
}

#[test]
fn without_the_apps_report_a_failed_drop_stays_on_the_status_line() {
    let rules = Rc::new(ShelfRules {
        fails: Some("The shelf is full".to_string()),
        ..Rc::into_inner(ShelfRules::new(AppDrop::Accepts {
            label: "Add to Shelf".to_string(),
        }))
        .expect("one owner")
    });
    let config = BrowserConfig::builder().places(rules.clone()).build();
    let (mut app, state, _desk, _shelf) = two_places(config, true);
    let at = drag_over(&mut app, "Row Item gear.stl", "Shelf Tree Node");
    release(&mut app, at, Modifiers::default());
    assert_eq!(*rules.ran.borrow(), ["gear.stl on Shelf"]);
    assert_eq!(
        state.borrow().status(),
        "The shelf is full",
        "listing again after the drop does not wipe the failure"
    );

    // A move that fails says so the same way.
    let dir = workshop("app-drop-failed-move-status");
    let config = BrowserConfig::builder()
        .places(Rc::new(FailingMoves))
        .build();
    let (mut app, state) = browser_app(disk(&dir), config, ViewMode::Rows);
    let at = drag_over(&mut app, "Row Item gear.stl", "Parts Tree Node");
    release(&mut app, at, Modifiers::default());
    assert_eq!(
        state.borrow().status(),
        "Couldn't move to \"Parts\": the disk is full"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_folder_the_app_refuses_is_never_a_target_and_its_reason_is_reported_on_release() {
    for read_only in [true, false] {
        let rules = ShelfRules::new(AppDrop::Refuses {
            reason: "Only tools go on the shelf".to_string(),
        });
        let (config, reported) = config_with(rules.clone());
        let (mut app, state, desk, shelf) = two_places(config, read_only);
        let at = drag_over(&mut app, "Row Item gear.stl", "Shelf Tree Node");
        assert_eq!(
            property(&app, "Shelf Tree Node", "drop_highlighted"),
            "false"
        );
        assert_eq!(property(&app, "Shelf Tree Node", "drop_label"), "");
        assert_eq!(
            state.borrow().drop_manager().refusal(),
            Some("Only tools go on the shelf"),
            "the reason is there while the drag is over the folder"
        );
        release(&mut app, at, Modifiers::default());
        assert_eq!(*reported.borrow(), ["Only tools go on the shelf"]);
        assert!(rules.ran.borrow().is_empty());
        assert_eq!(names(&desk), ["gear.stl"]);
        assert!(
            names(&shelf).is_empty(),
            "a writable folder refused takes nothing either"
        );
        assert_eq!(
            state.borrow().drop_manager().refusal(),
            None,
            "the drag is over"
        );
    }
}

#[test]
fn a_folder_left_to_the_rule_still_moves_and_carries_no_refusal() {
    let dir = workshop("app-drop-not-app");
    let rules = ShelfRules::new(AppDrop::Refuses {
        reason: "Only tools go on the shelf".to_string(),
    });
    let (config, reported) = config_with(rules.clone());
    let (mut app, state) = browser_app(disk(&dir), config, ViewMode::Rows);
    let at = drag_over(&mut app, "Row Item gear.stl", "Parts Tree Node");
    assert_eq!(
        property(&app, "Parts Tree Node", "drop_label"),
        "Move to Parts"
    );
    assert_eq!(state.borrow().drop_manager().refusal(), None);
    release(&mut app, at, Modifiers::default());
    assert!(dir.join("Parts/gear.stl").exists());
    assert!(reported.borrow().is_empty());
    assert!(rules.asked.borrow().iter().any(|n| n == "Parts"));
    let _ = std::fs::remove_dir_all(&dir);
}
