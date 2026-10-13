//! The rich editor offers `file://` pictures to the installed remote image
//! loader, the rule `MarkdownView` follows (`widgets/markdown/image_loader.rs`),
//! so a host reads local pictures off the UI frame.
//!
//! A fake loader records the urls it is asked for and holds each completion, so
//! a test finishes a load at the moment it chooses.  No test installs the
//! process-wide default loader (it is process state); each gives the editor its
//! own with `set_remote_image_loader`.

use std::sync::{Arc, Mutex};

use agg_gui::widgets::markdown_editing::RichMarkdownEditWidget;
use agg_gui::{RemoteImageDone, RemoteImageLoader, Widget};

use super::rich_markdown_edit_widget_tests::{fonts, Container};

const FILE_URL: &str = "file:///pictures/red.png";
const MISSING_URL: &str = "file:///pictures/does-not-exist-ag12.png";

#[derive(Default)]
struct FakeLoader {
    requests: Mutex<Vec<String>>,
    pending: Mutex<Vec<RemoteImageDone>>,
}

impl RemoteImageLoader for FakeLoader {
    fn load(&self, url: &str, done: RemoteImageDone) {
        self.requests
            .lock()
            .expect("requests")
            .push(url.to_string());
        self.pending.lock().expect("pending").push(done);
    }
}

impl FakeLoader {
    fn requests(&self) -> Vec<String> {
        self.requests.lock().expect("requests").clone()
    }
    fn take(&self, index: usize) -> RemoteImageDone {
        self.pending.lock().expect("pending").remove(index)
    }
}

/// A host's `file://` loader: reads the file on a worker thread, as a real
/// host would, and completes the load there.
#[derive(Default)]
struct FileLoader {
    asked: Mutex<Vec<String>>,
    workers: Mutex<Vec<std::thread::JoinHandle<()>>>,
}

impl RemoteImageLoader for FileLoader {
    fn load(&self, url: &str, done: RemoteImageDone) {
        self.asked.lock().expect("asked").push(url.to_string());
        let path = url.trim_start_matches("file://").to_string();
        let worker = std::thread::spawn(move || match std::fs::read(path) {
            Ok(bytes) => done.deliver_bytes(&bytes),
            Err(_) => done.fail(),
        });
        self.workers.lock().expect("workers").push(worker);
    }
}

impl FileLoader {
    fn finish(&self) {
        for worker in self.workers.lock().expect("workers").drain(..) {
            worker.join().expect("worker");
        }
    }
}

fn red_png() -> Vec<u8> {
    let rgba = [255u8, 0, 0, 255].repeat(16 * 16);
    agg_gui::screenshot::encode_png_rgba(&rgba, 16, 16).expect("encode test PNG")
}

fn has_pure_red(c: &Container) -> bool {
    (0..c.fb.height() as i32).any(|y| {
        (0..c.fb.width() as i32).any(|x| {
            let p = c.pixel(x, y);
            p[0] > 240 && p[1] < 15 && p[2] < 15
        })
    })
}

fn editor_with(markdown: &str, loader: Arc<dyn RemoteImageLoader>) -> Container {
    let mut editor = RichMarkdownEditWidget::new(fonts(), None);
    editor.set_remote_image_loader(Some(loader));
    container_of(editor, markdown)
}

/// `editor` showing `markdown`, laid out (which starts the pictures' loads).
fn container_of(mut editor: RichMarkdownEditWidget, markdown: &str) -> Container {
    editor.set_markdown(markdown).unwrap();
    let mut container = Container {
        editor,
        fb: agg_gui::Framebuffer::new(400, 300),
    };
    container.perform_layout(400.0, 300.0);
    container
}

#[test]
fn rust_only_file_picture_goes_to_the_loader_not_to_a_read_on_the_frame() {
    let loader = Arc::new(FakeLoader::default());
    // The file does not exist: a read on the frame would fail at once, while a
    // load handed to the loader stays pending.
    let mut c = editor_with(&format!("![pic]({MISSING_URL})"), loader.clone());
    c.draw();
    assert_eq!(loader.requests(), vec![MISSING_URL.to_string()]);
    assert!(
        c.editor.needs_draw(),
        "the editor keeps drawing while the loader works"
    );
    assert!(!has_pure_red(&c));

    // Asked once, however many frames paint.
    c.draw();
    assert_eq!(loader.requests().len(), 1);
}

#[test]
fn rust_only_file_picture_shows_after_the_loader_delivers() {
    let loader = Arc::new(FakeLoader::default());
    let mut c = editor_with(&format!("![pic]({FILE_URL})"), loader.clone());
    c.draw();
    assert_eq!(loader.requests(), vec![FILE_URL.to_string()]);

    loader.take(0).deliver_bytes(&red_png());
    c.draw();
    assert!(has_pure_red(&c), "the delivered picture is drawn");
    assert!(!c.editor.needs_draw(), "nothing is left to wait for");
}

#[test]
fn rust_only_resolved_relative_picture_goes_to_the_loader_as_a_file_url() {
    let loader = Arc::new(FakeLoader::default());
    let mut editor = RichMarkdownEditWidget::new(fonts(), None);
    editor.set_remote_image_loader(Some(loader.clone()));
    editor.set_resolve_image_url(Some(Box::new(|url| format!("file:///docs/{url}"))));
    let mut c = container_of(editor, "![pic](red.png)");
    c.draw();
    assert_eq!(loader.requests(), vec!["file:///docs/red.png".to_string()]);
}

#[test]
fn rust_only_file_picture_without_a_loader_makes_no_request_and_stays_a_placeholder() {
    let editor = RichMarkdownEditWidget::new(fonts(), None);
    let mut c = container_of(editor, &format!("![pic]({FILE_URL})"));
    c.draw();
    assert!(!c.editor.needs_draw(), "a failed load is not waited for");
    assert!(!has_pure_red(&c));
}

#[test]
fn rust_only_a_real_file_loads_through_a_loader_thread() {
    let dir = std::env::temp_dir().join(format!("agg_gui_ag12_{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("red.png");
    std::fs::write(&path, red_png()).expect("write png");

    let loader = Arc::new(FileLoader::default());
    let mut c = editor_with(
        &format!("![pic](file://{})", path.display()),
        loader.clone(),
    );
    c.draw();
    loader.finish();
    c.draw();
    assert!(has_pure_red(&c));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn rust_only_a_missing_file_ends_as_the_failed_load_the_viewer_shows() {
    let loader = Arc::new(FileLoader::default());
    let mut c = editor_with(&format!("![pic]({MISSING_URL})"), loader.clone());
    c.draw();
    loader.finish();
    c.draw();

    // The loader was asked, and its failure ended the load.
    assert_eq!(
        *loader.asked.lock().expect("asked"),
        vec![MISSING_URL.to_string()]
    );
    // The same end state as `MarkdownView`'s failed image: no pixels and
    // nothing left to redraw for.
    assert!(!c.editor.needs_draw());
    assert!(!has_pure_red(&c));
}
