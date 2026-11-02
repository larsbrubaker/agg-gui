//! The `disk` feature's waking directory monitor, through agg-gui's `App`: with it registered, a
//! file written into the disk folder a browser shows, by something outside the app, wakes the UI
//! (the host's waker runs, from the watch's thread) and the list shows the file at its next
//! frames, with no event from the user.

use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use agg_gui::text::Font;
use agg_gui::{App, Size, Widget};
use agg_gui_file_browser::{
    register_directory_monitor, BrowserConfig, BrowserState, FileBrowser, ViewMode,
};
use agg_gui_files::disk::{set_directory_monitor, AllFiles, DiskFolder};

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(agg_gui::fonts::NOTO_SANS_REGULAR).expect("bundled font loads"))
}

fn has(widget: &dyn Widget, id: &str) -> bool {
    widget.id() == Some(id) || widget.children().iter().any(|c| has(c.as_ref(), id))
}

#[test]
fn a_file_written_outside_wakes_the_ui_and_the_list_shows_it() {
    agg_gui::ui_thread::mark_current_thread_as_ui_thread_with_own_queue();
    let wakes = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&wakes);
    agg_gui::animation::set_host_waker(move || {
        counted.fetch_add(1, Ordering::SeqCst);
    });
    let dir =
        std::env::temp_dir().join(format!("agg-gui-file-browser-live-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("made");
    std::fs::write(dir.join("gear.stl"), b"solid gear").expect("written");
    register_directory_monitor();
    agg_gui::set_device_scale(1.0);
    let root = DiskFolder::new(&dir, Rc::new(AllFiles));
    let state = BrowserState::new(Box::new(root), BrowserConfig::default());
    state.borrow_mut().set_view_mode(ViewMode::Rows);
    let mut app = App::new(Box::new(FileBrowser::new(&state, font())));
    let size = Size::new(600.0, 400.0);
    app.layout(size);
    assert!(has(app.root(), "Row Item gear.stl"));
    // Let the watch take its first look before the folder changes.
    std::thread::sleep(Duration::from_millis(300));
    agg_gui::ui_thread::invoke_pending_actions();
    wakes.store(0, Ordering::SeqCst);
    std::fs::write(dir.join("bolt.stl"), b"solid bolt").expect("written");
    let start = Instant::now();
    // No frame runs meanwhile: only the watch's report can wake the UI.
    while wakes.load(Ordering::SeqCst) == 0 {
        assert!(start.elapsed() < Duration::from_secs(5), "no wake came");
        std::thread::sleep(Duration::from_millis(20));
    }
    agg_gui::ui_thread::invoke_pending_actions();
    let start = Instant::now();
    while !has(app.root(), "Row Item bolt.stl") {
        assert!(start.elapsed() < Duration::from_secs(5), "never relisted");
        app.layout(size);
        std::thread::sleep(Duration::from_millis(20));
    }
    drop(app);
    set_directory_monitor(None);
    agg_gui::ui_thread::unbind_current_thread();
    let _ = std::fs::remove_dir_all(&dir);
}
