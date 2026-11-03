//! A drag whose pressed row a navigation rebuilt away, through `FileBrowser` and agg-gui's `App`
//! with real pointer events: held over a folder in the list, the drag opens it, so the row the
//! press started on is gone and the folder's header now stands over the list. Released off the
//! list (over the bar's view button) the drag ends with nothing moved and later plain clicks drop
//! nothing; released on the empty space of the folder it opened, the part lands there. The
//! browser keeps the list (and its column) across the rebuild, so the press carries on in it.

mod drag_support;

use agg_gui_file_browser::FileBrowser;
use drag_support::*;

/// A scratch folder holding Parts (holding Inner, under its own header) and gear.stl, shown by a
/// `FileBrowser` in rows.
fn browser(label: &str) -> (App, Rc<RefCell<BrowserState>>, PathBuf) {
    agg_gui::set_device_scale(1.0);
    let dir = workshop(label);
    std::fs::create_dir_all(dir.join("Parts/Inner")).expect("made");
    std::fs::write(dir.join("Parts/index.md"), "Parts we keep").expect("written");
    let state = BrowserState::new(disk(&dir), BrowserConfig::default());
    state.borrow_mut().set_view_mode(ViewMode::Rows);
    let mut app = App::new(Box::new(FileBrowser::new(&state, font())));
    frame(&mut app);
    (app, state, dir)
}

/// Clicks the middle of the widget named `id`.
fn click(app: &mut App, id: &str) {
    let at = centre(app, id);
    move_to(app, at);
    app.on_mouse_down(at.x, HEIGHT - at.y, MouseButton::Left, Modifiers::default());
    release(app, at, Modifiers::default());
}

#[test]
fn a_release_off_the_list_after_a_spring_ends_the_drag_with_nothing_moved() {
    let _clock = agg_gui::clock::scoped_virtual(None);
    let (mut app, state, dir) = browser("rowless-release");
    drag_over(&mut app, "Row Item gear.stl", "Parts Row Item Collection");
    agg_gui::clock::advance(SPRING_DELAY);
    frame(&mut app);
    frame(&mut app);
    assert_eq!(
        state.borrow().path_names().last().map(String::as_str),
        Some("Parts")
    );
    assert!(
        find(app.root(), "Row Item gear.stl", Point::new(0.0, 0.0)).is_none(),
        "the pressed row is gone"
    );

    let button = state
        .borrow()
        .config()
        .look
        .names()
        .view_options_button
        .clone();
    let header = state.borrow().config().look.names().header_markdown.clone();
    assert!(
        find(app.root(), &header, Point::new(0.0, 0.0)).is_some(),
        "Parts' header shows"
    );
    assert_eq!(list_property(&app, "dragging"), "true", "the drag goes on");
    let at = centre(&app, &button);
    move_to(&mut app, at);
    move_to(&mut app, Point::new(at.x + 1.0, at.y));
    release(&mut app, Point::new(at.x + 1.0, at.y), Modifiers::default());
    assert_eq!(
        list_property(&app, "dragging"),
        "false",
        "the release ended the drag"
    );
    assert!(
        !state.borrow().item_press().is_pressed(),
        "no press is left under way"
    );

    // Later plain clicks drop nothing.
    click(&mut app, "Inner Row Item Collection");
    let empty = Point::new(WIDTH * 0.6, 30.0);
    move_to(&mut app, empty);
    app.on_mouse_down(
        empty.x,
        HEIGHT - empty.y,
        MouseButton::Left,
        Modifiers::default(),
    );
    release(&mut app, empty, Modifiers::default());
    assert!(dir.join("gear.stl").is_file(), "nothing moved");
    assert!(!dir.join("Parts/gear.stl").exists());
    assert!(!dir.join("Parts/Inner/gear.stl").exists());
    assert_eq!(list_property(&app, "dragging"), "false");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_drop_on_the_empty_space_of_a_folder_the_drag_opened_lands_in_it() {
    let _clock = agg_gui::clock::scoped_virtual(None);
    let (mut app, state, dir) = browser("rowless-drop");
    drag_over(&mut app, "Row Item gear.stl", "Parts Row Item Collection");
    agg_gui::clock::advance(SPRING_DELAY);
    frame(&mut app);
    frame(&mut app);
    assert_eq!(
        state.borrow().path_names().last().map(String::as_str),
        Some("Parts")
    );
    let list = found(&app, "LibraryView").1;
    let empty = Point::new(list.x + list.width * 0.6, list.y + 20.0);
    move_to(&mut app, empty);
    move_to(&mut app, Point::new(empty.x + 1.0, empty.y));
    assert_eq!(list_property(&app, "drop_label"), "Move to Parts");
    release(
        &mut app,
        Point::new(empty.x + 1.0, empty.y),
        Modifiers::default(),
    );
    assert!(
        dir.join("Parts/gear.stl").is_file(),
        "the part landed in Parts"
    );
    assert!(!dir.join("gear.stl").exists());
    assert_eq!(list_property(&app, "dragging"), "false");
    let _ = std::fs::remove_dir_all(&dir);
}
