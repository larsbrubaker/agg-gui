//! WASM demo crate for agg-gui — Phase 8 (WebGL2).
//!
//! The widget tree is rendered via `GlGfxCtx` (tess2 tessellation → WebGL2
//! draw calls) directly to the canvas.  A rotating 3D cube is drawn on top
//! each frame by `CubeGlRenderer`.
//!
//! WASM exports:
//! - `render(width, height)` — full-frame render (void; GL writes to canvas)
//! - `on_mouse_move/down/up/wheel/leave` — mouse events
//! - `on_key_down` — keyboard events

mod gl_gfx_ctx;
mod gl_resources;

use gl_gfx_ctx::GlGfxCtx;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use agg_gui::{
    App, Button, Checkbox, Color, CompOp, Container, DrawCtx, Event, EventResult, FlexColumn, FlexRow,
    Font, InspectorNode, InspectorPanel, Key, Label, Modifiers, MouseButton, NodeIcon, ProgressBar,
    RadioGroup, Rect, ScrollView, Separator, Size, SizedBox, Slider, Spacer, Splitter,
    Stack, TabView, TextField, TreeView, Widget, Window,
};
use gl_resources::{GlCubeWidget, GlState, CUBE_SCREEN_RECT};

// Embed the font at compile time.
const FONT_BYTES: &[u8] = include_bytes!("../../demo/assets/CascadiaCode.ttf");

fn make_font() -> Arc<Font> {
    Arc::new(Font::from_slice(FONT_BYTES).expect("embedded font is valid"))
}

// ---------------------------------------------------------------------------
// Single persistent App — outer TabView with all four tabs
// ---------------------------------------------------------------------------

thread_local! {
    static DEMO_APP:  RefCell<Option<App>>       = RefCell::new(None);
    static GL_STATE:  RefCell<Option<GlState>>   = RefCell::new(None);
    /// Persistent GL 2-D drawing context — created once, reset each frame.
    static GL_CTX:    RefCell<Option<GlGfxCtx>>  = RefCell::new(None);

    // Inspector shared state — set once by build_demo_ui, read each frame.
    static SHOW_INSPECTOR:  RefCell<Option<Rc<Cell<bool>>>>                     = RefCell::new(None);
    static INSPECTOR_NODES: RefCell<Option<Rc<RefCell<Vec<InspectorNode>>>>>    = RefCell::new(None);
}

/// Initialise panic hook so Rust panics appear in the browser console.
#[wasm_bindgen(start)]
pub fn wasm_start() {
    console_error_panic_hook::set_once();
}

fn ensure_demo_app() {
    DEMO_APP.with(|cell| {
        if cell.borrow().is_none() {
            *cell.borrow_mut() = Some(build_demo_ui(make_font()));
        }
    });
}

fn ensure_gl_state() {
    GL_STATE.with(|cell| {
        if cell.borrow().is_none() {
            let gl = init_webgl2();
            *cell.borrow_mut() = Some(unsafe { GlState::new(gl) });
        }
    });
}

/// Ensure the persistent `GlGfxCtx` is created (uses `GL_STATE`'s context).
fn ensure_gl_ctx(width: f32, height: f32) {
    // Get the Rc<glow::Context> from GL_STATE without keeping GL_STATE borrowed.
    let gl_rc = GL_STATE.with(|cell| {
        cell.borrow().as_ref().map(|s| s.gl_rc())
    });
    let gl_rc = gl_rc.expect("GL_STATE must be initialised before ensure_gl_ctx");

    GL_CTX.with(|cell| {
        let mut borrow = cell.borrow_mut();
        if borrow.is_none() {
            *borrow = Some(unsafe { GlGfxCtx::new(gl_rc, width, height) });
        }
    });
}

fn init_webgl2() -> glow::Context {
    let document = web_sys::window()
        .expect("no global window")
        .document()
        .expect("no document");
    let canvas = document
        .get_element_by_id("canvas")
        .expect("canvas element not found")
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .expect("element is not a canvas");
    let webgl2 = canvas
        .get_context("webgl2")
        .expect("get_context failed")
        .expect("webgl2 context unavailable")
        .dyn_into::<web_sys::WebGl2RenderingContext>()
        .expect("not a WebGl2RenderingContext");
    glow::Context::from_webgl2_context(webgl2)
}

fn build_demo_ui(font: Arc<Font>) -> App {
    // Shared state: inspector visibility + node snapshot
    let show_inspector  = Rc::new(Cell::new(false));
    let inspector_nodes = Rc::new(RefCell::new(Vec::<InspectorNode>::new()));

    // Toggle callback used by the tab-bar action button
    let show_clone = Rc::clone(&show_inspector);
    let tab_view = TabView::new(Arc::clone(&font))
        .with_tab_bar_height(40.0)
        .with_font_size(13.0)
        .add_tab("Basics",  Box::new(build_basics_content(Arc::clone(&font))))
        .add_tab("Widgets", Box::new(build_widgets_content(Arc::clone(&font))))
        .add_tab("Text",    Box::new(TextDemoWidget::new(Arc::clone(&font))))
        .add_tab("Layout",  Box::new(build_layout_content(Arc::clone(&font))))
        .add_tab("Tree",    Box::new(build_tree_content(Arc::clone(&font))))
        .with_action_button("Inspector", move || {
            show_clone.set(!show_clone.get());
        });

    let window = build_demo_window(Arc::clone(&font));

    let inspector = InspectorPanel::new(
        Arc::clone(&font),
        Rc::clone(&show_inspector),
        Rc::clone(&inspector_nodes),
    );

    // Store shared state in thread_locals so render() can update the snapshot
    SHOW_INSPECTOR.with(|c| *c.borrow_mut() = Some(Rc::clone(&show_inspector)));
    INSPECTOR_NODES.with(|c| *c.borrow_mut() = Some(Rc::clone(&inspector_nodes)));

    let root = Stack::new()
        .add(Box::new(tab_view))
        .add(Box::new(window))
        .add(Box::new(inspector));   // on top — paints over everything when visible

    App::new(Box::new(root))
}

fn build_demo_window(font: Arc<Font>) -> Window {
    let mut content = FlexColumn::new()
        .with_gap(8.0)
        .with_padding(10.0)
        .with_background(Color::rgb(0.08, 0.08, 0.12));

    content.push(Box::new(Label::new("WebGL2 — rotating cube", Arc::clone(&font))
        .with_font_size(11.0)
        .with_color(Color::rgba(1.0, 1.0, 1.0, 0.55))), 0.0);

    // The GlCubeWidget fills the remaining space; the GL renderer draws the
    // actual cube on top of the AGG framebuffer blit each frame.
    content.push(Box::new(GlCubeWidget::new()), 1.0);

    Window::new("3D Demo", font, Box::new(content))
        .with_bounds(Rect::new(60.0, 160.0, 300.0, 260.0))
}

// ---------------------------------------------------------------------------
// Tab content builders
// ---------------------------------------------------------------------------

fn build_basics_content(font: Arc<Font>) -> Container {
    let font2 = Arc::clone(&font);
    let font3 = Arc::clone(&font);
    let font4 = Arc::clone(&font);
    let font5 = Arc::clone(&font);

    let mut root = Container::new()
        .with_background(Color::rgb(0.94, 0.94, 0.96))
        .with_padding(24.0);

    root.children_mut().push(Box::new(
        Button::new("Primary Action", Arc::clone(&font))
            .with_font_size(14.0)
            .on_click(|| {}),
    ));
    root.children_mut().push(Box::new(
        Button::new("Secondary", Arc::clone(&font2))
            .with_font_size(14.0)
            .with_theme(agg_gui::widgets::button::ButtonTheme {
                background:         Color::rgba(0.22, 0.45, 0.88, 0.12),
                background_hovered: Color::rgba(0.22, 0.45, 0.88, 0.22),
                background_pressed: Color::rgba(0.22, 0.45, 0.88, 0.35),
                label_color:        Color::rgb(0.22, 0.45, 0.88),
                border_radius:      6.0,
                focus_ring_color:   Color::rgba(0.22, 0.45, 0.88, 0.55),
                focus_ring_width:   2.5,
            }),
    ));
    root.children_mut().push(Box::new(
        Button::new("Destructive", Arc::clone(&font3))
            .with_font_size(14.0)
            .with_theme(agg_gui::widgets::button::ButtonTheme {
                background:         Color::rgb(0.88, 0.25, 0.18),
                background_hovered: Color::rgb(0.95, 0.32, 0.24),
                background_pressed: Color::rgb(0.72, 0.18, 0.12),
                label_color:        Color::white(),
                border_radius:      6.0,
                focus_ring_color:   Color::rgba(0.88, 0.25, 0.18, 0.55),
                focus_ring_width:   2.5,
            }),
    ));
    root.children_mut().push(Box::new(
        TextField::new(Arc::clone(&font4))
            .with_font_size(14.0)
            .with_placeholder("Type something…"),
    ));
    root.children_mut().push(Box::new(
        TextField::new(Arc::clone(&font5))
            .with_font_size(14.0)
            .with_text("editable text")
            .with_placeholder("Another field"),
    ));
    root
}

fn build_widgets_content(font: Arc<Font>) -> ScrollView {
    // Shared state for the demo controls
    let slider_val  = Rc::new(Cell::new(0.42_f64));
    let cb1         = Rc::new(Cell::new(true));
    let cb2         = Rc::new(Cell::new(false));
    let cb3         = Rc::new(Cell::new(true));
    let radio_sel   = Rc::new(Cell::new(0_usize));

    // Progress bar driven by slider — we use a thread_local RefCell trick:
    // wrap it in a widget-level closure. Simpler: we use a ProgressBar leaf
    // and clone the Rc into its on_change.
    // Actually we need the ProgressBar to read slider_val each frame.
    // We'll use a custom wrapper widget for the live update.

    let mut col = FlexColumn::new()
        .with_gap(20.0)
        .with_padding(24.0)
        .with_background(Color::rgb(0.94, 0.94, 0.96));

    // --- Section: Buttons ---
    col.push(Box::new(Label::new("Buttons", Arc::clone(&font))
        .with_font_size(16.0)
        .with_color(Color::rgb(0.1, 0.1, 0.12))), 0.0);

    {
        let row = FlexRow::new()
            .with_gap(8.0)
            .add(Box::new(SizedBox::new().with_width(120.0).with_height(34.0).with_child(Box::new(
                Button::new("Primary", Arc::clone(&font)).with_font_size(13.0).on_click(|| {})
            ))))
            .add(Box::new(SizedBox::new().with_width(120.0).with_height(34.0).with_child(Box::new(
                Button::new("Secondary", Arc::clone(&font))
                    .with_font_size(13.0)
                    .with_theme(agg_gui::widgets::button::ButtonTheme {
                        background:         Color::rgba(0.22, 0.45, 0.88, 0.12),
                        background_hovered: Color::rgba(0.22, 0.45, 0.88, 0.22),
                        background_pressed: Color::rgba(0.22, 0.45, 0.88, 0.35),
                        label_color:        Color::rgb(0.22, 0.45, 0.88),
                        border_radius:      6.0,
                        focus_ring_color:   Color::rgba(0.22, 0.45, 0.88, 0.55),
                        focus_ring_width:   2.5,
                    })
                    .on_click(|| {})
            ))))
            .add(Box::new(SizedBox::new().with_width(120.0).with_height(34.0).with_child(Box::new(
                Button::new("Danger", Arc::clone(&font))
                    .with_font_size(13.0)
                    .with_theme(agg_gui::widgets::button::ButtonTheme {
                        background:         Color::rgb(0.88, 0.25, 0.18),
                        background_hovered: Color::rgb(0.95, 0.32, 0.24),
                        background_pressed: Color::rgb(0.72, 0.18, 0.12),
                        label_color:        Color::white(),
                        border_radius:      6.0,
                        focus_ring_color:   Color::rgba(0.88, 0.25, 0.18, 0.55),
                        focus_ring_width:   2.5,
                    })
                    .on_click(|| {})
            ))));
        col.push(Box::new(row), 0.0);
    }

    col.push(Box::new(Separator::horizontal()), 0.0);

    // --- Section: Checkboxes ---
    col.push(Box::new(Label::new("Checkboxes", Arc::clone(&font))
        .with_font_size(16.0)
        .with_color(Color::rgb(0.1, 0.1, 0.12))), 0.0);

    {
        let v1 = Rc::clone(&cb1);
        col.push(Box::new(Checkbox::new("Enable notifications", Arc::clone(&font), cb1.get())
            .on_change(move |v| { v1.set(v); })), 0.0);
        let v2 = Rc::clone(&cb2);
        col.push(Box::new(Checkbox::new("Dark mode", Arc::clone(&font), cb2.get())
            .on_change(move |v| { v2.set(v); })), 0.0);
        let v3 = Rc::clone(&cb3);
        col.push(Box::new(Checkbox::new("Send analytics", Arc::clone(&font), cb3.get())
            .on_change(move |v| { v3.set(v); })), 0.0);
    }

    col.push(Box::new(Separator::horizontal()), 0.0);

    // --- Section: Slider ---
    col.push(Box::new(Label::new("Slider", Arc::clone(&font))
        .with_font_size(16.0)
        .with_color(Color::rgb(0.1, 0.1, 0.12))), 0.0);

    {
        let sv = Rc::clone(&slider_val);
        col.push(Box::new(Slider::new(slider_val.get(), 0.0, 1.0, Arc::clone(&font))
            .with_step(0.01)
            .on_change(move |v| { sv.set(v); })), 0.0);
    }

    col.push(Box::new(Separator::horizontal()), 0.0);

    // --- Section: Radio ---
    col.push(Box::new(Label::new("Radio Group", Arc::clone(&font))
        .with_font_size(16.0)
        .with_color(Color::rgb(0.1, 0.1, 0.12))), 0.0);

    {
        let rs = Rc::clone(&radio_sel);
        col.push(Box::new(RadioGroup::new(
            vec!["Option A", "Option B", "Option C"],
            radio_sel.get(),
            Arc::clone(&font),
        ).on_change(move |i| { rs.set(i); })), 0.0);
    }

    col.push(Box::new(Separator::horizontal()), 0.0);

    // --- Section: Progress Bar (static value for WASM; driven by slider_val snapshot) ---
    col.push(Box::new(Label::new("Progress Bar", Arc::clone(&font))
        .with_font_size(16.0)
        .with_color(Color::rgb(0.1, 0.1, 0.12))), 0.0);

    col.push(Box::new(ProgressBar::new(slider_val.get(), Arc::clone(&font))), 0.0);

    col.push(Box::new(Separator::horizontal()), 0.0);

    // --- Section: Text Input ---
    col.push(Box::new(Label::new("Text Input", Arc::clone(&font))
        .with_font_size(16.0)
        .with_color(Color::rgb(0.1, 0.1, 0.12))), 0.0);

    col.push(Box::new(
        TextField::new(Arc::clone(&font))
            .with_font_size(14.0)
            .with_placeholder("Type something here…")
    ), 0.0);

    // Bottom spacer
    col.push(Box::new(SizedBox::fixed(0.0, 24.0)), 0.0);

    ScrollView::new(Box::new(col))
}

fn build_layout_content(font: Arc<Font>) -> TabView {
    TabView::new(Arc::clone(&font))
        .with_tab_bar_height(36.0)
        .with_font_size(13.0)
        .add_tab("Flex",   Box::new(build_flex_demo(Arc::clone(&font))))
        .add_tab("Scroll", Box::new(build_scroll_demo(Arc::clone(&font))))
        .add_tab("Split",  Box::new(build_split_demo(Arc::clone(&font))))
}

fn build_tree_content(font: Arc<Font>) -> TreeView {
    let mut tv = TreeView::new(Arc::clone(&font))
        .with_row_height(26.0)
        .with_font_size(13.0)
        .with_indent_width(18.0);

    let alpha = tv.add_root("Project Alpha", NodeIcon::Package);
    tv.expand(alpha);
    let src = tv.add_child(alpha, "src", NodeIcon::Folder);
    tv.expand(src);
    tv.add_child(src, "main.rs", NodeIcon::File);
    tv.add_child(src, "lib.rs", NodeIcon::File);
    let widgets_dir = tv.add_child(src, "widgets", NodeIcon::Folder);
    tv.expand(widgets_dir);
    tv.add_child(widgets_dir, "button.rs", NodeIcon::File);
    tv.add_child(widgets_dir, "scroll_view.rs", NodeIcon::File);
    tv.add_child(widgets_dir, "tree_view.rs", NodeIcon::File);
    let tests = tv.add_child(alpha, "tests", NodeIcon::Folder);
    tv.expand(tests);
    tv.add_child(tests, "integration.rs", NodeIcon::File);
    tv.add_child(tests, "unit.rs", NodeIcon::File);
    tv.add_child(alpha, "Cargo.toml", NodeIcon::File);
    tv.add_child(alpha, "README.md", NodeIcon::File);

    let beta = tv.add_root("Project Beta", NodeIcon::Package);
    let assets = tv.add_child(beta, "assets", NodeIcon::Folder);
    tv.add_child(assets, "logo.svg", NodeIcon::File);
    tv.add_child(assets, "icons.png", NodeIcon::File);
    let bsrc = tv.add_child(beta, "src", NodeIcon::Folder);
    tv.add_child(bsrc, "app.rs", NodeIcon::File);
    tv.add_child(bsrc, "config.rs", NodeIcon::File);
    tv.add_child(beta, "Cargo.toml", NodeIcon::File);

    let gamma = tv.add_root("Project Gamma", NodeIcon::Package);
    let gsrc = tv.add_child(gamma, "src", NodeIcon::Folder);
    tv.add_child(gsrc, "main.rs", NodeIcon::File);
    tv.add_child(gsrc, "render.rs", NodeIcon::File);
    tv.add_child(gsrc, "scene.rs", NodeIcon::File);
    let shaders = tv.add_child(gsrc, "shaders", NodeIcon::Folder);
    tv.add_child(shaders, "vert.glsl", NodeIcon::File);
    tv.add_child(shaders, "frag.glsl", NodeIcon::File);
    tv.add_child(gamma, "Cargo.toml", NodeIcon::File);

    tv
}

// ---------------------------------------------------------------------------
// TextDemoWidget — stateless widget that draws the Phase 3 text showcase
// ---------------------------------------------------------------------------

struct TextDemoWidget {
    bounds: Rect,
    font: Arc<Font>,
    children: Vec<Box<dyn Widget>>,
}

impl TextDemoWidget {
    fn new(font: Arc<Font>) -> Self {
        Self { bounds: Rect::default(), font, children: Vec::new() }
    }
}

impl Widget for TextDemoWidget {
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
        available
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let w = self.bounds.width;
        let h = self.bounds.height;
        draw_text_tab(ctx, w, h, &Arc::clone(&self.font));
    }

    fn on_event(&mut self, _event: &Event) -> EventResult { EventResult::Ignored }
}

// ---------------------------------------------------------------------------
// Text tab drawing helpers (Phase 3)
// ---------------------------------------------------------------------------

fn draw_text_tab(ctx: &mut dyn DrawCtx, w: f64, h: f64, font: &Arc<Font>) {
    ctx.set_font(Arc::clone(font));
    // Fill only the widget's local area, not the entire framebuffer.
    // ctx.clear() would erase the tab bar already painted by the parent TabView.
    ctx.set_fill_color(Color::rgb(0.94, 0.94, 0.96));
    ctx.begin_path();
    ctx.rect(0.0, 0.0, w, h);
    ctx.fill();

    let pad = (w.min(h) * 0.03).max(10.0);
    let gap = pad * 0.6;
    let col_w = (w - pad * 2.0 - gap) / 2.0;
    let row_h = (h - pad * 2.0 - gap) / 2.0;

    let panels = [
        (pad,               pad + row_h + gap, col_w, row_h),
        (pad + col_w + gap, pad + row_h + gap, col_w, row_h),
        (pad,               pad,               col_w, row_h),
        (pad + col_w + gap, pad,               col_w, row_h),
    ];

    for &(px, py, pw, ph) in &panels {
        draw_card(ctx, px, py, pw, ph);
    }

    { let (px, py, pw, ph) = panels[0]; draw_sizes_panel(ctx, px, py, pw, ph); }
    { let (px, py, pw, ph) = panels[1]; draw_measure_panel(ctx, px, py, pw, ph, font); }
    { let (px, py, pw, ph) = panels[2]; draw_multiline_panel(ctx, px, py, pw, ph, font); }
    { let (px, py, pw, ph) = panels[3]; draw_buttons_panel(ctx, px, py, pw, ph); }

    let lsize = (w * 0.012).clamp(9.0, 13.0);
    ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, 0.35));
    ctx.fill_text_gsv("agg-gui  Phase 3 — Text", pad, pad * 0.4, lsize);
}

fn draw_card(ctx: &mut dyn DrawCtx, x: f64, y: f64, w: f64, h: f64) {
    ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, 0.08));
    ctx.set_blend_mode(CompOp::Multiply);
    ctx.begin_path();
    ctx.rounded_rect(x + 2.0, y - 2.0, w, h, 10.0);
    ctx.fill();
    ctx.set_blend_mode(CompOp::SrcOver);
    ctx.set_fill_color(Color::rgb(1.0, 1.0, 1.0));
    ctx.begin_path();
    ctx.rounded_rect(x, y, w, h, 10.0);
    ctx.fill();
}

fn panel_title_gsv(ctx: &mut dyn DrawCtx, px: f64, py: f64, pw: f64, ph: f64, title: &str) {
    let size = (pw * 0.055).clamp(10.0, 16.0);
    ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, 0.55));
    ctx.fill_text_gsv(title, px + pw * 0.05, py + ph * 0.86, size);
}

fn draw_sizes_panel(ctx: &mut dyn DrawCtx, px: f64, py: f64, pw: f64, ph: f64) {
    panel_title_gsv(ctx, px, py, pw, ph, "Font Sizes");
    let margin = pw * 0.06;
    let sizes: &[(f64, &str)] = &[
        (10.0, "Caption — 10px  The quick brown fox"),
        (13.0, "Body — 13px  The quick brown fox"),
        (18.0, "Subhead — 18px  The quick"),
        (24.0, "Heading — 24px  agg-gui"),
        (34.0, "Display — 34px  Aa"),
    ];
    let mut y = py + ph * 0.82;
    let baseline_adv = ph * 0.155;
    ctx.set_fill_color(Color::rgba(0.05, 0.05, 0.1, 0.85));
    for &(size, label) in sizes.iter() {
        ctx.set_font_size(size);
        ctx.fill_text(label, px + margin, y);
        y -= baseline_adv;
    }
}

fn draw_measure_panel(ctx: &mut dyn DrawCtx, px: f64, py: f64, pw: f64, ph: f64, font: &Arc<Font>) {
    panel_title_gsv(ctx, px, py, pw, ph, "Measure Text");
    let margin = pw * 0.06;
    let font_size = (pw * 0.08).clamp(14.0, 26.0);
    ctx.set_font_size(font_size);
    let samples = ["Hello", "World!", "agg-gui", "Rust"];
    let col_w = (pw - margin * 2.0) / samples.len() as f64;
    let base_y = py + ph * 0.5;
    for (i, &word) in samples.iter().enumerate() {
        let x = px + margin + col_w * i as f64;
        let m = ctx.measure_text(word).unwrap_or_default();
        ctx.set_stroke_color(Color::rgba(0.6, 0.6, 0.65, 0.5));
        ctx.set_line_width(1.0);
        ctx.begin_path(); ctx.move_to(x, base_y - 2.0); ctx.line_to(x + m.width, base_y - 2.0); ctx.stroke();
        ctx.set_stroke_color(Color::rgba(0.2, 0.5, 0.9, 0.35));
        ctx.begin_path(); ctx.move_to(x, base_y + m.ascent); ctx.line_to(x + m.width, base_y + m.ascent); ctx.stroke();
        ctx.set_stroke_color(Color::rgba(0.9, 0.3, 0.3, 0.35));
        ctx.begin_path(); ctx.move_to(x, base_y - m.descent); ctx.line_to(x + m.width, base_y - m.descent); ctx.stroke();
        ctx.set_fill_color(Color::rgba(0.2, 0.5, 0.9, 0.07));
        ctx.begin_path(); ctx.rect(x, base_y - m.descent, m.width, m.ascent + m.descent); ctx.fill();
        ctx.set_fill_color(Color::rgba(0.05, 0.05, 0.1, 0.88));
        ctx.fill_text(word, x, base_y);
    }
    let lsize = (pw * 0.032).clamp(7.0, 10.0);
    let ly = py + ph * 0.22;
    let lx = px + margin;
    ctx.set_font_size(lsize);
    ctx.set_fill_color(Color::rgba(0.2, 0.5, 0.9, 0.7)); ctx.fill_text("— ascent", lx, ly);
    ctx.set_fill_color(Color::rgba(0.9, 0.3, 0.3, 0.7)); ctx.fill_text("— descent", lx, ly - lsize * 1.5);
    ctx.set_fill_color(Color::rgba(0.5, 0.5, 0.55, 0.7)); ctx.fill_text("— baseline", lx, ly - lsize * 3.0);
    let _ = font;
}

fn draw_multiline_panel(ctx: &mut dyn DrawCtx, px: f64, py: f64, pw: f64, ph: f64, font: &Arc<Font>) {
    panel_title_gsv(ctx, px, py, pw, ph, "Multi-line");
    let margin = pw * 0.06;
    let font_size = (pw * 0.055).clamp(11.0, 16.0);
    ctx.set_font_size(font_size);
    ctx.set_fill_color(Color::rgba(0.05, 0.05, 0.1, 0.85));
    let line_h = font.line_height_px(font_size) * 1.25;
    let x = px + margin;
    let lines = [
        "agg-gui renders text by",
        "shaping with rustybuzz,",
        "extracting outlines via",
        "ttf-parser, and feeding",
        "Bezier curves into AGG.",
        "",
        "No glyph atlas. Kerning",
        "and hinting are preserved.",
    ];
    let mut y = py + ph * 0.82;
    for line in lines.iter() {
        if !line.is_empty() { ctx.fill_text(line, x, y); }
        y -= line_h;
    }
}

fn draw_buttons_panel(ctx: &mut dyn DrawCtx, px: f64, py: f64, pw: f64, ph: f64) {
    panel_title_gsv(ctx, px, py, pw, ph, "Text + Graphics");
    let margin = pw * 0.07;
    let btn_h = ph * 0.16;
    let btn_r = btn_h * 0.35;
    let bx = px + margin;
    let bw = pw - margin * 2.0;
    let buttons: &[(&str, Color, Color)] = &[
        ("Primary Action",  Color::rgb(0.22, 0.45, 0.88), Color::white()),
        ("Secondary",       Color::rgba(0.22, 0.45, 0.88, 0.12), Color::rgb(0.22, 0.45, 0.88)),
        ("Destructive",     Color::rgb(0.88, 0.25, 0.18), Color::white()),
        ("Disabled",        Color::rgba(0.0, 0.0, 0.0, 0.08), Color::rgba(0.0,0.0,0.0,0.3)),
    ];
    let spacing = (ph * 0.74) / buttons.len() as f64;
    let font_size = (btn_h * 0.38).clamp(10.0, 16.0);
    ctx.set_font_size(font_size);
    for (i, &(label, bg, fg)) in buttons.iter().enumerate() {
        let by = py + ph * 0.78 - i as f64 * spacing;
        ctx.set_fill_color(bg);
        ctx.set_blend_mode(CompOp::SrcOver);
        ctx.begin_path(); ctx.rounded_rect(bx, by - btn_h * 0.5, bw, btn_h, btn_r); ctx.fill();
        if let Some(m) = ctx.measure_text(label) {
            let tx = bx + (bw - m.width) * 0.5;
            let ty = by - m.ascent * 0.45 + m.descent * 0.45;
            ctx.set_fill_color(fg);
            ctx.fill_text(label, tx, ty);
        }
    }
}

// ---------------------------------------------------------------------------
// Layout tab sub-demos
// ---------------------------------------------------------------------------

fn build_flex_demo(font: Arc<Font>) -> FlexColumn {
    let fa = Arc::clone(&font);
    let fb = Arc::clone(&font);
    let fc = Arc::clone(&font);
    let fd = Arc::clone(&font);
    let fe = Arc::clone(&font);

    let row = FlexRow::new()
        .with_gap(8.0)
        .add_flex(Box::new(Button::new("One",   fa).with_font_size(13.0).on_click(|| {})), 1.0)
        .add_flex(Box::new(Button::new("Two",   fb).with_font_size(13.0).on_click(|| {})), 1.0)
        .add_flex(Box::new(Button::new("Three", fc).with_font_size(13.0).on_click(|| {})), 1.0);

    FlexColumn::new()
        .with_gap(12.0)
        .with_padding(20.0)
        .with_background(Color::rgb(0.94, 0.94, 0.96))
        .add(Box::new(SizedBox::new().with_height(36.0).with_child(Box::new(row))))
        .add(Box::new(SizedBox::new().with_height(36.0).with_child(Box::new(
            TextField::new(fd).with_font_size(14.0).with_placeholder("Search…"),
        ))))
        .add_flex(Box::new(Spacer::new()), 1.0)
        .add(Box::new(SizedBox::new().with_height(36.0).with_child(Box::new(
            Button::new("Confirm", fe).with_font_size(14.0).on_click(|| {}),
        ))))
}

fn build_scroll_demo(font: Arc<Font>) -> ScrollView {
    let mut col = FlexColumn::new()
        .with_gap(8.0)
        .with_padding(16.0)
        .with_background(Color::rgb(0.94, 0.94, 0.96));

    for i in 0..24u32 {
        let label = format!("Item {:02}", i + 1);
        col.push(
            Box::new(SizedBox::new().with_height(40.0).with_child(Box::new(
                Button::new(label, Arc::clone(&font)).with_font_size(13.0).on_click(|| {}),
            ))),
            0.0,
        );
    }

    ScrollView::new(Box::new(col))
}

fn build_split_demo(font: Arc<Font>) -> Splitter {
    let fa = Arc::clone(&font);
    let fb = Arc::clone(&font);
    let fc = Arc::clone(&font);
    let fd = Arc::clone(&font);

    let left = FlexColumn::new()
        .with_gap(8.0)
        .with_padding(16.0)
        .with_background(Color::rgb(0.96, 0.96, 0.99))
        .add(Box::new(SizedBox::new().with_height(36.0).with_child(Box::new(
            Button::new("Left A", fa).with_font_size(13.0).on_click(|| {}),
        ))))
        .add(Box::new(SizedBox::new().with_height(36.0).with_child(Box::new(
            Button::new("Left B", fb).with_font_size(13.0).on_click(|| {}),
        ))))
        .add_flex(Box::new(Spacer::new()), 1.0);

    let right = FlexColumn::new()
        .with_gap(8.0)
        .with_padding(16.0)
        .with_background(Color::rgb(0.99, 0.96, 0.96))
        .add(Box::new(SizedBox::new().with_height(36.0).with_child(Box::new(
            TextField::new(fc).with_font_size(13.0).with_placeholder("Right field…"),
        ))))
        .add(Box::new(SizedBox::new().with_height(36.0).with_child(Box::new(
            Button::new("Action", fd).with_font_size(13.0).on_click(|| {}),
        ))))
        .add_flex(Box::new(Spacer::new()), 1.0);

    Splitter::new(Box::new(left), Box::new(right)).with_ratio(0.4)
}

// ---------------------------------------------------------------------------
// Key parsing — unified for all tabs
// ---------------------------------------------------------------------------

fn parse_js_key(key: &str) -> Option<Key> {
    Some(match key {
        "Backspace"  => Key::Backspace,
        "Delete"     => Key::Delete,
        "ArrowLeft"  => Key::ArrowLeft,
        "ArrowRight" => Key::ArrowRight,
        "ArrowUp"    => Key::ArrowUp,
        "ArrowDown"  => Key::ArrowDown,
        "Home"       => Key::Home,
        "End"        => Key::End,
        "Tab"        => Key::Tab,
        "Enter"      => Key::Enter,
        "Escape"     => Key::Escape,
        " "          => Key::Char(' '),
        s if s.chars().count() == 1 => Key::Char(s.chars().next()?),
        s => Key::Other(s.to_string()),
    })
}

// ---------------------------------------------------------------------------
// WASM render export
// ---------------------------------------------------------------------------

/// Full-frame render.  Direct GL path: the widget tree is painted via
/// `GlGfxCtx` (tess2 tessellation → WebGL2 draw calls).  No off-screen
/// framebuffer is used.  The rotating 3D cube is drawn last, on top.
#[wasm_bindgen]
pub fn render(width: u32, height: u32) {
    ensure_demo_app();
    ensure_gl_state();
    ensure_gl_ctx(width as f32, height as f32);

    // ── 1. GL clear ─────────────────────────────────────────────────────────
    GL_STATE.with(|gl_cell| {
        if let Some(state) = gl_cell.borrow().as_ref() {
            let gl = state.gl_rc();
            unsafe {
                use glow::HasContext;
                gl.viewport(0, 0, width as i32, height as i32);
                gl.clear_color(0.1, 0.1, 0.1, 1.0);
                gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
                gl.enable(glow::BLEND);
                gl.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
                gl.disable(glow::DEPTH_TEST);
                gl.disable(glow::SCISSOR_TEST);
            }
        }
    });

    // ── 2. Sync inspector nodes snapshot (before paint) ─────────────────────
    let show_inspector = SHOW_INSPECTOR.with(|c| c.borrow().as_ref().map(|r| r.get()).unwrap_or(false));
    if show_inspector {
        let nodes = DEMO_APP.with(|cell| {
            cell.borrow().as_ref().map(|app| app.collect_inspector_nodes())
        });
        if let Some(nodes) = nodes {
            INSPECTOR_NODES.with(|c| {
                if let Some(ref rc) = *c.borrow() {
                    *rc.borrow_mut() = nodes;
                }
            });
        }
    }

    // ── 3. Reset GL_CTX for this frame then paint ────────────────────────────
    GL_CTX.with(|ctx_cell| {
        let mut ctx_borrow = ctx_cell.borrow_mut();
        if let Some(gl_ctx) = ctx_borrow.as_mut() {
            gl_ctx.reset(width as f32, height as f32);

            DEMO_APP.with(|app_cell| {
                let mut app_borrow = app_cell.borrow_mut();
                if let Some(app) = app_borrow.as_mut() {
                    app.layout(Size::new(width as f64, height as f64));
                    app.paint(gl_ctx);
                }
            });
        }
    });

    // ── 4. Draw rotating 3D cube on top ─────────────────────────────────────
    let cube_rect = CUBE_SCREEN_RECT.with(|r| r.get());
    GL_STATE.with(|gl_cell| {
        if let Some(state) = gl_cell.borrow_mut().as_mut() {
            unsafe { state.draw_cube_only(cube_rect, width as i32, height as i32); }
        }
    });
}

// ---------------------------------------------------------------------------
// Software render pixel readback — for visual testing
// ---------------------------------------------------------------------------

/// Render the same app via the AGG software path and return raw RGBA pixels.
///
/// The framebuffer is Y-up (row 0 = bottom).  For HTML Canvas `putImageData`
/// (which is Y-down), flip the rows in JS or use `pixels_flipped`.
/// Returns a byte array of length `width * height * 4` (RGBA, 8-bit per channel).
#[wasm_bindgen]
pub fn render_software_pixels(width: u32, height: u32) -> Vec<u8> {
    use agg_gui::{Framebuffer, GfxCtx};
    ensure_demo_app();

    let mut fb = Framebuffer::new(width, height);
    DEMO_APP.with(|app_cell| {
        let mut app_borrow = app_cell.borrow_mut();
        if let Some(app) = app_borrow.as_mut() {
            let mut ctx = GfxCtx::new(&mut fb);
            app.layout(Size::new(width as f64, height as f64));
            app.paint(&mut ctx);
        }
    });

    // Return Y-down (flipped) so JS putImageData works directly.
    fb.pixels_flipped()
}

// ---------------------------------------------------------------------------
// Focused text-rendering test exports
// ---------------------------------------------------------------------------
//
// These render ONLY the text string "TESTING FONT RENDERING" on a white
// background, using each render path independently.  The test calls all three
// and compares the resulting pixel buffers to isolate failures:
//
//   render_text_software     — AGG rasterizer (ground truth)
//   render_text_tess_agg     — tess2 triangles drawn with AGG (tests tess2 geometry)
//   render_text_gl_pixels    — tess2 triangles submitted to WebGL (tests GL pipeline)
//
// If software ≈ tess_agg, tess2 geometry is correct.
// If tess_agg ≈ gl, the GL pipeline is correct.

/// Render "TESTING FONT RENDERING" via the AGG software path.
/// Returns Y-down RGBA bytes (ready for `putImageData`).
#[wasm_bindgen]
pub fn render_text_software(width: u32, height: u32) -> Vec<u8> {
    use agg_gui::{Color, Framebuffer, GfxCtx};

    let mut fb = Framebuffer::new(width, height);
    let font = make_font();
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::rgba(1.0, 1.0, 1.0, 1.0));
        ctx.set_font(Arc::clone(&font));
        ctx.set_font_size(24.0);
        ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, 1.0));
        ctx.fill_text("TESTING FONT RENDERING", 20.0, 40.0);
    }
    fb.pixels_flipped()
}

/// Render "TESTING FONT RENDERING" by tessellating glyph outlines with tess2
/// and drawing the resulting triangles with the AGG software rasterizer.
///
/// This isolates tess2 geometry from the WebGL pipeline: if this output matches
/// `render_text_software`, tess2 is producing correct triangles and any visual
/// discrepancy in `render_text_gl_pixels` is a GL-side issue.
///
/// Returns Y-down RGBA bytes (ready for `putImageData`).
#[wasm_bindgen]
pub fn render_text_tess_agg_pixels(width: u32, height: u32) -> Vec<u8> {
    use agg_gui::{Color, Framebuffer, GfxCtx};
    use agg_gui::text::shape_and_flatten_text_via_agg;

    let mut fb = Framebuffer::new(width, height);
    let font = make_font();
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::rgba(1.0, 1.0, 1.0, 1.0));
        ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, 1.0));

        // Flatten glyphs using AGG's own ConvCurve — same algorithm as
        // the software rasterizer.  Draw those contours DIRECTLY via AGG
        // (no tess2) to prove the contour geometry is correct.
        // Returns Vec<Vec<Vec<[f32;2]>>> — one entry per glyph, each glyph has
        // one or more contours (outer + optional counter holes).
        let glyphs = shape_and_flatten_text_via_agg(
            &font, "TESTING FONT RENDERING", 24.0, 20.0, 40.0,
        );

        // Draw each glyph's contours as ONE path so AGG's non-zero fill rule
        // handles counter holes (CW outer + CCW inner = hole) correctly —
        // the same way rasterize_fill does for the software path.
        for glyph_contours in &glyphs {
            ctx.begin_path();
            for contour in glyph_contours {
                if contour.len() < 2 { continue; }
                for (i, &[x, y]) in contour.iter().enumerate() {
                    if i == 0 { ctx.move_to(x as f64, y as f64); }
                    else { ctx.line_to(x as f64, y as f64); }
                }
            }
            ctx.fill();
        }
    }
    fb.pixels_flipped()
}

/// Render "TESTING FONT RENDERING" via the GL/tess2 path and return raw RGBA
/// pixels (Y-down, same format as `render_text_software`).
///
/// Uses `gl.readPixels` to capture the result within the same task (before the
/// browser compositor clears the framebuffer).  Does NOT resize the canvas, so
/// the WebGL context remains valid across calls.  The render is always done into
/// a `width × height` region anchored at the bottom-left of the canvas.
#[wasm_bindgen]
pub fn render_text_gl_pixels(width: u32, height: u32) -> Vec<u8> {
    ensure_gl_state();
    ensure_gl_ctx(width as f32, height as f32);

    // ── 1. GL clear ──────────────────────────────────────────────────────────
    GL_STATE.with(|gl_cell| {
        if let Some(state) = gl_cell.borrow().as_ref() {
            let gl = state.gl_rc();
            unsafe {
                use glow::HasContext;
                gl.viewport(0, 0, width as i32, height as i32);
                gl.clear_color(1.0, 1.0, 1.0, 1.0);
                gl.clear(glow::COLOR_BUFFER_BIT);
                gl.enable(glow::BLEND);
                gl.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
                gl.disable(glow::DEPTH_TEST);
                gl.disable(glow::SCISSOR_TEST);
            }
        }
    });

    // ── 2. Draw text via GL / tess2 ──────────────────────────────────────────
    // GlGfxCtx is Y-up (y=0 at bottom).  Baseline y=40 matches GfxCtx (also
    // Y-up): both put the baseline 40 px above the bottom of the render target.
    let font = make_font();
    GL_CTX.with(|ctx_cell| {
        let mut ctx_borrow = ctx_cell.borrow_mut();
        if let Some(gl_ctx) = ctx_borrow.as_mut() {
            gl_ctx.reset(width as f32, height as f32);
            gl_ctx.set_font(Arc::clone(&font));
            gl_ctx.set_font_size(24.0);
            gl_ctx.set_fill_color(agg_gui::Color::rgba(0.0, 0.0, 0.0, 1.0));
            gl_ctx.fill_text("TESTING FONT RENDERING", 20.0, 40.0);
        }
    });

    // ── 3. Read pixels (Y-up, bottom-left origin) ────────────────────────────
    let byte_count = (width * height * 4) as usize;
    let mut raw = vec![0u8; byte_count];
    GL_STATE.with(|gl_cell| {
        if let Some(state) = gl_cell.borrow().as_ref() {
            let gl = state.gl_rc();
            unsafe {
                use glow::HasContext;
                gl.read_pixels(
                    0, 0, width as i32, height as i32,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelPackData::Slice(&mut raw),
                );
            }
        }
    });

    // ── 4. Flip Y so output is Y-down (matches render_text_software) ─────────
    let stride = (width * 4) as usize;
    let h = height as usize;
    let mut flipped = vec![0u8; byte_count];
    for row in 0..h {
        let src = &raw[row * stride..(row + 1) * stride];
        let dst_row = h - 1 - row;
        flipped[dst_row * stride..(dst_row + 1) * stride].copy_from_slice(src);
    }
    flipped
}

// ---------------------------------------------------------------------------
// WASM event exports — single unified set
// ---------------------------------------------------------------------------

#[wasm_bindgen]
pub fn on_mouse_move(x: f64, y: f64) {
    DEMO_APP.with(|cell| {
        if let Some(app) = cell.borrow_mut().as_mut() {
            app.on_mouse_move(x, y);
        }
    });
}

#[wasm_bindgen]
pub fn on_mouse_down(x: f64, y: f64, button: u8) {
    let btn = match button {
        0 => MouseButton::Left, 1 => MouseButton::Middle, 2 => MouseButton::Right,
        n => MouseButton::Other(n),
    };
    DEMO_APP.with(|cell| {
        if let Some(app) = cell.borrow_mut().as_mut() {
            app.on_mouse_down(x, y, btn, Modifiers::default());
        }
    });
}

#[wasm_bindgen]
pub fn on_mouse_up(x: f64, y: f64, button: u8) {
    let btn = match button {
        0 => MouseButton::Left, 1 => MouseButton::Middle, 2 => MouseButton::Right,
        n => MouseButton::Other(n),
    };
    DEMO_APP.with(|cell| {
        if let Some(app) = cell.borrow_mut().as_mut() {
            app.on_mouse_up(x, y, btn, Modifiers::default());
        }
    });
}

#[wasm_bindgen]
pub fn on_mouse_wheel(x: f64, y: f64, delta_y: f64) {
    DEMO_APP.with(|cell| {
        if let Some(app) = cell.borrow_mut().as_mut() {
            app.on_mouse_wheel(x, y, delta_y);
        }
    });
}

#[wasm_bindgen]
pub fn on_mouse_leave() {
    DEMO_APP.with(|cell| {
        if let Some(app) = cell.borrow_mut().as_mut() {
            app.on_mouse_leave();
        }
    });
}

#[wasm_bindgen]
pub fn on_key_down(key_str: &str, shift: bool, ctrl: bool, alt: bool) {
    if let Some(key) = parse_js_key(key_str) {
        let mods = Modifiers { shift, ctrl, alt };
        DEMO_APP.with(|cell| {
            if let Some(app) = cell.borrow_mut().as_mut() {
                app.on_key_down(key, mods);
            }
        });
    }
}
