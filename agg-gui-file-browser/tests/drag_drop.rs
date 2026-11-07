//! Dragging the browser's items onto its folders, through agg-gui's `App` with real pointer
//! events over a folder tree beside the list (`drag_support`): a row dropped on a folder row, a
//! folder tile or a tree row moves on disk and the move can be undone; with the copy modifier the
//! drop copies; a drop between two places copies; a read-only folder is never highlighted and
//! takes nothing; and a copy is offered to the app first (`take_over_copy`), which may run it,
//! refuse it or hand it back. A drag held over a folder opens it after the spring delay (on
//! agg-gui's virtual clock): a tree row in place, a folder in the list in the list, where a drop
//! on the empty space lands in it. The drag's picture follows the pointer (everywhere without a
//! hook), hides outside the browser when the app says so whatever it answers, and a release over
//! the app's view goes to its `OutsideDrops` hook (`cancel` when the drag comes back). An app's
//! overlay on a carried row paints faded with it, and learns how its square sits in a row or a
//! tile.

mod drag_support;

use drag_support::*;

/// Drags gear.stl's row onto `target` in `mode` and checks the move on disk and its undo.
fn moves_onto(label: &str, mode: ViewMode, target: &str) {
    let dir = workshop(label);
    let (mut app, state) = browser_app(disk(&dir), BrowserConfig::default(), mode);
    let at = drag_over(&mut app, "Row Item gear.stl", target);
    assert_eq!(property(&app, target, "drop_label"), "Move to Parts");
    assert_eq!(property(&app, "Row Item gear.stl", "lifted"), "true");
    release(&mut app, at, Modifiers::default());
    assert!(dir.join("Parts/gear.stl").exists() && !dir.join("gear.stl").exists());
    // The list shows the folder without it.
    assert!(find(app.root(), "Row Item gear.stl", Point::new(0.0, 0.0)).is_none());
    let history = state.borrow().history();
    assert_eq!(
        history.undo_description().as_deref(),
        Some("Moved gear.stl to Parts")
    );
    assert!(BrowserState::undo(&state));
    assert!(dir.join("gear.stl").exists() && !dir.join("Parts/gear.stl").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_row_dropped_on_a_folder_row_moves_and_can_be_undone() {
    moves_onto("row", ViewMode::Rows, "Parts Row Item Collection");
}

#[test]
fn a_row_dropped_on_a_folder_tile_moves_and_can_be_undone() {
    moves_onto("tile", ViewMode::Icons, "Parts Row Item Collection");
}

#[test]
fn a_row_dropped_on_a_tree_row_moves_and_can_be_undone() {
    moves_onto("tree", ViewMode::Rows, "Parts Tree Node");
}

/// A folder with no ID of its own (the app's disk folders may answer an empty one): only its
/// location tells it apart.
struct Unnamed(DiskFolder);

impl Folder for Unnamed {
    fn id(&self) -> &str {
        ""
    }
    fn name(&self) -> &str {
        self.0.name()
    }
    fn is_protected(&self) -> bool {
        self.0.is_protected()
    }
    fn child_containers(&self) -> Vec<Rc<dyn FolderLink>> {
        self.0.child_containers()
    }
    fn items(&self) -> Vec<Rc<dyn Item>> {
        self.0.items()
    }
    fn load(&mut self) {
        self.0.load()
    }
    fn place(&self) -> Option<PlaceId> {
        self.0.place()
    }
    fn location_key(&self) -> Option<String> {
        self.0.location_key()
    }
    fn as_writable(&self) -> Option<&dyn agg_gui_files::WritableFolder> {
        self.0.as_writable()
    }
    fn as_writable_mut(&mut self) -> Option<&mut dyn agg_gui_files::WritableFolder> {
        self.0.as_writable_mut()
    }
}

#[test]
fn a_move_into_the_folder_above_lists_that_open_folder_again() {
    let dir = workshop("into-above");
    std::fs::write(dir.join("Parts/bolt.stl"), b"solid bolt").expect("written");
    let root: Box<dyn Folder> = Box::new(Unnamed(DiskFolder::new(&dir, Rc::new(AllFiles))));
    let (mut app, state) = browser_app(root, BrowserConfig::default(), ViewMode::Rows);
    let parts = state
        .borrow()
        .entries()
        .iter()
        .find(|e| e.name() == "Parts")
        .and_then(|e| e.item().as_container_link().map(|_| ()))
        .is_some();
    assert!(parts, "the workshop lists Parts");
    let link = state.borrow().open_path()[0]
        .borrow()
        .child_containers()
        .into_iter()
        .find(|l| l.name() == "Parts")
        .expect("Parts");
    state.borrow_mut().open(link.as_ref());
    frame(&mut app);
    let bolt = state.borrow().open_path()[1].borrow().items()[0].clone();
    let above: Box<dyn Folder> = Box::new(Unnamed(DiskFolder::new(&dir, Rc::new(AllFiles))));
    assert!(agg_gui_file_browser::drop_into(
        &state,
        above,
        "workshop",
        &[bolt],
        1,
        false
    ));
    assert!(dir.join("bolt.stl").exists());
    let listed: Vec<String> = state.borrow().open_path()[0]
        .borrow()
        .items()
        .iter()
        .map(|i| i.name().to_string())
        .collect();
    assert!(
        listed.contains(&"bolt.stl".to_string()),
        "the open folder above lists what landed in it: {listed:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_copy_modifier_copies() {
    let dir = workshop("copy");
    let (mut app, state) = browser_app(disk(&dir), BrowserConfig::default(), ViewMode::Rows);
    let at = drag_over(&mut app, "Row Item gear.stl", "Parts Row Item Collection");
    release(&mut app, at, copy_modifier());
    assert!(dir.join("Parts/gear.stl").exists() && dir.join("gear.stl").exists());
    let history = state.borrow().history();
    assert!(history.can_undo(), "the copy is one step");
    assert!(BrowserState::undo(&state));
    assert!(!dir.join("Parts/gear.stl").exists() && dir.join("gear.stl").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_drop_between_two_places_copies() {
    let (mut app, _state, desk, shelf) = two_places(BrowserConfig::default(), false);
    let at = drag_over(&mut app, "Row Item gear.stl", "Shelf Tree Node");
    assert_eq!(
        property(&app, "Shelf Tree Node", "drop_label"),
        "Copy to Shelf"
    );
    release(&mut app, at, Modifiers::default());
    assert_eq!(names(&desk), ["gear.stl"]);
    assert_eq!(names(&shelf), ["gear.stl"]);
}

#[test]
fn a_read_only_folder_is_never_highlighted_and_takes_nothing() {
    let (mut app, _state, desk, shelf) = two_places(BrowserConfig::default(), true);
    let at = drag_over(&mut app, "Row Item gear.stl", "Shelf Tree Node");
    assert_eq!(
        property(&app, "Shelf Tree Node", "drop_highlighted"),
        "false"
    );
    assert_eq!(property(&app, "Shelf Tree Node", "drop_label"), "");
    release(&mut app, at, Modifiers::default());
    assert_eq!(names(&desk), ["gear.stl"]);
    assert!(names(&shelf).is_empty());
}

/// Rules that take every copy (`take`) or hand it back, noting what they were handed.
struct Offered {
    take: bool,
    seen: RefCell<Vec<String>>,
}

impl PlaceRules for Offered {
    fn take_over_copy(&self, cx: CopyCx) -> CopyTakeover {
        let source = cx
            .source
            .as_ref()
            .map_or(String::new(), |s| s.borrow().name().to_string());
        self.seen.borrow_mut().push(format!(
            "{} item(s) from {source} to {}",
            cx.items.len(),
            cx.target_name
        ));
        match self.take {
            true => {
                (cx.report)("copying the app's way");
                CopyTakeover::Taken
            }
            false => CopyTakeover::Declined(cx),
        }
    }
}

#[test]
fn a_copy_taken_over_is_the_apps_and_one_handed_back_runs_here() {
    for take in [true, false] {
        let rules = Rc::new(Offered {
            take,
            seen: RefCell::new(Vec::new()),
        });
        let config = BrowserConfig::builder().places(rules.clone()).build();
        let (mut app, state, _desk, shelf) = two_places(config, false);
        let at = drag_over(&mut app, "Row Item gear.stl", "Shelf Tree Node");
        release(&mut app, at, Modifiers::default());
        assert_eq!(*rules.seen.borrow(), ["1 item(s) from Desk to Shelf"]);
        match take {
            true => {
                assert!(names(&shelf).is_empty(), "the app runs it");
                assert_eq!(state.borrow().status(), "copying the app's way");
            }
            false => assert_eq!(names(&shelf), ["gear.stl"]),
        }
    }
}

/// Rules that refuse every copy, telling the user why as the app does.
struct RefusesCopies;

impl PlaceRules for RefusesCopies {
    fn take_over_copy(&self, cx: CopyCx) -> CopyTakeover {
        (cx.report)("Items can't be copied into this folder.");
        CopyTakeover::Refused
    }
}

#[test]
fn a_copy_the_app_refuses_is_no_drop_and_says_why() {
    let config = BrowserConfig::builder()
        .places(Rc::new(RefusesCopies))
        .build();
    let (_app, state, desk, shelf) = two_places(config, false);
    let gear = desk.items();
    let dropped = drop_into(&state, Box::new(shelf.clone()), "Shelf", &gear, 1, true);
    assert!(!dropped, "a refused copy copied nothing");
    assert!(names(&shelf).is_empty());
    assert_eq!(
        state.borrow().status(),
        "Items can't be copied into this folder."
    );
}

/// The workshop with a folder Inner inside Parts.
fn nested_workshop(label: &str) -> PathBuf {
    let dir = workshop(label);
    std::fs::create_dir_all(dir.join("Parts/Inner")).expect("made");
    dir
}

#[test]
fn a_drag_held_over_a_closed_tree_row_opens_it_after_the_delay() {
    let _clock = agg_gui::clock::scoped_virtual(None);
    let dir = nested_workshop("spring-tree");
    let (mut app, _state) = browser_app(disk(&dir), BrowserConfig::default(), ViewMode::Rows);
    drag_over(&mut app, "Row Item gear.stl", "Parts Tree Node");
    assert_eq!(property(&app, "Parts Tree Node", "expanded"), "false");
    agg_gui::clock::advance(SPRING_DELAY / 2);
    frame(&mut app);
    assert_eq!(
        property(&app, "Parts Tree Node", "expanded"),
        "false",
        "not yet"
    );
    agg_gui::clock::advance(SPRING_DELAY / 2);
    // The paint that finds the wait over opens the row; the next frame shows it.
    frame(&mut app);
    frame(&mut app);
    assert_eq!(property(&app, "Parts Tree Node", "expanded"), "true");

    // The folder it opened takes the drop.
    let at = centre(&app, "Inner Tree Node");
    move_to(&mut app, at);
    move_to(&mut app, Point::new(at.x + 1.0, at.y));
    release(&mut app, Point::new(at.x + 1.0, at.y), Modifiers::default());
    assert!(dir.join("Parts/Inner/gear.stl").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_drag_held_over_a_folder_in_the_list_opens_it_there_and_drops_into_it() {
    let _clock = agg_gui::clock::scoped_virtual(None);
    let dir = workshop("spring-list");
    let (mut app, state) = browser_app(disk(&dir), BrowserConfig::default(), ViewMode::Rows);
    drag_over(&mut app, "Row Item gear.stl", "Parts Row Item Collection");
    agg_gui::clock::advance(SPRING_DELAY);
    frame(&mut app);
    frame(&mut app);
    assert_eq!(
        state.borrow().path_names().last().map(String::as_str),
        Some("Parts")
    );

    // On the empty space of the folder it opened, the drop lands there.
    let empty = Point::new(TREE + 150.0, 20.0);
    move_to(&mut app, empty);
    move_to(&mut app, Point::new(empty.x + 1.0, empty.y));
    // The list is lit as a folder is, and says what the drop does.
    assert_eq!(property(&app, "LibraryView", "dragging"), "true");
    assert_eq!(property(&app, "LibraryView", "drop_label"), "Move to Parts");
    release(
        &mut app,
        Point::new(empty.x + 1.0, empty.y),
        Modifiers::default(),
    );
    assert!(dir.join("Parts/gear.stl").exists() && !dir.join("gear.stl").exists());
    assert!(find(app.root(), "Row Item gear.stl", Point::new(0.0, 0.0)).is_some());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_picture_follows_hides_over_the_apps_view_and_a_release_there_is_the_apps() {
    let dir = workshop("outside");
    let view = Rc::new(View::default());
    let config = BrowserConfig::builder().drops(view.clone()).build();
    let (mut app, _state) = browser_app(disk(&dir), config, ViewMode::Rows);
    let at = drag_over(&mut app, "Row Item gear.stl", "Parts Row Item Collection");
    let expected = format!("{},{},64,64", at.x + 16.0, at.y - 16.0 - 64.0);
    assert_eq!(list_property(&app, "drag_picture"), expected);
    assert_eq!(list_property(&app, "drag_picture_count"), "1");
    assert_eq!(
        list_property(&app, "drag_picture_image"),
        property(&app, "Row Item gear.stl", "thumbnail"),
        "the picture is the art the carried row draws"
    );
    assert!(view.calls.borrow().is_empty(), "inside the browser");

    // Over the app's view, which shows the items itself.
    let outside = Point::new(WIDTH - VIEW / 2.0, HEIGHT / 2.0);
    move_to(&mut app, outside);
    assert_eq!(list_property(&app, "drag_picture"), "");
    // Back inside: the view puts itself back and the picture returns.
    let inside = centre(&app, "Parts Row Item Collection");
    move_to(&mut app, inside);
    assert_ne!(list_property(&app, "drag_picture"), "");
    move_to(&mut app, outside);
    release(&mut app, outside, Modifiers::default());
    assert_eq!(
        *view.calls.borrow(),
        ["over 1", "cancel", "over 1", "release gear.stl"]
    );
    assert!(dir.join("gear.stl").exists(), "the browser moved nothing");
    assert_eq!(list_property(&app, "drag_picture"), "");
    let _ = std::fs::remove_dir_all(&dir);
}

/// An app whose drag pictures show only over the browser: nothing outside takes the items.
struct OnlyOverBrowser;

impl OutsideDrops for OnlyOverBrowser {
    fn over(&self, _items: &[Rc<dyn Item>], _window_at: Point) -> OutsideOver {
        OutsideOver::NoTarget {
            hides_picture: true,
        }
    }
    fn release(&self, _items: &[Rc<dyn Item>], _window_at: Point) -> bool {
        false
    }
    fn cancel(&self) {}
}

#[test]
fn outside_the_picture_follows_without_a_hook_and_hides_where_the_app_says() {
    for hook in [false, true] {
        let dir = workshop(if hook { "only-over" } else { "no-hook" });
        let config = match hook {
            true => BrowserConfig::builder()
                .drops(Rc::new(OnlyOverBrowser))
                .build(),
            false => BrowserConfig::default(),
        };
        let (mut app, _state) = browser_app(disk(&dir), config, ViewMode::Rows);
        drag_over(&mut app, "Row Item gear.stl", "Parts Row Item Collection");
        let outside = Point::new(WIDTH - VIEW / 2.0, HEIGHT / 2.0);
        move_to(&mut app, outside);
        let picture = list_property(&app, "drag_picture");
        match hook {
            true => assert_eq!(picture, "", "hidden by a no-target answer"),
            false => assert_eq!(
                picture,
                format!("{},{},64,64", outside.x + 16.0, outside.y - 16.0 - 64.0)
            ),
        }
        release(&mut app, outside, Modifiers::default());
        assert!(dir.join("gear.stl").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn a_spring_wait_counts_from_the_hover_even_on_a_clock_set_before_the_epoch() {
    // A virtual clock standing before the UI epoch reads zero time since the epoch until it
    // passes it: a wait counted from the epoch would never end here.
    let before = agg_gui::clock::epoch()
        .checked_sub(std::time::Duration::from_secs(5))
        .expect("five seconds before the epoch is a time");
    let _clock = agg_gui::clock::scoped_virtual(Some(before));
    let dir = nested_workshop("spring-before-epoch");
    let (mut app, _state) = browser_app(disk(&dir), BrowserConfig::default(), ViewMode::Rows);
    drag_over(&mut app, "Row Item gear.stl", "Parts Tree Node");
    agg_gui::clock::advance(SPRING_DELAY);
    frame(&mut app);
    frame(&mut app);
    assert_eq!(property(&app, "Parts Tree Node", "expanded"), "true");
    let _ = std::fs::remove_dir_all(&dir);
}

/// An overlay that fills its square opaque red.
struct RedSquare {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl Widget for RedSquare {
    fn type_name(&self) -> &'static str {
        "RedSquare"
    }
    fn id(&self) -> Option<&str> {
        Some("Red Square")
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
        available
    }
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        ctx.set_fill_color(agg_gui::Color::rgba(1.0, 0.0, 0.0, 1.0));
        ctx.begin_path();
        ctx.rect(0.0, 0.0, self.bounds.width, self.bounds.height);
        ctx.fill();
    }
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// A red square over gear.stl's picture, noting the frames it was laid in.
struct RedOverGear {
    insets: Rc<RefCell<Vec<f64>>>,
}

impl ItemPictures for RedOverGear {
    fn picture(&self, _cx: &PictureCx<'_>, _entry: &Entry, _side: i32) -> PictureState {
        PictureState::Default
    }
    fn tile_overlay(&self, entry: &Entry, frame: OverlayFrame) -> Option<Box<dyn Widget>> {
        self.insets.borrow_mut().push(frame.inset);
        (entry.item().name() == "gear.stl").then(|| {
            Box::new(RedSquare {
                bounds: Rect::default(),
                children: Vec::new(),
            }) as Box<dyn Widget>
        })
    }
}

/// The colour painted at `at` (the root's coordinates, Y up).
fn pixel_at(app: &mut App, at: Point) -> [u8; 4] {
    app.layout(Size::new(WIDTH, HEIGHT));
    let mut fb = Framebuffer::new(WIDTH as u32, HEIGHT as u32);
    let mut ctx = GfxCtx::new(&mut fb);
    app.paint(&mut ctx);
    drop(ctx);
    // Saturates for a point off the framebuffer; the tests ask inside it.
    let (x, y) = (at.x as usize, at.y as usize);
    let i = (y * WIDTH as usize + x) * 4;
    let p = &fb.pixels()[i..i + 4];
    [p[0], p[1], p[2], p[3]]
}

#[test]
fn an_overlay_paints_lifted_with_its_carried_row_and_learns_its_frame() {
    let home = MemoryFolder::new("Home");
    home.add_folder("Parts").expect("Parts is free");
    home.add_file("gear.stl", b"g".to_vec());
    let insets = Rc::new(RefCell::new(Vec::new()));
    let pictures = Rc::new(RedOverGear {
        insets: Rc::clone(&insets),
    });
    let config = BrowserConfig::builder().pictures(pictures).build();
    let (mut app, _) = browser_app(Box::new(home), config, ViewMode::Rows);
    assert!(
        insets.borrow().iter().all(|i| *i == 0.0),
        "a row's square is its edge"
    );
    let square = centre(&app, "Red Square");
    assert_eq!(pixel_at(&mut app, square), [255, 0, 0, 255], "drawn whole");
    drag_over(&mut app, "Row Item gear.stl", "Parts Row Item Collection");
    assert_eq!(property(&app, "Row Item gear.stl", "lifted"), "true");
    let lifted = pixel_at(&mut app, square);
    assert!(
        lifted[0] < 255 || lifted[1] > 0,
        "faded with its row: {lifted:?}"
    );

    let tiles = MemoryFolder::new("Home");
    tiles.add_file("gear.stl", b"g".to_vec());
    let insets = Rc::new(RefCell::new(Vec::new()));
    let pictures = Rc::new(RedOverGear {
        insets: Rc::clone(&insets),
    });
    let config = BrowserConfig::builder().pictures(pictures).build();
    let _ = browser_app(Box::new(tiles), config, ViewMode::Icons);
    assert!(
        !insets.borrow().is_empty() && insets.borrow().iter().all(|i| *i == 1.0),
        "a tile's square is inside its border"
    );
}

#[test]
fn the_folder_the_items_are_in_takes_no_drop() {
    let (mut app, state, desk, shelf) = two_places(BrowserConfig::default(), false);
    let at = drag_over(&mut app, "Row Item gear.stl", "Desk Tree Node");
    assert_eq!(
        property(&app, "Desk Tree Node", "drop_highlighted"),
        "false"
    );
    assert_eq!(property(&app, "Desk Tree Node", "drop_label"), "");
    release(&mut app, at, Modifiers::default());
    assert_eq!(names(&desk), ["gear.stl"]);
    assert!(names(&shelf).is_empty());
    assert!(!state.borrow().history().can_undo(), "nothing was recorded");
}

#[test]
fn a_drop_into_the_source_folder_itself_moves_nothing() {
    let dir = workshop("into-source");
    let (_app, state) = browser_app(disk(&dir), BrowserConfig::default(), ViewMode::Rows);
    let items: Vec<Rc<dyn Item>> = state
        .borrow()
        .entries()
        .iter()
        .filter(|e| e.item().name() == "gear.stl")
        .map(|e| e.item())
        .collect();
    assert_eq!(items.len(), 1);
    let moved = agg_gui_file_browser::drop_into(&state, disk(&dir), "here", &items, 0, false);
    assert!(!moved);
    assert!(dir.join("gear.stl").exists() && !dir.join("gear (2).stl").exists());
    assert!(!state.borrow().history().can_undo(), "nothing was recorded");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_tree_row_past_the_trees_edge_takes_no_drop() {
    let (_app, state, _desk, shelf) = two_places(BrowserConfig::default(), false);
    let gear: Vec<Rc<dyn Item>> = state.borrow().entries().iter().map(|e| e.item()).collect();
    {
        let mut s = state.borrow_mut();
        let drop = s.drop_manager_mut();
        drop.clear_targets(DropTargetKind::Tree);
        drop.clear_targets(DropTargetKind::Content);
        // The tree shows from y 200 up; Shelf's row was scrolled below its bottom edge.
        drop.set_tree_rect(Rect::new(0.0, 200.0, TREE, 200.0));
        drop.register_target(
            Rect::new(0.0, 150.0, TREE, 24.0),
            shelf.link(),
            DropKey {
                kind: DropTargetKind::Tree,
                index: 1,
            },
        );
        drop.begin(gear, 1);
    }
    agg_gui_file_browser::drag_over(&state, Point::new(50.0, 160.0));
    assert_eq!(state.borrow().drop_manager().highlighted(), None);
    // Inside the tree, the same row takes it.
    state
        .borrow_mut()
        .drop_manager_mut()
        .set_tree_rect(Rect::new(0.0, 100.0, TREE, 300.0));
    agg_gui_file_browser::drag_over(&state, Point::new(50.0, 160.0));
    assert!(state.borrow().drop_manager().highlighted().is_some());
}

#[test]
fn a_drop_holding_a_protected_item_moves_nothing_and_says_why() {
    let home = MemoryFolder::new("Home");
    let parts = home.add_folder("Parts").expect("free");
    let gear = home.add_file("gear.stl", b"g".to_vec());
    let state = BrowserState::new(Box::new(home.clone()), BrowserConfig::default());
    let top: Rc<dyn Item> = MemoryFolder::new("Desk").link();
    assert!(top.is_protected());
    let items = vec![gear, top];
    let moved =
        agg_gui_file_browser::drop_into(&state, Box::new(parts.clone()), "Parts", &items, 0, false);
    assert!(!moved);
    assert_eq!(names(&home), ["gear.stl"], "nothing moved");
    assert!(names(&parts).is_empty());
    assert_eq!(
        state.borrow().status(),
        "Couldn't move to \"Parts\": Protected items can't be moved"
    );
    assert!(!state.borrow().history().can_undo());
}

/// A folder a search found by its name takes a drop like any folder row: gear.stl, found beside
/// it by the same search ("ar"), dropped on the found Parts lands in it.
#[test]
fn a_row_dropped_on_a_folder_found_by_search_moves_into_it() {
    let dir = workshop("search-result");
    let (mut app, state) = browser_app(disk(&dir), BrowserConfig::default(), ViewMode::Rows);
    let spawner = Rc::new(agg_gui_file_browser::LocalSpawner::new(Rc::new(
        |m: &str| panic!("fault: {m}"),
    )));
    // The folder the browser shows is loaded; the search reads it as it stands.
    let mut root = disk(&dir);
    root.load();
    let mut results = agg_gui_files::SearchResultsFolder::new(root.as_ref(), "ar", spawner);
    results.start_search();
    for _ in 0..1000 {
        if results.is_finished() {
            break;
        }
        run_ready_local_tasks();
    }
    assert!(results.is_finished(), "the search ends");
    state.borrow_mut().push_search_results(Rc::new(results));
    frame(&mut app);
    let target = "Parts Row Item Collection";
    let at = drag_over(&mut app, "Row Item gear.stl", target);
    // Search results are no place of their own, so the drop is the between-places one: a copy.
    assert_eq!(property(&app, target, "drop_label"), "Copy to Parts");
    release(&mut app, at, Modifiers::default());
    assert!(dir.join("Parts/gear.stl").exists());
    let _ = std::fs::remove_dir_all(&dir);
}
