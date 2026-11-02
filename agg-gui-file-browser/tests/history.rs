//! The browser's undo history (`BrowserHistory`): steps undo and redo latest first and a new step
//! empties the redo list; an undo that finishes later holds the next one back until it lands;
//! each change's notice says what happened and offers Undo or Redo (or nothing for an
//! announcement), naming what couldn't be put back; a step recorded later through a recorder goes
//! on top when it lands and is dropped once the history is gone; views given one history share
//! it; and on a real folder on disk a recorded rename is undone and redone on disk with the
//! browser listing the folder again each time.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use agg_gui_file_browser::{BrowserConfig, BrowserHistory, BrowserState, NoticeAction};
use agg_gui_files::disk::{AllFiles, DiskFolder};
use agg_gui_files::{HistoryStep, MemoryFolder, StepDone, StepOutcome};

type Log = Rc<RefCell<Vec<String>>>;

/// A step that logs each undo and redo and finishes at once; `fail` names an item it couldn't
/// put back (and then keeps nothing).
struct Logged {
    name: String,
    log: Log,
    fail: Option<String>,
}

impl Logged {
    fn boxed(name: &str, log: &Log) -> Box<dyn HistoryStep> {
        Box::new(Self {
            name: name.to_string(),
            log: Rc::clone(log),
            fail: None,
        })
    }

    fn finish(self: Box<Self>, what: &str, done: StepDone) {
        self.log.borrow_mut().push(format!("{what} {}", self.name));
        let outcome = match self.fail.clone() {
            Some(failure) => StepOutcome {
                kept: None,
                failures: vec![failure],
            },
            None => StepOutcome {
                kept: Some(self),
                failures: Vec::new(),
            },
        };
        done(outcome);
    }
}

impl HistoryStep for Logged {
    fn description(&self) -> String {
        format!("Changed {}", self.name)
    }
    fn undo(self: Box<Self>, done: StepDone) {
        self.finish("undo", done);
    }
    fn redo(self: Box<Self>, done: StepDone) {
        self.finish("redo", done);
    }
}

/// Where a `Later` step waits while a server would answer.
type Parked = Rc<RefCell<Option<(Box<Later>, StepDone)>>>;

/// A step whose undo and redo finish only when the test lands them.
struct Later {
    parked: Parked,
}

impl HistoryStep for Later {
    fn description(&self) -> String {
        "Moved Gear to Cloud".to_string()
    }
    fn undo(self: Box<Self>, done: StepDone) {
        let parked = Rc::clone(&self.parked);
        *parked.borrow_mut() = Some((self, done));
    }
    fn redo(self: Box<Self>, done: StepDone) {
        let parked = Rc::clone(&self.parked);
        *parked.borrow_mut() = Some((self, done));
    }
}

/// The server answers: the parked undo or redo finishes, keeping its step.
fn land(parked: &Parked) {
    let (step, done) = parked.borrow_mut().take().expect("a step is waiting");
    done(StepOutcome {
        kept: Some(step),
        failures: Vec::new(),
    });
}

/// A browser on an empty memory folder.
fn browser() -> Rc<RefCell<BrowserState>> {
    BrowserState::new(
        Box::new(MemoryFolder::new("Home")),
        BrowserConfig::default(),
    )
}

fn notice(state: &Rc<RefCell<BrowserState>>) -> (String, Option<NoticeAction>) {
    let notice = state.borrow().history().notice().expect("a notice shows");
    (notice.text, notice.action)
}

#[test]
fn steps_undo_and_redo_latest_first_and_a_new_step_empties_redo() {
    let (state, log) = (browser(), Log::default());
    let history = state.borrow().history();
    assert!(!history.can_undo() && !history.can_redo());
    for name in ["a", "b", "c"] {
        history.record(Logged::boxed(name, &log));
    }
    assert_eq!(history.undo_description().as_deref(), Some("Changed c"));
    assert!(BrowserState::undo(&state));
    assert!(BrowserState::undo(&state));
    assert_eq!(history.depths(), (1, 2));
    assert_eq!(history.redo_description().as_deref(), Some("Changed b"));
    assert!(BrowserState::redo(&state));
    assert_eq!(*log.borrow(), ["undo c", "undo b", "redo b"]);
    assert_eq!(history.depths(), (2, 1));

    history.record(Logged::boxed("d", &log));
    assert!(!history.can_redo());
    assert!(!BrowserState::redo(&state));
    assert_eq!(history.undo_description().as_deref(), Some("Changed d"));
}

#[test]
fn an_undo_finishing_later_holds_the_next_one_back_until_it_lands() {
    let (state, log) = (browser(), Log::default());
    let history = state.borrow().history();
    let parked = Parked::default();
    history.record(Logged::boxed("a", &log));
    history.record(Box::new(Later {
        parked: Rc::clone(&parked),
    }));
    let generation = history.generation();

    assert!(BrowserState::undo(&state));
    assert!(history.is_busy());
    assert!(history.generation() > generation, "the buttons follow");
    assert!(!history.can_undo() && !history.can_redo());
    assert!(!BrowserState::undo(&state), "a second undo waits");
    assert!(log.borrow().is_empty());

    let writes = state.borrow().writes();
    land(&parked);
    assert!(!history.is_busy());
    assert_eq!(
        state.borrow().writes(),
        writes + 1,
        "the browser lists again"
    );
    assert_eq!(
        notice(&state),
        (
            "Undone: Moved Gear to Cloud".to_string(),
            Some(NoticeAction::Redo)
        )
    );
    assert_eq!(history.depths(), (1, 1));
    assert!(BrowserState::undo(&state));
    assert_eq!(*log.borrow(), ["undo a"]);
}

#[test]
fn the_notice_says_what_happened_and_what_it_offers() {
    let (state, log) = (browser(), Log::default());
    let history = state.borrow().history();
    history.record(Logged::boxed("a", &log));
    assert_eq!(
        notice(&state),
        ("Changed a".to_string(), Some(NoticeAction::Undo))
    );
    let first = history.notice().map(|n| n.id);

    BrowserState::undo(&state);
    assert_eq!(
        notice(&state),
        ("Undone: Changed a".to_string(), Some(NoticeAction::Redo))
    );
    assert_ne!(history.notice().map(|n| n.id), first, "a new notice");
    BrowserState::redo(&state);
    assert_eq!(
        notice(&state),
        ("Redone: Changed a".to_string(), Some(NoticeAction::Undo))
    );

    history.record_with_note(Logged::boxed("b", &log), Some("1 item stayed".to_string()));
    assert_eq!(notice(&state).0, "Changed b. 1 item stayed");
    history.record_saying(Logged::boxed("c", &log), "Copied c".to_string());
    assert_eq!(
        notice(&state),
        ("Copied c".to_string(), Some(NoticeAction::Undo))
    );
    history.announce("Nothing to move".to_string());
    assert_eq!(notice(&state), ("Nothing to move".to_string(), None));
}

#[test]
fn an_undo_that_puts_nothing_back_names_what_failed_and_offers_nothing() {
    let (state, log) = (browser(), Log::default());
    let history = state.borrow().history();
    history.record(Box::new(Logged {
        name: "a".to_string(),
        log: Rc::clone(&log),
        fail: Some("gear.stl: its place is taken".to_string()),
    }));
    BrowserState::undo(&state);
    assert_eq!(
        notice(&state),
        (
            "Couldn't put back: gear.stl: its place is taken".to_string(),
            None
        )
    );
    assert_eq!(history.depths(), (0, 0));
}

#[test]
fn a_step_recorded_later_goes_on_top_and_none_once_the_history_is_gone() {
    let log = Log::default();
    let history = Rc::new(BrowserHistory::new());
    let recorder = history.recorder();
    history.record(Logged::boxed("a", &log));
    history.record(Logged::boxed("b", &log));
    let state = BrowserState::new(
        Box::new(MemoryFolder::new("Home")),
        BrowserConfig::builder()
            .history(Rc::clone(&history))
            .build(),
    );
    BrowserState::undo(&state);
    assert!(history.can_redo());

    // The server answers the move made before: it lands on top and empties redo.
    recorder.record(Logged::boxed("late", &log));
    assert_eq!(history.undo_description().as_deref(), Some("Changed late"));
    assert!(!history.can_redo());

    drop(state);
    drop(history);
    recorder.record(Logged::boxed("after", &log));
    assert!(recorder
        .keep(agg_gui_files::Recorded::unrecorded(Err("no".into())))
        .is_err());
}

#[test]
fn views_given_one_history_share_it() {
    let log = Log::default();
    let history = Rc::new(BrowserHistory::new());
    let config = || {
        BrowserConfig::builder()
            .history(Rc::clone(&history))
            .build()
    };
    let tab = BrowserState::new(Box::new(MemoryFolder::new("Home")), config());
    let dialog = BrowserState::new(Box::new(MemoryFolder::new("Home")), config());
    dialog
        .borrow()
        .history()
        .record(Logged::boxed("moved in the dialog", &log));
    assert!(BrowserState::undo(&tab));
    assert_eq!(*log.borrow(), ["undo moved in the dialog"]);
}

/// A fresh, empty folder under the system's temp directory for one test.
fn scratch(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "agg-gui-file-browser-history-{label}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("the scratch folder is made");
    path
}

fn names(state: &Rc<RefCell<BrowserState>>) -> Vec<String> {
    state
        .borrow()
        .entries()
        .iter()
        .map(|e| e.name().to_string())
        .collect()
}

#[test]
fn a_rename_on_disk_is_recorded_undone_and_redone_with_the_folder_listed_again() {
    let dir = scratch("rename");
    std::fs::write(dir.join("gear.stl"), b"solid gear").expect("written");
    let folder = DiskFolder::new(&dir, Rc::new(AllFiles));
    let state = BrowserState::new(Box::new(folder), BrowserConfig::default());
    assert_eq!(names(&state), ["gear.stl"]);

    let item = state.borrow().entries()[0].item();
    let result = BrowserState::write_recorded(&state, 0, |w| w.rename_recorded(&item, "cog.stl"));
    assert_eq!(result, Ok(()));
    assert!(dir.join("cog.stl").exists() && !dir.join("gear.stl").exists());
    assert_eq!(names(&state), ["cog.stl"]);
    assert_eq!(
        notice(&state),
        (
            "Renamed gear.stl to cog.stl".to_string(),
            Some(NoticeAction::Undo)
        )
    );

    assert!(BrowserState::undo(&state));
    assert!(dir.join("gear.stl").exists() && !dir.join("cog.stl").exists());
    assert_eq!(names(&state), ["gear.stl"], "listed again after the undo");
    assert_eq!(notice(&state).0, "Undone: Renamed gear.stl to cog.stl");

    assert!(BrowserState::redo(&state));
    assert!(dir.join("cog.stl").exists());
    assert_eq!(names(&state), ["cog.stl"], "listed again after the redo");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_write_to_a_folder_that_takes_none_is_refused_and_records_nothing() {
    let mut root = agg_gui_files::DynamicFolder::new("Home");
    root.data.child_containers = Vec::new();
    let state = BrowserState::new(Box::new(root), BrowserConfig::default());
    let result = BrowserState::write_recorded(&state, 0, |w| w.create_folder_recorded("New"));
    assert_eq!(result, Err("This folder can't be changed".to_string()));
    assert_eq!(state.borrow().status(), "This folder can't be changed");
    assert!(state.borrow().history().notice().is_none());
}
