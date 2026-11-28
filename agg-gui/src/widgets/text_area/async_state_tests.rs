//! `TextArea` and the async-state epoch: the area opts out of async-state
//! re-rasters while its cache signature is current
//! (`Widget::depends_on_async_state` in `widget_impl.rs`), so everything its
//! cached bitmap shows must be in that signature, and a band strip update on a
//! frame where the epoch moved must still match a full raster.
//!
//! Each test runs through `App` (layout, the async-state walk, paint) on a UI
//! thread bound to a queue of its own, with workers waking it through that
//! queue's handle, so parallel tests never see each other's wakes. In-crate
//! because the strip check reads the area's strip-raster counter.

use std::sync::Arc;

use super::*;
use crate::framebuffer::Framebuffer;
use crate::gfx_ctx::GfxCtx;
use crate::widget::App;

const FONT_BYTES: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
}

/// The calling test thread as a UI thread on a fresh queue of its own until
/// dropped (also when an assertion fails).
struct OwnUiThread;

impl OwnUiThread {
    fn new() -> Self {
        crate::ui_thread::mark_current_thread_as_ui_thread_with_own_queue();
        crate::animation::clear_draw_request();
        OwnUiThread
    }
}

impl Drop for OwnUiThread {
    fn drop(&mut self) {
        crate::ui_thread::unbind_current_thread();
    }
}

/// One host frame of `app` at `w × h`: drain, lay out, paint into a fresh
/// framebuffer. Returns it and the backbuffer rasters the frame did.
fn frame(app: &mut App, w: f64, h: f64) -> (Framebuffer, u64) {
    let before = crate::paint_work::paint_work();
    crate::ui_thread::invoke_pending_actions();
    app.layout(Size::new(w, h));
    let mut fb = Framebuffer::new(w as u32, h as u32);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        app.paint(&mut ctx);
    }
    (
        fb,
        crate::paint_work::paint_work()
            .since(before)
            .backbuffer_rasters,
    )
}

fn area(app: &mut App) -> &mut TextArea {
    app.root_mut()
        .as_any_mut()
        .and_then(|any| any.downcast_mut::<TextArea>())
        .expect("the root is a TextArea")
}

fn differing_pixels(a: &Framebuffer, b: &Framebuffer) -> usize {
    a.pixels()
        .chunks_exact(4)
        .zip(b.pixels().chunks_exact(4))
        .filter(|(x, y)| x != y)
        .count()
}

/// Wakes this UI thread from a worker, with a change the area never saw.
fn signal_from_worker() {
    let queue = crate::ui_thread::current_queue();
    std::thread::spawn(move || queue.signal_async_state_change())
        .join()
        .expect("worker signals");
}

const WRAPPING: &str = "one two three four five six seven eight nine ten eleven twelve";

#[test]
fn set_word_wrap_shows_the_new_wrapping_on_the_next_frame() {
    let _ui = OwnUiThread::new();
    let (w, h) = (240.0, 160.0);
    let unwrapped = |wrap: bool| {
        App::new(Box::new(
            TextArea::new(font())
                .with_word_wrap(wrap)
                .with_text(WRAPPING),
        ))
    };
    let (reference, _) = frame(&mut unwrapped(false), w, h);

    let mut app = unwrapped(true);
    frame(&mut app, w, h);
    let (before, rasters) = frame(&mut app, w, h);
    assert_eq!(rasters, 0, "setup: the area paints from its cache");
    assert_ne!(
        differing_pixels(&before, &reference),
        0,
        "setup: wrapping changes what is painted"
    );

    area(&mut app).set_word_wrap(false);
    let (after, _) = frame(&mut app, w, h);
    let differing = differing_pixels(&after, &reference);
    // A wake must not be what makes it show, nor hide that it did not.
    signal_from_worker();
    let (after_wake, _) = frame(&mut app, w, h);
    assert_eq!(
        differing, 0,
        "the frame after set_word_wrap(false) must show the text unwrapped, \
         but {differing} pixels differ from a fresh unwrapped area"
    );
    assert_eq!(differing_pixels(&after_wake, &reference), 0);
}

/// A document far taller than the viewport, so the over-scan band is active.
fn tall_doc() -> String {
    (0..200)
        .map(|i| format!("line number {i} with some content"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Types one character at the start of the top (visible) line through the
/// area's own edit funnel (`edit_ops.rs`), as a keystroke does.
fn type_on_top_line(ta: &mut TextArea) {
    ta.set_cursor_to_start();
    ta.insert_str("X");
}

#[test]
fn a_band_strip_update_on_a_frame_the_epoch_moved_matches_a_full_raster() {
    let _ui = OwnUiThread::new();
    let (w, h) = (400.0, 300.0);
    let editor = |text: String| App::new(Box::new(TextArea::new(font()).with_text(text)));

    // A fresh area given the same edit before its first paint: a full
    // raster of the post-edit document, caret where the edit leaves it.
    let mut reference_app = editor(tall_doc());
    type_on_top_line(area(&mut reference_app));
    let (reference, _) = frame(&mut reference_app, w, h);
    assert_eq!(
        area(&mut reference_app).debug_strip_raster_count(),
        0,
        "setup: the reference is a full raster"
    );

    let mut app = editor(tall_doc());
    frame(&mut app, w, h);
    assert!(area(&mut app).band.active, "setup: the band is active");
    let strips_before = area(&mut app).debug_strip_raster_count();

    // An edit and a worker's wake before the same frame: the area's
    // signature is current again after that frame's layout, so the epoch
    // does not force a full raster and the edit takes the strip path.
    type_on_top_line(area(&mut app));
    signal_from_worker();
    let (shown, _) = frame(&mut app, w, h);
    assert_eq!(
        area(&mut app).debug_strip_raster_count(),
        strips_before + 1,
        "the edit re-rasters only its strip on a frame where the epoch moved"
    );
    let differing = differing_pixels(&shown, &reference);
    assert_eq!(
        differing, 0,
        "a strip update on a frame where the epoch moved must match a full \
         raster, but {differing} pixels differ"
    );
}
