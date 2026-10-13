//! Remote images in `MarkdownView` load only through a host-supplied
//! [`RemoteImageLoader`] (`image_loader.rs`).
//!
//! A fake loader records the urls it is asked for and keeps each load's
//! [`RemoteImageDone`], so a test completes loads on its own thread at the
//! moment it chooses; no socket is ever opened. These tests never install the
//! process-wide default loader (other tests in this binary paint remote images
//! with no loader); `tests/markdown_default_image_loader.rs` covers the
//! default in a process of its own.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::image_loader::{remote_images_in_flight, RemoteImageDone, RemoteImageLoader};
use super::{ImageState, MarkdownView};
use crate::event::{Event, EventResult};
use crate::geometry::{Rect, Size};
use crate::text::Font;
use crate::widget::Widget;

const TEST_FONT: &[u8] = include_bytes!("../../../../demo/assets/LiberationSans-Regular.ttf");
const RED_URL: &str = "https://example.com/red.png";
const GIF_URL: &str = "https://example.com/anim.gif";
const SIDE: f64 = 200.0;

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(TEST_FONT).expect("test font"))
}

/// Records each requested url and holds its completion for the test.
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
    /// The completion of the `index`th load still held.
    fn take(&self, index: usize) -> RemoteImageDone {
        self.pending.lock().expect("pending").remove(index)
    }
}

/// Minimal parent laying the view out at its natural size.
struct Host {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl Widget for Host {
    fn type_name(&self) -> &'static str {
        "Host"
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
        for child in &mut self.children {
            let s = child.layout(available);
            child.set_bounds(Rect::new(
                0.0,
                available.height - s.height,
                s.width,
                s.height,
            ));
        }
        available
    }
    fn paint(&mut self, _ctx: &mut dyn crate::draw_ctx::DrawCtx) {}
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

fn host_with(view: MarkdownView) -> Host {
    crate::widget::set_current_viewport(Size::new(SIDE, SIDE));
    let mut host = Host {
        bounds: Rect::new(0.0, 0.0, SIDE, SIDE),
        children: vec![Box::new(view)],
    };
    host.layout(Size::new(SIDE, SIDE));
    host
}

/// Re-lay out and paint `host`, returning the frame.
fn paint(host: &mut Host) -> crate::framebuffer::Framebuffer {
    host.layout(Size::new(SIDE, SIDE));
    let mut fb = crate::framebuffer::Framebuffer::new(SIDE as u32, SIDE as u32);
    {
        let mut ctx = crate::gfx_ctx::GfxCtx::new(&mut fb);
        crate::widget::paint_subtree(host, &mut ctx);
    }
    fb
}

fn view_of(host: &Host) -> &MarkdownView {
    host.children[0]
        .as_any()
        .expect("as_any")
        .downcast_ref::<MarkdownView>()
        .expect("markdown view")
}

fn view_mut(host: &mut Host) -> &mut MarkdownView {
    host.children[0]
        .as_any_mut()
        .expect("as_any_mut")
        .downcast_mut::<MarkdownView>()
        .expect("markdown view")
}

fn has_pure_red(fb: &crate::framebuffer::Framebuffer) -> bool {
    fb.pixels()
        .chunks_exact(4)
        .any(|p| p[0] > 240 && p[1] < 15 && p[2] < 15 && p[3] > 240)
}

fn red_png() -> Vec<u8> {
    let rgba = [255u8, 0, 0, 255].repeat(16 * 16);
    crate::screenshot::encode_png_rgba(&rgba, 16, 16).expect("encode test PNG")
}

fn is_pending(view: &MarkdownView, index: usize) -> bool {
    matches!(
        *view.image_cache[index].state.lock().expect("state"),
        ImageState::RemotePending
    )
}

#[test]
fn rust_only_no_loader_makes_no_request() {
    let view = MarkdownView::new(format!("![remote]({RED_URL})"), font());
    let mut host = host_with(view);
    let fb = paint(&mut host);

    // Painted on screen, yet nothing was asked for: the image is still
    // pending, nothing is in flight, and the placeholder shows.
    assert!(is_pending(view_of(&host), 0), "no load was started");
    assert_eq!(view_of(&host).remote_images_in_flight(), 0);
    assert!(!has_pure_red(&fb));
    assert!(!view_of(&host).needs_draw(), "no load to poll for");

    // The pending image loads once the host supplies a loader.
    let loader = Arc::new(FakeLoader::default());
    view_mut(&mut host).set_remote_image_loader(Some(loader.clone()));
    paint(&mut host);
    assert_eq!(loader.requests(), vec![RED_URL.to_string()]);
}

#[test]
fn rust_only_fake_loader_bytes_show_the_image() {
    let loader = Arc::new(FakeLoader::default());
    let view = MarkdownView::new(format!("![remote]({RED_URL})"), font())
        .with_remote_image_loader(loader.clone());
    let mut host = host_with(view);

    let fb = paint(&mut host);
    assert_eq!(loader.requests(), vec![RED_URL.to_string()]);
    assert!(!has_pure_red(&fb), "placeholder while loading");
    assert!(view_of(&host).needs_draw(), "polls while loading");

    // A repaint asks for nothing more: one load per image.
    paint(&mut host);
    assert_eq!(loader.requests().len(), 1);

    loader.take(0).deliver_bytes(&red_png());
    assert!(has_pure_red(&paint(&mut host)), "the delivered image shows");
}

#[test]
fn rust_only_fake_loader_rgba_shows_the_image() {
    let loader = Arc::new(FakeLoader::default());
    let view = MarkdownView::new(format!("![remote]({RED_URL})"), font())
        .with_remote_image_loader(loader.clone());
    let mut host = host_with(view);
    paint(&mut host);
    loader
        .take(0)
        .deliver_rgba([255u8, 0, 0, 255].repeat(16 * 16), 16, 16);
    assert!(has_pure_red(&paint(&mut host)));
}

#[test]
fn rust_only_delivered_animated_gif_animates() {
    let _clock = crate::clock::scoped_virtual(None);
    let loader = Arc::new(FakeLoader::default());
    let view = MarkdownView::new(format!("![anim]({GIF_URL})"), font())
        .with_remote_image_loader(loader.clone());
    let mut host = host_with(view);
    paint(&mut host);
    assert_eq!(loader.requests(), vec![GIF_URL.to_string()]);

    let bytes = crate::image_sequence::tests::gif_bytes(
        8,
        8,
        &[([255, 0, 0, 255], 10), ([0, 0, 255, 255], 20)],
    );
    loader.take(0).deliver_bytes(&bytes);
    paint(&mut host);
    assert!(view_of(&host).is_image_animating(GIF_URL));
    assert_eq!(view_of(&host).image_frame_index(GIF_URL), Some(0));

    crate::clock::advance(Duration::from_millis(100));
    paint(&mut host);
    assert_eq!(view_of(&host).image_frame_index(GIF_URL), Some(1));
}

#[test]
fn rust_only_delivered_sequence_animates() {
    let _clock = crate::clock::scoped_virtual(None);
    let loader = Arc::new(FakeLoader::default());
    let view = MarkdownView::new(format!("![anim]({GIF_URL})"), font())
        .with_remote_image_loader(loader.clone());
    let mut host = host_with(view);
    paint(&mut host);
    let bytes = crate::image_sequence::tests::gif_bytes(
        8,
        8,
        &[([255, 0, 0, 255], 10), ([0, 0, 255, 255], 20)],
    );
    let sequence = crate::image_sequence::ImageSequence::load_gif(&bytes).expect("decodes");
    loader.take(0).deliver_sequence(sequence);
    paint(&mut host);
    assert!(view_of(&host).is_image_animating(GIF_URL));
}

#[test]
fn rust_only_in_flight_count_rises_and_falls() {
    let loader = Arc::new(FakeLoader::default());
    let view = MarkdownView::new(format!("![a]({RED_URL})\n\n![b]({GIF_URL})"), font())
        .with_remote_image_loader(loader.clone());
    let mut host = host_with(view);
    assert_eq!(view_of(&host).remote_images_in_flight(), 0);

    paint(&mut host);
    assert_eq!(loader.requests().len(), 2);
    assert_eq!(view_of(&host).remote_images_in_flight(), 2);
    // Other tests' loads may be in flight too, never fewer than ours.
    assert!(remote_images_in_flight() >= 2);

    loader.take(0).fail();
    assert_eq!(view_of(&host).remote_images_in_flight(), 1);

    // A loader that drops a completion fails that load.
    drop(loader.take(0));
    assert_eq!(view_of(&host).remote_images_in_flight(), 0);
    assert!(view_of(&host)
        .image_cache
        .iter()
        .all(|entry| matches!(*entry.state.lock().expect("state"), ImageState::Failed)));
    assert!(!view_of(&host).needs_draw(), "nothing left to poll for");
}

#[test]
fn rust_only_resolver_turns_the_source_into_the_url_that_loads() {
    let loader = Arc::new(FakeLoader::default());
    let view = MarkdownView::new("![pic](images/red.png)", font())
        .with_image_url_resolver(|url| format!("https://example.com/content/{url}"))
        .with_remote_image_loader(loader.clone());
    let mut host = host_with(view);
    paint(&mut host);
    assert_eq!(
        loader.requests(),
        vec!["https://example.com/content/images/red.png".to_string()]
    );

    loader.take(0).deliver_bytes(&red_png());
    assert!(has_pure_red(&paint(&mut host)));
}

#[test]
fn rust_only_resolver_url_reaches_the_image_provider() {
    let asked = Arc::new(Mutex::new(Vec::<String>::new()));
    let seen = Arc::clone(&asked);
    let view = MarkdownView::new("![pic](red.png)", font())
        .with_image_url_resolver(|url| format!("/content/{url}"))
        .with_image_provider(move |url| {
            seen.lock().expect("asked").push(url.to_string());
            Some(([255u8, 0, 0, 255].repeat(16 * 16), 16, 16))
        });
    let mut host = host_with(view);
    assert!(has_pure_red(&paint(&mut host)));
    assert_eq!(*asked.lock().expect("asked"), vec!["/content/red.png"]);
}

#[test]
fn rust_only_no_resolver_loads_the_url_as_written() {
    let loader = Arc::new(FakeLoader::default());
    let view = MarkdownView::new(format!("![remote]({RED_URL})"), font())
        .with_remote_image_loader(loader.clone());
    let mut host = host_with(view);
    paint(&mut host);
    assert_eq!(loader.requests(), vec![RED_URL.to_string()]);
}

#[test]
fn rust_only_loader_is_offered_file_urls() {
    let loader = Arc::new(FakeLoader::default());
    let view = MarkdownView::new("![pic](file:///content/red.png)", font())
        .with_remote_image_loader(loader.clone());
    let mut host = host_with(view);
    paint(&mut host);
    assert_eq!(
        loader.requests(),
        vec!["file:///content/red.png".to_string()]
    );

    loader.take(0).deliver_bytes(&red_png());
    assert!(has_pure_red(&paint(&mut host)));
}

#[test]
fn rust_only_resolver_can_point_a_relative_picture_at_a_file_url() {
    let loader = Arc::new(FakeLoader::default());
    let view = MarkdownView::new("![pic](red.png)", font())
        .with_image_url_resolver(|url| format!("file:///docs/help/{url}"))
        .with_remote_image_loader(loader.clone());
    let mut host = host_with(view);
    paint(&mut host);
    assert_eq!(
        loader.requests(),
        vec!["file:///docs/help/red.png".to_string()]
    );
}

#[test]
fn rust_only_file_url_without_a_loader_stays_failed() {
    let view = MarkdownView::new("![pic](file:///content/red.png)", font());
    let mut host = host_with(view);
    paint(&mut host);
    assert!(matches!(
        *view_of(&host).image_cache[0].state.lock().expect("state"),
        ImageState::Failed
    ));
    assert_eq!(view_of(&host).remote_images_in_flight(), 0);
}
