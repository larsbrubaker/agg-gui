//! `Widget::depends_on_async_state`: which cached widgets an async-state
//! change (a worker's result, queued idle work) re-rasters.
//!
//! `App::paint`'s async-state walk (`widget/tree.rs`) marks dirty only the
//! widgets that answer `true`, and both backbuffer paths
//! (`widget/paint/offscreen.rs`) ignore the epoch for a widget that answers
//! `false`. These tests pin both halves: an opted-out widget keeps its cache
//! after a wake (its setters and event dispatch still dirty it), a default
//! widget under an opted-out one is still marked, every built-in that opts out
//! paints its next frame from its cache, and a result those built-ins show
//! still reaches the screen: a label's text set by a worker's idle action, a
//! text field's hint cell changed after layout, a text area's highlighter
//! reading shared state. A built-in with a child pushed into it keeps
//! answering `true`, as the child paints into its cache. `Label::set_dim`
//! invalidates the label's cache itself, since no async-state re-raster
//! refreshes an opted-out label.
//!
//! Each test's thread is a UI thread on a queue of its own, and its workers
//! wake it through that queue's handle, so parallel tests never see each
//! other's wakes; the raster counts are per thread too. The retained GL layer
//! path is covered in `src/tests/async_state_retained_layer.rs`, where a
//! retained-layer test context lives, and the text area's signature and band
//! strip updates in `src/widgets/text_area/async_state_tests.rs`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::animation::{clear_draw_request, request_draw};
use agg_gui::ui_thread;
use agg_gui::widget::{BackbufferCache, BackbufferMode};
use agg_gui::{
    paint_work, single_font_resolver, App, Color, DrawCtx, Event, EventResult, Font, Framebuffer,
    GfxCtx, Hyperlink, Label, Rect, RichDoc, RichTextEdit, Size, TextArea, TextField, Widget,
};

const FONT_BYTES: &[u8] = include_bytes!("../../demo/assets/CascadiaCode.ttf");

const VIEWPORT: Size = Size {
    width: 240.0,
    height: 160.0,
};

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(FONT_BYTES).expect("test font"))
}

/// The calling test thread as a UI thread bound to a fresh queue of its own,
/// starting idle, until dropped (also when an assertion fails).
struct OwnUiThread;

impl OwnUiThread {
    fn new() -> Self {
        ui_thread::mark_current_thread_as_ui_thread_with_own_queue();
        clear_draw_request();
        OwnUiThread
    }
}

impl Drop for OwnUiThread {
    fn drop(&mut self) {
        ui_thread::unbind_current_thread();
    }
}

/// One host frame: drain the UI queue as the shells do before painting, lay
/// out, paint. Returns the framebuffer and the backbuffer rasters it did.
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

fn frame(app: &mut App) -> u64 {
    painted_frame(app).1
}

/// Lays out and paints `app` twice; the second frame must paint from caches.
fn warm(app: &mut App, what: &str) {
    frame(app);
    let second = frame(app);
    assert_eq!(
        second, 0,
        "setup: {what} paints a frame after nothing happened from its caches, \
         but {second} backbuffers rastered"
    );
}

fn label_mut(app: &mut App) -> &mut Label {
    app.root_mut()
        .as_any_mut()
        .and_then(|any| any.downcast_mut::<Label>())
        .expect("the root is a Label")
}

#[test]
fn set_dim_invalidates_the_label_cache() {
    let _ui = OwnUiThread::new();
    let label = Label::new("dim me", font())
        .with_font_size(14.0)
        .with_has_backbuffer(true);
    let mut app = App::new(Box::new(label));
    warm(&mut app, "a label");

    label_mut(&mut app).set_dim(true);
    let rasters = frame(&mut app);
    assert_eq!(
        rasters, 1,
        "a label whose dim colour changed must re-raster, but {rasters} did"
    );
    label_mut(&mut app).set_dim(true);
    let rasters = frame(&mut app);
    assert_eq!(rasters, 0, "setting the same dim again changes nothing");
}

/// Wakes this UI thread from a worker, through its queue's handle, with a
/// change no widget's setters saw.
fn signal_from_worker() {
    let queue = ui_thread::current_queue();
    std::thread::spawn(move || queue.signal_async_state_change())
        .join()
        .expect("worker signals");
}

// ── Opting out ───────────────────────────────────────────────────────────────

/// A widget that counts `mark_dirty` calls and answers `depends` to
/// `depends_on_async_state`; with a cache it is a CPU-backbuffered leaf or
/// container, whose `MouseMove` asks for a draw (so dispatch dirties it).
struct Probe {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    cache: Option<BackbufferCache>,
    depends: bool,
    marks: Rc<Cell<usize>>,
}

impl Probe {
    fn new(cached: bool, depends: bool) -> (Self, Rc<Cell<usize>>) {
        let marks = Rc::new(Cell::new(0));
        let probe = Self {
            bounds: Rect::default(),
            children: Vec::new(),
            cache: cached.then(BackbufferCache::new),
            depends,
            marks: Rc::clone(&marks),
        };
        (probe, marks)
    }
}

impl Widget for Probe {
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
    fn on_event(&mut self, event: &Event) -> EventResult {
        if matches!(event, Event::MouseMove { .. }) {
            request_draw();
            return EventResult::Consumed;
        }
        EventResult::Ignored
    }
    fn mark_dirty(&mut self) {
        self.marks.set(self.marks.get() + 1);
        if let Some(cache) = self.cache.as_mut() {
            cache.invalidate();
        }
    }
    fn depends_on_async_state(&self) -> bool {
        self.depends
    }
    fn backbuffer_cache_mut(&mut self) -> Option<&mut BackbufferCache> {
        self.cache.as_mut()
    }
    fn backbuffer_mode(&self) -> BackbufferMode {
        BackbufferMode::Rgba
    }
}

#[test]
fn an_opted_out_cached_leaf_keeps_its_cache_after_a_wake_but_not_after_an_event() {
    let _ui = OwnUiThread::new();
    let (leaf, marks) = Probe::new(true, false);
    let mut app = App::new(Box::new(leaf));
    warm(&mut app, "an opted-out leaf");

    marks.set(0);
    signal_from_worker();
    let rasters = frame(&mut app);
    assert_eq!(
        rasters, 0,
        "an opted-out leaf ignores the epoch: {rasters} rasters"
    );
    assert_eq!(
        marks.get(),
        0,
        "the async-state walk does not mark it dirty"
    );

    // An event that asks for a draw still dirties it through dispatch.
    app.on_mouse_move(10.0, 10.0);
    assert!(
        marks.get() > 0,
        "event dispatch marks the opted-out leaf dirty"
    );
    let rasters = frame(&mut app);
    assert_eq!(rasters, 1, "the dirtied leaf re-rasters: {rasters} rasters");
}

#[test]
fn a_default_child_under_an_opted_out_parent_is_still_marked() {
    let _ui = OwnUiThread::new();
    let (mut parent, parent_marks) = Probe::new(true, false);
    let (child, child_marks) = Probe::new(false, true);
    parent.children.push(Box::new(child));
    let mut app = App::new(Box::new(parent));
    warm(&mut app, "an opted-out parent");

    parent_marks.set(0);
    child_marks.set(0);
    signal_from_worker();
    frame(&mut app);
    assert_eq!(parent_marks.get(), 0, "the opted-out parent is not marked");
    assert_eq!(child_marks.get(), 1, "its default child still is, once");
}

// ── Built-ins that opt out paint the frame after a wake from their caches ───

/// `widget` as an App's root, warmed; then a worker's wake must cost the next
/// frame no backbuffer raster.
fn assert_stays_cached_after_a_wake(what: &str, widget: Box<dyn Widget>) {
    let mut app = App::new(widget);
    warm(&mut app, what);
    signal_from_worker();
    let rasters = frame(&mut app);
    assert_eq!(
        rasters, 0,
        "after a worker's wake that changed nothing it shows, a cached {what} \
         re-rastered {rasters} backbuffers; it must paint from its cache"
    );
}

#[test]
fn opted_out_built_ins_paint_from_their_caches_after_a_wake() {
    let _ui = OwnUiThread::new();
    let font = font();
    assert_stays_cached_after_a_wake(
        "Label",
        Box::new(Label::new("label", Arc::clone(&font)).with_has_backbuffer(true)),
    );
    assert_stays_cached_after_a_wake(
        "Hyperlink",
        Box::new(Hyperlink::new("hyperlink", Arc::clone(&font))),
    );
    assert_stays_cached_after_a_wake(
        "TextField",
        Box::new(TextField::new(Arc::clone(&font)).with_text("text field")),
    );
    assert_stays_cached_after_a_wake(
        "TextArea",
        Box::new(TextArea::new(Arc::clone(&font)).with_text("text area\nsecond line")),
    );
    assert_stays_cached_after_a_wake(
        "RichTextEdit",
        Box::new(RichTextEdit::new(
            RichDoc::new(),
            single_font_resolver(Arc::clone(&font)),
        )),
    );
}

#[test]
fn a_built_in_with_a_pushed_child_follows_async_state_changes() {
    let _ui = OwnUiThread::new();
    let font = font();
    let built_ins: Vec<(&str, Box<dyn Widget>)> = vec![
        ("Label", Box::new(Label::new("label", Arc::clone(&font)))),
        (
            "Hyperlink",
            Box::new(Hyperlink::new("link", Arc::clone(&font))),
        ),
        ("TextField", Box::new(TextField::new(Arc::clone(&font)))),
        ("TextArea", Box::new(TextArea::new(Arc::clone(&font)))),
        (
            "RichTextEdit",
            Box::new(RichTextEdit::new(
                RichDoc::new(),
                single_font_resolver(Arc::clone(&font)),
            )),
        ),
    ];
    for (what, widget) in built_ins {
        let mut app = App::new(widget);
        warm(&mut app, what);
        assert!(
            !app.root().depends_on_async_state(),
            "setup: a laid-out {what} with no children opts out"
        );
        let (child, _) = Probe::new(false, true);
        app.root_mut().children_mut().push(Box::new(child));
        assert!(
            app.root().depends_on_async_state(),
            "a {what} with a child painting into its cache must follow the epoch"
        );
    }
}

// ── A result those built-ins show still reaches the screen ───────────────────

/// A container with a CPU backbuffer (`Rgba`) that lays its one child over
/// its whole bounds and invalidates only on a size change, so a child's new
/// pixels show only when the framework re-rasters it.
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
}

/// `child`, bare or inside a [`CachedPanel`].
fn hosted(child: Box<dyn Widget>, in_panel: bool) -> Box<dyn Widget> {
    if in_panel {
        Box::new(CachedPanel::new(child))
    } else {
        child
    }
}

/// `shown` must be pixel for pixel what a fresh app showing the result paints
/// (`reference`), and `before` must not be, so the result is visible.
fn assert_shows_result(
    what: &str,
    before: &Framebuffer,
    shown: &Framebuffer,
    reference: &Framebuffer,
) {
    assert!(
        before.pixels() != reference.pixels(),
        "setup: {what}: the result must change what is painted"
    );
    let differing = shown
        .pixels()
        .chunks_exact(4)
        .zip(reference.pixels().chunks_exact(4))
        .filter(|(a, b)| a != b)
        .count();
    assert_eq!(
        differing, 0,
        "{what}: the frame must show the result, but {differing} pixels still \
         differ from a fresh app showing it"
    );
}

/// A labelled column: the label under test, bare or in a [`CachedPanel`].
fn label_app(text: &str, in_panel: bool) -> App {
    let label = Label::new(text, font())
        .with_font_size(16.0)
        .with_color(Color::black())
        .with_has_backbuffer(true);
    App::new(hosted(Box::new(label), in_panel))
}

fn find_label(w: &mut dyn Widget) -> Option<&mut Label> {
    if w.as_any().is_some_and(|a| a.is::<Label>()) {
        return w.as_any_mut().and_then(|a| a.downcast_mut::<Label>());
    }
    w.children_mut()
        .iter_mut()
        .find_map(|c| find_label(c.as_mut()))
}

thread_local! {
    /// The app a worker's idle action reaches on the UI thread.
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

/// [`painted_frame`] for the app in [`APP`]: the drain runs with it unborrowed,
/// so an idle action can reach it.
fn painted_app_frame() -> (Framebuffer, u64) {
    let before = paint_work();
    ui_thread::invoke_pending_actions();
    let fb = APP.with(|slot| {
        let mut slot = slot.borrow_mut();
        let app = slot.as_mut().expect("app installed");
        app.layout(VIEWPORT);
        let mut fb = Framebuffer::new(VIEWPORT.width as u32, VIEWPORT.height as u32);
        {
            let mut ctx = GfxCtx::new(&mut fb);
            app.paint(&mut ctx);
        }
        fb
    });
    (fb, paint_work().since(before).backbuffer_rasters)
}

#[test]
fn a_label_text_a_worker_sets_through_run_on_idle_shows_the_next_frame() {
    let _ui = OwnUiThread::new();
    for in_panel in [false, true] {
        let what = if in_panel {
            "a label in a cached panel"
        } else {
            "a bare label"
        };
        let (reference, _) = painted_frame(&mut label_app("result arrived", in_panel));
        APP.with(|slot| *slot.borrow_mut() = Some(label_app("waiting", in_panel)));
        painted_app_frame();
        let (before, rasters) = painted_app_frame();
        assert_eq!(rasters, 0, "setup: {what} paints from its caches");

        // The worker hands the result to the UI thread, which sets the text.
        let queue = ui_thread::current_queue();
        std::thread::spawn(move || {
            queue.run_on_idle(|| {
                APP.with(|slot| {
                    let mut slot = slot.borrow_mut();
                    let app = slot.as_mut().expect("app installed");
                    find_label(app.root_mut())
                        .expect("the label")
                        .set_text("result arrived");
                })
            })
        })
        .join()
        .expect("worker posts");
        let (shown, _) = painted_app_frame();
        APP.with(|slot| *slot.borrow_mut() = None);
        assert_shows_result(what, &before, &shown, &reference);
    }
}

#[test]
fn a_text_field_hint_cell_changed_after_layout_shows_that_frame() {
    let _ui = OwnUiThread::new();
    for in_panel in [false, true] {
        let what = if in_panel {
            "a text field's hint cell in a cached panel"
        } else {
            "a bare text field's hint cell"
        };
        let field_app = |hint: &Rc<RefCell<String>>| {
            let field = TextField::new(font()).with_placeholder_cell(Rc::clone(hint));
            App::new(hosted(Box::new(field), in_panel))
        };
        let new_hint = Rc::new(RefCell::new("a much longer new hint".to_string()));
        let (reference, _) = painted_frame(&mut field_app(&new_hint));

        let hint = Rc::new(RefCell::new("hint".to_string()));
        let mut app = field_app(&hint);
        warm(&mut app, what);
        let (before, _) = painted_frame(&mut app);

        // The hint cell changes after this frame's layout, and a worker wakes
        // the thread before its paint: the field's signature is stale, so the
        // async-state walk re-rasters it, and its paint reads the cell.
        ui_thread::invoke_pending_actions();
        app.layout(VIEWPORT);
        *hint.borrow_mut() = "a much longer new hint".to_string();
        signal_from_worker();
        let mut shown = Framebuffer::new(VIEWPORT.width as u32, VIEWPORT.height as u32);
        {
            let mut ctx = GfxCtx::new(&mut shown);
            app.paint(&mut ctx);
        }
        assert_shows_result(what, &before, &shown, &reference);
    }
}

#[test]
fn a_text_area_highlighter_reading_shared_state_repaints_after_a_wake() {
    let _ui = OwnUiThread::new();
    const BEFORE: Color = Color::rgb(0.8, 0.1, 0.1);
    const AFTER: Color = Color::rgb(0.1, 0.6, 0.2);
    let area_app = |shown: &Rc<Cell<Color>>| {
        let shown = Rc::clone(shown);
        let area = TextArea::new(font())
            .with_text("highlighted text")
            .with_highlighter(move |line| vec![(0, line.len(), shown.get())]);
        App::new(Box::new(area))
    };
    let (reference, _) = painted_frame(&mut area_app(&Rc::new(Cell::new(AFTER))));

    let shown = Rc::new(Cell::new(BEFORE));
    let mut app = area_app(&shown);
    painted_frame(&mut app);
    let (before, _) = painted_frame(&mut app);
    shown.set(AFTER);
    signal_from_worker();
    let (after, _) = painted_frame(&mut app);
    assert_shows_result("a text area's highlighter", &before, &after, &reference);
}
