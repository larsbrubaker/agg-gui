//! The drag's hand-offs to the app, through agg-gui's `App` with real pointer events over a
//! folder tree beside the list (`drag_support`): a move offered to the app first
//! (`take_over_move`), which may run it or hand it back; a hook that keeps its view while the drag
//! crosses back over the browser (`cancel_on_return` false), hearing at the drag's end a cancel
//! after a drop on a folder, else the release; and the keys after a drop, which stay with the
//! list for a drop on the browser's own folder and leave it for a drop outside, and which a list
//! made to take no keys (`list_takes_keys` off, a dialog's beside its name field) never takes.

mod drag_support;

use drag_support::*;

#[test]
fn a_drop_on_a_tree_row_leaves_the_list_its_keys_and_one_outside_the_browser_gives_them_up() {
    let dir = workshop("keys");
    let (mut app, _state) = browser_app(disk(&dir), BrowserConfig::default(), ViewMode::Rows);
    let at = drag_over(&mut app, "Row Item gear.stl", "Parts Tree Node");
    release(&mut app, at, Modifiers::default());
    assert_eq!(
        app.focused_widget_type_name(),
        Some("BrowserList"),
        "a drop on the browser's own folder keeps the undo keys with the list"
    );
    std::fs::write(dir.join("bolt.stl"), b"solid bolt").expect("written");
    let (mut app, _state) = browser_app(disk(&dir), BrowserConfig::default(), ViewMode::Rows);
    drag_over(&mut app, "Row Item bolt.stl", "Parts Tree Node");
    let outside = Point::new(WIDTH - VIEW / 2.0, HEIGHT / 2.0);
    move_to(&mut app, outside);
    release(&mut app, outside, Modifiers::default());
    assert_ne!(app.focused_widget_type_name(), Some("BrowserList"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Rules that take every move (`take`, failing it with `answer`'s error when it has one) or hand
/// it back, noting what they were handed.
struct MovesOffered {
    take: bool,
    seen: RefCell<Vec<String>>,
}

impl PlaceRules for MovesOffered {
    fn take_over_move<'a>(&self, cx: MoveCx<'a>) -> MoveTakeover<'a> {
        self.seen.borrow_mut().push(format!(
            "{} item(s) to {}: {}",
            cx.items.len(),
            cx.target_name,
            cx.items[0].name()
        ));
        match self.take {
            true => MoveTakeover::Taken(Err("the app's own move failed".to_string())),
            false => MoveTakeover::Declined(cx),
        }
    }
}

#[test]
fn a_move_taken_over_is_the_apps_and_one_handed_back_runs_here() {
    for take in [true, false] {
        let dir = workshop(&format!("move-takeover-{take}"));
        let rules = Rc::new(MovesOffered {
            take,
            seen: RefCell::new(Vec::new()),
        });
        let reported = Rc::new(RefCell::new(Vec::<String>::new()));
        let report = Rc::clone(&reported);
        let config = BrowserConfig::builder()
            .places(rules.clone())
            .report(Rc::new(move |m: &str| {
                report.borrow_mut().push(m.to_string())
            }))
            .build();
        let (mut app, state) = browser_app(disk(&dir), config, ViewMode::Rows);
        let at = drag_over(&mut app, "Row Item gear.stl", "Parts Tree Node");
        release(&mut app, at, Modifiers::default());
        assert_eq!(*rules.seen.borrow(), ["1 item(s) to Parts: gear.stl"]);
        let undo = state.borrow().history().undo_description();
        match take {
            true => {
                assert!(
                    dir.join("gear.stl").exists(),
                    "the app runs it, not the folder"
                );
                assert_eq!(undo, None, "the app records its own step");
                assert_eq!(
                    *reported.borrow(),
                    ["Couldn't move to \"Parts\": the app's own move failed"]
                );
            }
            false => {
                assert!(dir.join("Parts/gear.stl").exists());
                assert_eq!(undo.as_deref(), Some("Moved gear.stl to Parts"));
                assert!(reported.borrow().is_empty());
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// The app's view under a browser panel: it keeps its preview while the drag crosses the panel.
#[derive(Default)]
struct ViewUnderPanel(View);

impl OutsideDrops for ViewUnderPanel {
    fn over(&self, items: &[Rc<dyn Item>], window_at: Point) -> OutsideOver {
        self.0.over(items, window_at)
    }
    fn release(&self, items: &[Rc<dyn Item>], window_at: Point) -> bool {
        self.0.release(items, window_at)
    }
    fn cancel(&self) {
        self.0.cancel()
    }
    fn cancel_on_return(&self) -> bool {
        false
    }
}

#[test]
fn a_view_kept_on_return_hears_at_the_drags_end_a_cancel_after_a_folder_drop_else_the_release() {
    for onto_folder in [true, false] {
        let dir = workshop(&format!("kept-view-{onto_folder}"));
        let view = Rc::new(ViewUnderPanel::default());
        let config = BrowserConfig::builder().drops(view.clone()).build();
        let (mut app, _state) = browser_app(disk(&dir), config, ViewMode::Rows);
        drag_over(&mut app, "Row Item gear.stl", "Parts Row Item Collection");
        let outside = Point::new(WIDTH - VIEW / 2.0, HEIGHT / 2.0);
        move_to(&mut app, outside);
        // Back over the browser: the view keeps its preview.
        let folder = centre(&app, "Parts Row Item Collection");
        let at = match onto_folder {
            true => folder,
            false => Point::new(folder.x, 10.0),
        };
        move_to(&mut app, at);
        assert_eq!(
            *view.0.calls.borrow(),
            ["over 1"],
            "no cancel on the way back"
        );
        release(&mut app, at, Modifiers::default());
        match onto_folder {
            true => {
                assert_eq!(*view.0.calls.borrow(), ["over 1", "cancel"]);
                assert!(dir.join("Parts/gear.stl").exists(), "the folder took it");
            }
            false => {
                assert_eq!(*view.0.calls.borrow(), ["over 1", "release gear.stl"]);
                assert!(dir.join("gear.stl").exists(), "the browser moved nothing");
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn a_list_that_takes_no_keys_leaves_the_focus_where_it_was_and_hears_no_key() {
    for takes_keys in [true, false] {
        let dir = workshop(&format!("takes-keys-{takes_keys}"));
        std::fs::write(dir.join("Parts/bolt.stl"), b"solid bolt").expect("written");
        std::fs::write(dir.join("Parts/nut.stl"), b"solid nut").expect("written");
        let options = BrowserOptions {
            list_takes_keys: takes_keys,
            ..BrowserOptions::default()
        };
        let config = BrowserConfig::builder().options(options).build();
        let (mut app, state) = browser_app(disk(&dir), config, ViewMode::Rows);
        let click = |app: &mut App, id: &str| {
            let at = centre(app, id);
            app.on_mouse_move(at.x, HEIGHT - at.y);
            app.on_mouse_down(at.x, HEIGHT - at.y, MouseButton::Left, Modifiers::default());
            app.on_mouse_up(at.x, HEIGHT - at.y, MouseButton::Left, Modifiers::default());
            frame(app);
        };
        // The tree has the keys, as a dialog's name field would.
        click(&mut app, "Parts Tree Node");
        let before = app.focused_widget_type_name();
        assert!(before.is_some_and(|name| name != "BrowserList"));
        click(&mut app, "Row Item bolt.stl");
        assert_eq!(
            state.borrow().selected_entries().len(),
            1,
            "the click selects"
        );
        match takes_keys {
            true => assert_eq!(app.focused_widget_type_name(), Some("BrowserList")),
            false => assert_eq!(app.focused_widget_type_name(), before, "the focus stays"),
        }
        let command = Modifiers {
            meta: cfg!(target_os = "macos"),
            ctrl: !cfg!(target_os = "macos"),
            ..Modifiers::default()
        };
        // Select all, then Delete: a list with the keys selects every row and asks to remove
        // them; one without hears neither.
        app.on_key_down(Key::Char('a'), command);
        app.on_key_down(Key::Delete, Modifiers::default());
        frame(&mut app);
        let selected = state.borrow().selected_entries().len();
        match takes_keys {
            true => assert!(selected > 1, "select all reached the list"),
            false => assert_eq!(selected, 1, "no key reached the list"),
        }
        assert!(dir.join("Parts/bolt.stl").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
