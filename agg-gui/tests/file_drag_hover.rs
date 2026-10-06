//! File drag-hover, drag-leave and byte-carrying drops, through the real
//! `App` entry points the platform shells call:
//!
//! 1. `Event::FileDragHover` reaches the widget under the pointer with its
//!    position in that widget's local space, and falls back to the rest of
//!    the tree when that widget ignores it (like `FileDropped`).
//! 2. `Event::FileDragLeave` reaches every widget, once per drag, whether the
//!    drag leaves, is cancelled, or ends in a drop (then before the drop).
//! 3. `Event::FileDataDropped` carries names and bytes, translated like a
//!    path drop, and shares the bytes instead of copying them per level.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{App, DrawCtx, DroppedFileData, Event, EventResult, Rect, Size, Widget};

type Log = Rc<RefCell<Vec<String>>>;

/// Logs every file event it gets as text (`"<name> <event> x,y"`), and
/// consumes hovers and drops when `consumes` is set.
struct Recorder {
    name: &'static str,
    consumes: bool,
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    log: Log,
    data: Rc<RefCell<Vec<DroppedFileData>>>,
}

impl Widget for Recorder {
    fn type_name(&self) -> &'static str {
        "Recorder"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }
    fn layout(&mut self, _available: Size) -> Size {
        Size::new(self.bounds.width, self.bounds.height)
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, e: &Event) -> EventResult {
        let line = match e {
            Event::FileDragHover { pos, paths } => {
                format!("hover {},{} {}", pos.x, pos.y, paths.len())
            }
            Event::FileDragLeave => {
                self.log.borrow_mut().push(format!("{} leave", self.name));
                return EventResult::Ignored;
            }
            Event::FileDropped { pos, .. } => format!("drop {},{}", pos.x, pos.y),
            Event::FileDataDropped { pos, files } => {
                self.data.borrow_mut().extend(files.iter().cloned());
                format!("data {},{} {}", pos.x, pos.y, files.len())
            }
            _ => return EventResult::Ignored,
        };
        self.log.borrow_mut().push(format!("{} {line}", self.name));
        if self.consumes {
            EventResult::Consumed
        } else {
            EventResult::Ignored
        }
    }
}

/// Left half `left`, right half `right`, laid out side by side.
struct SplitRoot {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl Widget for SplitRoot {
    fn type_name(&self) -> &'static str {
        "SplitRoot"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }
    fn layout(&mut self, available: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
        let half = available.width / 2.0;
        for (i, child) in self.children.iter_mut().enumerate() {
            child.set_bounds(Rect::new(half * i as f64, 0.0, half, available.height));
            child.layout(Size::new(half, available.height));
        }
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _e: &Event) -> EventResult {
        EventResult::Ignored
    }
}

struct Fixture {
    app: App,
    log: Log,
    data: Rc<RefCell<Vec<DroppedFileData>>>,
}

/// An 800x600 window: an ignoring `left` pane, a consuming `right` pane at
/// x = 400 (so local positions differ from window ones).
fn fixture(right_consumes: bool) -> Fixture {
    agg_gui::set_device_scale(1.0);
    let log: Log = Rc::default();
    let data: Rc<RefCell<Vec<DroppedFileData>>> = Rc::default();
    let pane = |name, consumes| -> Box<dyn Widget> {
        Box::new(Recorder {
            name,
            consumes,
            bounds: Rect::default(),
            children: Vec::new(),
            log: log.clone(),
            data: data.clone(),
        })
    };
    let root: Box<dyn Widget> = Box::new(SplitRoot {
        bounds: Rect::default(),
        children: vec![pane("left", false), pane("right", right_consumes)],
    });
    let mut app = App::new(root);
    app.layout(Size::new(800.0, 600.0));
    Fixture { app, log, data }
}

fn take(log: &Log) -> Vec<String> {
    std::mem::take(&mut *log.borrow_mut())
}

#[test]
fn hover_reaches_the_widget_under_the_pointer_in_local_space() {
    let mut f = fixture(true);
    // Screen (500, 100) is Y-down: world (500, 500), right-pane local (100, 500).
    f.app
        .on_file_drag_hover(500.0, 100.0, vec!["a.stl".into(), "b.stl".into()]);
    assert_eq!(take(&f.log), ["right hover 100,500 2"]);
    assert!(f.app.file_drag_active());
}

#[test]
fn ignored_hover_is_offered_to_the_rest_of_the_tree() {
    let mut f = fixture(true);
    // Over the left pane, which ignores it; the broadcast reaches the right
    // pane, with the position in its local space (outside its bounds).
    f.app.on_file_drag_hover(100.0, 100.0, Vec::new());
    assert_eq!(
        take(&f.log),
        ["left hover 100,500 0", "right hover -300,500 0"]
    );
}

#[test]
fn leave_reaches_every_widget_once_per_drag() {
    let mut f = fixture(true);
    // No drag in progress: nothing to end.
    f.app.on_file_drag_leave();
    assert!(take(&f.log).is_empty());

    f.app.on_file_drag_hover(500.0, 100.0, Vec::new());
    take(&f.log);
    f.app.on_file_drag_leave();
    assert_eq!(take(&f.log), ["left leave", "right leave"]);
    assert!(!f.app.file_drag_active());

    f.app.on_file_drag_leave();
    assert!(take(&f.log).is_empty(), "a second leave is a no-op");
}

#[test]
fn a_drop_ends_the_drag_before_it_is_delivered() {
    let mut f = fixture(true);
    f.app.on_file_drag_hover(500.0, 100.0, Vec::new());
    take(&f.log);
    f.app
        .on_file_dropped(500.0, 100.0, vec![std::path::PathBuf::from("a.stl")]);
    assert_eq!(
        take(&f.log),
        ["left leave", "right leave", "right drop 100,500"]
    );
    assert!(!f.app.file_drag_active());

    // Without a hover first (a platform that reports none) there is no leave.
    f.app
        .on_file_dropped(500.0, 100.0, vec![std::path::PathBuf::from("a.stl")]);
    assert_eq!(take(&f.log), ["right drop 100,500"]);
}

#[test]
fn data_drop_carries_names_and_shared_bytes_in_local_space() {
    let mut f = fixture(true);
    f.app.on_file_drag_hover(500.0, 100.0, Vec::new());
    take(&f.log);
    let bytes: Arc<[u8]> = Arc::from(&b"solid cube"[..]);
    f.app.on_file_data_dropped(
        500.0,
        100.0,
        vec![DroppedFileData::new("cube.stl", bytes.clone())],
    );
    assert_eq!(
        take(&f.log),
        ["left leave", "right leave", "right data 100,500 1"]
    );
    let data = f.data.borrow();
    assert_eq!(data.len(), 1);
    assert_eq!(data[0].name, "cube.stl");
    assert_eq!(&data[0].bytes[..], b"solid cube");
    assert!(
        Arc::ptr_eq(&data[0].bytes, &bytes),
        "descending the tree must share the bytes, not copy them"
    );
}

#[test]
fn ignored_data_drop_is_offered_to_the_rest_of_the_tree() {
    let mut f = fixture(true);
    f.app
        .on_file_data_dropped(100.0, 100.0, vec![DroppedFileData::new("a.svg", vec![1u8])]);
    assert_eq!(
        take(&f.log),
        ["left data 100,500 1", "right data -300,500 1"]
    );
}

#[test]
fn an_empty_data_drop_delivers_nothing() {
    let mut f = fixture(true);
    f.app.on_file_data_dropped(500.0, 100.0, Vec::new());
    assert!(take(&f.log).is_empty());
}

#[test]
fn dropped_file_data_debug_shows_length_not_contents() {
    let file = DroppedFileData::new("big.stl", vec![7u8; 4096]);
    assert_eq!(
        format!("{file:?}"),
        "DroppedFileData { name: \"big.stl\", len: 4096 }"
    );
}
