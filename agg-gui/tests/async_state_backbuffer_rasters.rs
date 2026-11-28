//! A cached widget repaints when what it shows changes, and only then, whatever
//! wakes the UI thread.
//!
//! Queuing UI work (`ui_thread::run_on_idle`, `run_on_idle_after`,
//! `set_interval`) and a worker's `UiQueue::signal_async_state_change` wake the
//! UI thread that drains the queue (`ui_thread/queue.rs`, `animation.rs`). A
//! widget with a CPU backbuffer (`widget/backbuffer.rs`) paints from its cached
//! bitmap until something it shows changes (`widget/paint/offscreen.rs`). Two
//! halves pin how the two meet:
//!
//! - **A wake that changed nothing a widget shows leaves its cache alone.** An
//!   `App` of cached `Label`s paints one frame after each kind of wake, and
//!   `agg_gui::paint_work` counts the backbuffer rasters that frame did: none,
//!   since no label's text, colour, size, theme or typography moved.
//! - **An async result still reaches the screen.** A result that lands in
//!   shared state outside every widget's mutation path, under a
//!   CPU-backbuffered ancestor nothing invalidates, shows in the next frame: a
//!   markdown picture a loader delivers on a worker thread
//!   (`widgets/markdown/image_loader.rs`, whose completion signals the UI
//!   thread's queue), and a fixture's colour that a worker sets and signals, or
//!   delivers through `UiQueue::run_on_idle` with no signal of its own (the
//!   way C#'s `UiThread.RunOnIdle` hands a result back).
//!
//! Each test's thread is a UI thread on a queue of its own
//! (`ui_thread::mark_current_thread_as_ui_thread_with_own_queue`), and its
//! workers wake it through that queue's handle, so tests running in parallel
//! never see each other's wakes; the raster counts are per thread too.
//! [`frame`] does what a host frame does: drain the queue as the shells do
//! before painting, lay out, paint.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use agg_gui::animation::clear_draw_request;
use agg_gui::ui_thread::{self, UiQueue};
use agg_gui::widget::{BackbufferCache, BackbufferMode};
use agg_gui::{
    paint_work, App, Color, DrawCtx, Event, EventResult, FlexColumn, Font, Framebuffer, GfxCtx,
    Label, MarkdownView, Rect, RemoteImageDone, RemoteImageLoader, Size, Widget,
};

const FONT_BYTES: &[u8] = include_bytes!("../../demo/assets/CascadiaCode.ttf");

const VIEWPORT: Size = Size {
    width: 240.0,
    height: 160.0,
};

/// How many cached labels the labels app shows.
const LABELS: u64 = 4;

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(FONT_BYTES).expect("test font"))
}

/// The calling test thread as a UI thread bound to a fresh queue of its own,
/// starting idle, until dropped (also when an assertion fails).
struct OwnUiThread;

impl OwnUiThread {
    fn new() -> Self {
        let _queue: UiQueue = ui_thread::mark_current_thread_as_ui_thread_with_own_queue();
        clear_draw_request();
        OwnUiThread
    }
}

impl Drop for OwnUiThread {
    fn drop(&mut self) {
        ui_thread::unbind_current_thread();
    }
}

/// One host frame: drain the UI queue as the shells do before every painted
/// frame (`ui_thread::invoke_pending_actions`), lay out, paint into a fresh
/// framebuffer. Returns the framebuffer and the backbuffer rasters the frame
/// did.
fn painted_frame(app: &mut App) -> (Framebuffer, u64) {
    let before = paint_work();
    ui_thread::invoke_pending_actions();
    app.layout(VIEWPORT);
    let mut fb = Framebuffer::new(VIEWPORT.width as u32, VIEWPORT.height as u32);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        app.paint(&mut ctx);
    }
    (fb, paint_work().since(before).backbuffer_rasters)
}

/// [`painted_frame`]'s backbuffer rasters.
fn frame(app: &mut App) -> u64 {
    painted_frame(app).1
}

// ── A wake that changed nothing leaves cached labels cached ──────────────────

/// A column of [`LABELS`] labels, each on a CPU backbuffer, warmed: the first
/// frame rasters each label once and the second paints them all from their
/// caches (which proves the caches work).
fn warmed_labels_app() -> App {
    let font = font();
    let mut column = FlexColumn::new();
    for i in 0..LABELS {
        column = column.add(Box::new(
            Label::new(format!("cached label {i}"), Arc::clone(&font))
                .with_font_size(14.0)
                .with_color(Color::black())
                .with_has_backbuffer(true),
        ));
    }
    let mut app = App::new(Box::new(column));
    let first = frame(&mut app);
    assert_eq!(first, LABELS, "the first frame rasters each label once");
    let second = frame(&mut app);
    assert_eq!(
        second, 0,
        "setup: a frame after nothing happened paints every label from its \
         cache, but {second} backbuffers rastered"
    );
    app
}

/// The frame after `wake` did `rasters` backbuffer rasters; none of the cached
/// labels' pixels depend on it.
fn assert_labels_stay_cached(wake: &str, rasters: u64) {
    assert_eq!(
        rasters, 0,
        "after {wake}, {rasters} backbuffers re-rastered ({LABELS} cached labels \
         on screen, none of whose pixels depend on it); a cached label must \
         paint from its cache"
    );
}

#[test]
fn queued_idle_work_leaves_cached_labels_cached() {
    let _ui = OwnUiThread::new();
    let mut app = warmed_labels_app();
    ui_thread::run_on_idle(|| {});
    assert_labels_stay_cached("ui_thread::run_on_idle(|| {})", frame(&mut app));
}

#[test]
fn delayed_idle_work_leaves_cached_labels_cached() {
    let _ui = OwnUiThread::new();
    let mut app = warmed_labels_app();
    ui_thread::run_on_idle_after(Duration::from_secs(60), || {});
    assert_labels_stay_cached("ui_thread::run_on_idle_after(60 s, || {})", frame(&mut app));
}

#[test]
fn an_interval_leaves_cached_labels_cached() {
    let _ui = OwnUiThread::new();
    let mut app = warmed_labels_app();
    let interval = ui_thread::set_interval(|| {}, Duration::from_secs(60));
    let rasters = frame(&mut app);
    // Cleared before asserting, so a failing run stops it too.
    ui_thread::clear_interval(&interval);
    assert_labels_stay_cached("ui_thread::set_interval(|| {}, 60 s)", rasters);
}

#[test]
fn a_worker_wake_that_changed_nothing_leaves_cached_labels_cached() {
    let _ui = OwnUiThread::new();
    let mut app = warmed_labels_app();
    let queue = ui_thread::current_queue();
    std::thread::spawn(move || queue.signal_async_state_change())
        .join()
        .expect("worker signals");
    assert_labels_stay_cached(
        "a worker's signal_async_state_change that changed nothing a label shows",
        frame(&mut app),
    );
}

// ── An async result still repaints a cached widget ───────────────────────────

/// A container with a CPU backbuffer (`Rgba`) that lays its children over its
/// whole bounds and paints them into its bitmap. It invalidates its cache only
/// when its size changes, so a child's new pixels show only when the framework
/// re-rasters it.
struct CachedPanel {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    cache: BackbufferCache,
}

impl CachedPanel {
    fn new(child: Box<dyn Widget>) -> Self {
        Self {
            bounds: Rect::default(),
            children: vec![child],
            cache: BackbufferCache::new(),
        }
    }
}

impl Widget for CachedPanel {
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        if b.width != self.bounds.width || b.height != self.bounds.height {
            self.cache.invalidate();
        }
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
            child.layout(available);
            child.set_bounds(Rect::new(0.0, 0.0, available.width, available.height));
        }
        available
    }
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        ctx.set_fill_color(Color::white());
        ctx.begin_path();
        ctx.rect(0.0, 0.0, self.bounds.width, self.bounds.height);
        ctx.fill();
    }
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
    fn backbuffer_cache_mut(&mut self) -> Option<&mut BackbufferCache> {
        Some(&mut self.cache)
    }
    fn backbuffer_mode(&self) -> BackbufferMode {
        BackbufferMode::Rgba
    }
}

/// Fills its bounds with the colour `shown` holds when it paints, and
/// invalidates nothing when that colour changes: a result (a decoded image)
/// landing in shared state, not through a widget's setter.
struct Swatch {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    shown: Arc<Mutex<Color>>,
}

impl Widget for Swatch {
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
        available
    }
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let color = *self.shown.lock().expect("shown colour");
        ctx.set_fill_color(color);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, self.bounds.width, self.bounds.height);
        ctx.fill();
    }
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

const BEFORE: [u8; 3] = [200, 40, 40];
const AFTER: [u8; 3] = [30, 160, 60];

fn color(rgb: [u8; 3]) -> Color {
    Color::from_rgb8(rgb[0], rgb[1], rgb[2])
}

/// The RGB of the framebuffer pixel at the viewport's centre, under the swatch.
fn centre_rgb(fb: &Framebuffer) -> [u8; 3] {
    let (x, y) = (fb.width() / 2, fb.height() / 2);
    let i = ((y * fb.width() + x) * 4) as usize;
    let p = &fb.pixels()[i..i + 4];
    [p[0], p[1], p[2]]
}

fn close_to(actual: [u8; 3], expected: [u8; 3]) -> bool {
    actual.iter().zip(expected).all(|(a, e)| a.abs_diff(e) <= 2)
}

/// A cached panel over a swatch showing [`BEFORE`], warmed: the second frame
/// paints it from the panel's cache. Returns the app and the swatch's colour.
fn warmed_swatch_app() -> (App, Arc<Mutex<Color>>) {
    let shown = Arc::new(Mutex::new(color(BEFORE)));
    let swatch = Swatch {
        bounds: Rect::default(),
        children: Vec::new(),
        shown: Arc::clone(&shown),
    };
    let mut app = App::new(Box::new(CachedPanel::new(Box::new(swatch))));
    painted_frame(&mut app);
    let (fb, rasters) = painted_frame(&mut app);
    assert_eq!(rasters, 0, "setup: the panel paints from its cache");
    assert!(
        close_to(centre_rgb(&fb), BEFORE),
        "setup: the swatch shows its first colour, got {:?}",
        centre_rgb(&fb)
    );
    (app, shown)
}

#[test]
fn a_result_a_worker_signals_repaints_its_cached_ancestor() {
    let _ui = OwnUiThread::new();
    let (mut app, shown) = warmed_swatch_app();
    let queue = ui_thread::current_queue();
    std::thread::spawn(move || {
        *shown.lock().expect("shown colour") = color(AFTER);
        queue.signal_async_state_change();
    })
    .join()
    .expect("worker delivers");
    let (fb, _) = painted_frame(&mut app);
    assert!(
        close_to(centre_rgb(&fb), AFTER),
        "the frame after a worker set the colour and signalled must show it \
         ({AFTER:?}) inside the cached panel, got {:?}",
        centre_rgb(&fb)
    );
}

#[test]
fn a_result_delivered_through_run_on_idle_repaints_its_cached_ancestor() {
    let _ui = OwnUiThread::new();
    let (mut app, shown) = warmed_swatch_app();
    let queue = ui_thread::current_queue();
    // The worker hands the result to the UI thread and signals nothing itself.
    std::thread::spawn(move || {
        queue.run_on_idle(move || *shown.lock().expect("shown colour") = color(AFTER));
    })
    .join()
    .expect("worker posts");
    let (fb, _) = painted_frame(&mut app);
    assert!(
        close_to(centre_rgb(&fb), AFTER),
        "the frame whose drain ran a worker's run_on_idle result must show it \
         ({AFTER:?}) inside the cached panel, got {:?}",
        centre_rgb(&fb)
    );
}

/// Holds each load's completion for the test to deliver; opens no socket.
#[derive(Default)]
struct HoldingLoader {
    pending: Mutex<Vec<RemoteImageDone>>,
}

impl RemoteImageLoader for HoldingLoader {
    fn load(&self, _url: &str, done: RemoteImageDone) {
        self.pending.lock().expect("pending").push(done);
    }
}

/// Pixels of pure green, the colour of the picture the loader delivers.
fn green_pixels(fb: &Framebuffer) -> usize {
    fb.pixels()
        .chunks_exact(4)
        .filter(|p| p[0] < 15 && p[1] > 240 && p[2] < 15 && p[3] > 240)
        .count()
}

#[test]
fn a_markdown_picture_a_worker_delivers_repaints_its_cached_ancestor() {
    let _ui = OwnUiThread::new();
    let loader = Arc::new(HoldingLoader::default());
    let view = MarkdownView::new("![picture](https://example.com/async.png)", font())
        .with_remote_image_loader(loader.clone());
    let mut app = App::new(Box::new(CachedPanel::new(Box::new(view))));

    // The first frame paints the placeholder and starts the load; the second
    // paints it from the panel's cache.
    painted_frame(&mut app);
    let (fb, rasters) = painted_frame(&mut app);
    assert_eq!(rasters, 0, "setup: the panel paints from its cache");
    assert_eq!(green_pixels(&fb), 0, "setup: the placeholder shows");
    let done = loader
        .pending
        .lock()
        .expect("pending")
        .pop()
        .expect("setup: the picture's load started");

    const SIDE: u32 = 24;
    let picture = [0u8, 255, 0, 255].repeat((SIDE * SIDE) as usize);
    std::thread::spawn(move || done.deliver_rgba(picture, SIDE, SIDE))
        .join()
        .expect("worker delivers");
    let (fb, _) = painted_frame(&mut app);
    let green = green_pixels(&fb);
    assert!(
        green >= (SIDE * SIDE / 2) as usize,
        "the frame after a worker delivered the picture must show it inside \
         the cached panel: {green} of its {} pixels are on screen",
        SIDE * SIDE
    );
}
