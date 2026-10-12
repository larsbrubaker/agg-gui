//! The process-wide default remote image loader
//! (`agg_gui::set_default_remote_image_loader`, `widgets/markdown/image_loader.rs`).
//!
//! In a test binary of its own because the default is process-wide state:
//! the library's unit tests paint remote markdown images with no loader and
//! would see a default installed here. A fake loader records requests and
//! completes them on the test thread; no socket is opened.

use std::sync::{Arc, Mutex};

use agg_gui::{
    default_remote_image_loader, remote_images_in_flight, set_default_remote_image_loader, App,
    Font, Framebuffer, GfxCtx, MarkdownView, RemoteImageDone, RemoteImageLoader, Size,
};

const TEST_FONT: &[u8] = include_bytes!("../../demo/assets/LiberationSans-Regular.ttf");
const URL: &str = "https://example.com/red.png";
const VIEWPORT: Size = Size {
    width: 200.0,
    height: 200.0,
};

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

fn frame(app: &mut App) -> Framebuffer {
    app.layout(VIEWPORT);
    let mut fb = Framebuffer::new(VIEWPORT.width as u32, VIEWPORT.height as u32);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        app.paint(&mut ctx);
    }
    fb
}

fn has_pure_red(fb: &Framebuffer) -> bool {
    fb.pixels()
        .chunks_exact(4)
        .any(|p| p[0] > 240 && p[1] < 15 && p[2] < 15 && p[3] > 240)
}

fn markdown_app() -> App {
    let font = Arc::new(Font::from_slice(TEST_FONT).expect("test font"));
    App::new(Box::new(MarkdownView::new(
        format!("![remote]({URL})"),
        font,
    )))
}

#[test]
fn rust_only_default_loader_serves_views_without_their_own() {
    // None by default: a view paints its placeholder and asks for nothing.
    assert!(default_remote_image_loader().is_none());
    let mut idle = markdown_app();
    assert!(!has_pure_red(&frame(&mut idle)));
    assert_eq!(remote_images_in_flight(), 0);

    let loader = Arc::new(FakeLoader::default());
    set_default_remote_image_loader(Some(loader.clone()));
    let mut app = markdown_app();
    frame(&mut app);
    assert_eq!(
        *loader.requests.lock().expect("requests"),
        vec![URL.to_string()]
    );
    assert_eq!(remote_images_in_flight(), 1);

    let done = loader.pending.lock().expect("pending").remove(0);
    done.deliver_rgba([255u8, 0, 0, 255].repeat(16 * 16), 16, 16);
    assert_eq!(remote_images_in_flight(), 0);
    assert!(has_pure_red(&frame(&mut app)), "the delivered image shows");

    set_default_remote_image_loader(None);
    assert!(default_remote_image_loader().is_none());
}
