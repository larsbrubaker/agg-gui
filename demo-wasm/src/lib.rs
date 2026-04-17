//! WASM demo crate for agg-gui.
//!
//! This crate is a **rendering harness only** — it wires up WebGL2 resources,
//! browser event forwarding, and frame presentation. All demo/UI code belongs
//! in `demo-ui`; this crate should contain no widget or layout logic.
//!
//! WASM exports:
//! - `render(width, height)` — full-frame render (void; GL writes to canvas)
//! - `on_mouse_move/down/up/wheel/leave` — mouse events
//! - `on_key_down` — keyboard events

mod gl_resources;

use demo_gl::{GlGfxCtx, begin_frame, sync_inspector, render_app_frame};
use gl_resources::{GlCubeWidget, GlState, CUBE_SCREEN_RECT};

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use agg_gui::{App, Font, InspectorNode, Key, Modifiers, MouseButton, Rect, Size};

// Embed the font family at compile time.  The primary font is CascadiaCode;
// Font Awesome 4 supplies the sidebar/button icons (private-use codepoints);
// NotoEmoji fills in true emoji.  Same fallback chain as the native harness.
const FONT_BYTES:  &[u8] = include_bytes!("../../demo/assets/CascadiaCode.ttf");
const FA_BYTES:    &[u8] = include_bytes!("../../demo/assets/fa.ttf");
const EMOJI_BYTES: &[u8] = include_bytes!("../../demo/assets/NotoEmoji-Regular.ttf");

fn make_font() -> Arc<Font> {
    let emoji = Font::from_slice(EMOJI_BYTES).expect("parse NotoEmoji-Regular.ttf");
    let fa    = Font::from_slice(FA_BYTES).expect("parse fa.ttf")
        .with_fallback(Arc::new(emoji));
    Arc::new(
        Font::from_slice(FONT_BYTES).expect("parse CascadiaCode.ttf")
            .with_fallback(Arc::new(fa))
    )
}

// ---------------------------------------------------------------------------
// Thread-local state
// ---------------------------------------------------------------------------

thread_local! {
    static DEMO_APP:  RefCell<Option<App>>       = RefCell::new(None);
    static GL_STATE:  RefCell<Option<GlState>>   = RefCell::new(None);
    /// Persistent GL 2-D drawing context — created once, reset each frame.
    static GL_CTX:    RefCell<Option<GlGfxCtx>>  = RefCell::new(None);

    // Inspector shared state — set once by build_demo_app, read each frame.
    static SHOW_INSPECTOR:  RefCell<Option<Rc<Cell<bool>>>>                     = RefCell::new(None);
    static INSPECTOR_NODES: RefCell<Option<Rc<RefCell<Vec<InspectorNode>>>>>    = RefCell::new(None);
    /// Shared hover-bounds handle — written by the inspector, read by render().
    static HOVERED_BOUNDS: RefCell<Option<Rc<RefCell<Option<Rect>>>>>           = RefCell::new(None);
    /// Current canvas dimensions — written each frame, read by the backend panel.
    static SCREEN_SIZE: RefCell<Option<Rc<Cell<(u32, u32)>>>>                   = RefCell::new(None);
    /// Accessor for reading window open/position state for localStorage persistence.
    static STATE_ACCESSOR: RefCell<Option<demo_ui::StateAccessor>>                              = RefCell::new(None);
    /// Shared frame history — written each frame so the backend panel shows live CPU usage.
    static FRAME_HISTORY: RefCell<Option<Rc<RefCell<demo_ui::FrameHistory>>>>                   = RefCell::new(None);
    /// Frame counter used to throttle localStorage saves.
    static FRAME_COUNT: Cell<u32> = Cell::new(0);
    /// Repaint dirty flag — set by any input handler, cleared by `render()`.
    /// The JS animation loop calls `needs_repaint()` each rAF tick and skips
    /// `render()` when nothing has changed, matching the native harness's
    /// Wait / WaitUntil behaviour.
    static NEEDS_REPAINT: Cell<bool> = Cell::new(true);
    /// Share the cube-visibility + focus flags so `needs_repaint()` can keep
    /// the loop running while animation or cursor blink is in progress.
    static CUBE_VISIBLE: RefCell<Option<Rc<Cell<bool>>>> = RefCell::new(None);
    /// Screenshot request flag — set by the demo button, cleared by render().
    static SCREENSHOT_REQUEST: RefCell<Option<Rc<Cell<bool>>>>                  = RefCell::new(None);
    /// Shared latest-screenshot image (top-down RGBA8 + dims).
    static SCREENSHOT_IMAGE:   RefCell<Option<Rc<RefCell<Option<(Vec<u8>, u32, u32)>>>>> = RefCell::new(None);
}

/// Initialise panic hook so Rust panics appear in the browser console.
#[wasm_bindgen(start)]
pub fn wasm_start() {
    console_error_panic_hook::set_once();
}

// ---------------------------------------------------------------------------
// State persistence helpers (localStorage)
// ---------------------------------------------------------------------------

fn load_state_wasm() -> Option<demo_ui::SavedState> {
    let storage = web_sys::window()?.local_storage().ok()??;
    let s = storage.get_item("agg-gui-demo-state").ok()??;
    demo_ui::SavedState::deserialize(&s)
}

fn save_state_wasm(accessor: &demo_ui::StateAccessor) {
    if let Some(storage) = web_sys::window()
        .and_then(|w| w.local_storage().ok().flatten())
    {
        let state = accessor.current_state();
        let _ = storage.set_item("agg-gui-demo-state", &state.serialize());
    }
}

fn ensure_demo_app() {
    DEMO_APP.with(|cell| {
        if cell.borrow().is_none() {
            let font = make_font();
            let initial_state = load_state_wasm();
            let (app, handles) = demo_ui::build_demo_ui(
                Arc::clone(&font),
                Box::new(GlCubeWidget::new()),
                "WebGL2",
                "Browser WebGL2",
                initial_state,
            );
            SHOW_INSPECTOR.with(|c| *c.borrow_mut() = Some(Rc::clone(&handles.show_inspector)));
            INSPECTOR_NODES.with(|c| *c.borrow_mut() = Some(Rc::clone(&handles.inspector_nodes)));
            HOVERED_BOUNDS.with(|c| *c.borrow_mut() = Some(Rc::clone(&handles.hovered_bounds)));
            SCREEN_SIZE.with(|c| *c.borrow_mut() = Some(Rc::clone(&handles.screen_size)));
            FRAME_HISTORY.with(|c| *c.borrow_mut() = Some(Rc::clone(&handles.frame_history)));
            SCREENSHOT_REQUEST.with(|c| *c.borrow_mut() = Some(Rc::clone(&handles.screenshot_request)));
            SCREENSHOT_IMAGE.with(|c| *c.borrow_mut() = Some(Rc::clone(&handles.screenshot_image)));
            CUBE_VISIBLE.with(|c| *c.borrow_mut() = Some(Rc::clone(&handles.cube_visible)));
            STATE_ACCESSOR.with(|c| *c.borrow_mut() = Some(handles.state));
            *cell.borrow_mut() = Some(app);
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

// ---------------------------------------------------------------------------
// WASM render export
// ---------------------------------------------------------------------------

/// Full-frame render.  Direct GL path: the widget tree is painted via
/// `GlGfxCtx` (tess2 tessellation → WebGL2 draw calls).  No off-screen
/// framebuffer is used.  The rotating 3D cube is drawn last, on top.
///
/// `frame_ms` is the render time of the *previous* frame, measured by the JS
/// caller.  It is shown in the bottom-left status overlay (identical to the
/// native path).
#[wasm_bindgen]
pub fn render(width: u32, height: u32, frame_ms: f64) {
    ensure_demo_app();
    ensure_gl_state();
    ensure_gl_ctx(width as f32, height as f32);

    // ── 1. GL clear ─────────────────────────────────────────────────────────
    GL_STATE.with(|gl_cell| {
        if let Some(state) = gl_cell.borrow().as_ref() {
            begin_frame(&state.gl_rc(), width, height);
        }
    });

    // ── 2. Sync inspector nodes snapshot (before paint) ─────────────────────
    let show_inspector = SHOW_INSPECTOR.with(|c| c.borrow().as_ref().map(|r| r.get()).unwrap_or(false));
    DEMO_APP.with(|app_cell| {
        if let Some(app) = app_cell.borrow().as_ref() {
            INSPECTOR_NODES.with(|nodes_cell| {
                if let Some(ref nodes_rc) = *nodes_cell.borrow() {
                    HOVERED_BOUNDS.with(|hb_cell| {
                        if let Some(ref hb_rc) = *hb_cell.borrow() {
                            sync_inspector(app, show_inspector, nodes_rc, hb_rc);
                        }
                    });
                }
            });
        }
    });

    // ── 3. Update screen size for the backend panel ─────────────────────────
    SCREEN_SIZE.with(|c| {
        if let Some(ref rc) = *c.borrow() {
            rc.set((width, height));
        }
    });

    // ── 4. Paint widget tree (cube draws inline via DrawCtx::gl_paint) ──────
    CUBE_SCREEN_RECT.with(|r| r.set(agg_gui::Rect::default()));
    GL_CTX.with(|ctx_cell| {
        let mut ctx_borrow = ctx_cell.borrow_mut();
        if let Some(gl_ctx) = ctx_borrow.as_mut() {
            let hovered = HOVERED_BOUNDS.with(|c| {
                c.borrow().as_ref().and_then(|rc| *rc.borrow())
            });
            DEMO_APP.with(|app_cell| {
                let mut app_borrow = app_cell.borrow_mut();
                if let Some(app) = app_borrow.as_mut() {
                    render_app_frame(gl_ctx, app, width, height, frame_ms, hovered);
                }
            });

            // Satisfy any pending screenshot request — must happen BEFORE the
            // browser presents the frame so the WebGL back buffer still holds
            // these pixels.
            let requested = SCREENSHOT_REQUEST.with(|c| {
                c.borrow().as_ref().map(|rc| rc.get()).unwrap_or(false)
            });
            if requested {
                let (rgba, w, h) = gl_ctx.read_screenshot();
                SCREENSHOT_IMAGE.with(|c| {
                    if let Some(ref rc) = *c.borrow() {
                        *rc.borrow_mut() = Some((rgba, w, h));
                    }
                });
                SCREENSHOT_REQUEST.with(|c| {
                    if let Some(ref rc) = *c.borrow() { rc.set(false); }
                });
            }
        }
    });

    // ── 5. Push frame time to history so backend panel shows live CPU usage ───
    if frame_ms > 0.0 {
        FRAME_HISTORY.with(|c| {
            if let Some(ref rc) = *c.borrow() {
                rc.borrow_mut().push(frame_ms as f32);
            }
        });
    }

    // ── 7. Periodically save window layout to localStorage ──────────────────
    let fc = FRAME_COUNT.get() + 1;
    FRAME_COUNT.set(fc);
    if fc % 120 == 0 {
        STATE_ACCESSOR.with(|c| {
            if let Some(ref acc) = *c.borrow() {
                save_state_wasm(acc);
            }
        });
    }

    // Frame successfully rendered — clear the dirty flag.  `needs_repaint()`
    // will return `true` again only if an event fires or an animation source
    // (cube / focus) still needs frames.
    NEEDS_REPAINT.with(|c| c.set(false));
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

        let glyphs = shape_and_flatten_text_via_agg(
            &font, "TESTING FONT RENDERING", 24.0, 20.0, 40.0,
        );

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
#[wasm_bindgen]
pub fn render_text_gl_pixels(width: u32, height: u32) -> Vec<u8> {
    ensure_gl_state();
    ensure_gl_ctx(width as f32, height as f32);

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
// Clipboard bridge
//
// The JS harness reads/writes the in-process clipboard buffer to connect
// Rust's copy/cut/paste logic to the browser's system clipboard.
// See `agg_gui::wasm_clipboard` for the buffer implementation.
// ---------------------------------------------------------------------------

/// Read the in-process clipboard buffer.  Returns `None` when empty.
/// Called by the JS `copy`/`cut` DOM event handler to populate
/// `event.clipboardData` before the browser commits to the system clipboard.
#[wasm_bindgen]
pub fn wasm_clipboard_get() -> Option<String> {
    agg_gui::wasm_clipboard::get()
}

/// Write `text` into the in-process clipboard buffer.
/// Called by the JS `paste` DOM event handler with the text from
/// `event.clipboardData` before synthesising a Ctrl+V key event.
#[wasm_bindgen]
pub fn wasm_clipboard_set(text: &str) {
    agg_gui::wasm_clipboard::set(text);
}

// ---------------------------------------------------------------------------
// WASM event exports
// ---------------------------------------------------------------------------

#[wasm_bindgen]
pub fn on_mouse_move(x: f64, y: f64) {
    mark_dirty();
    DEMO_APP.with(|cell| {
        if let Some(app) = cell.borrow_mut().as_mut() {
            app.on_mouse_move(x, y);
        }
    });
    // Apply CSS cursor to the canvas element.
    if let Some(window) = web_sys::window() {
        if let Some(doc) = window.document() {
            if let Some(el) = doc.get_element_by_id("canvas") {
                let css = agg_gui::current_cursor_icon().to_css();
                let _ = el.set_attribute("style", &format!("cursor:{css}"));
            }
        }
    }
}

#[wasm_bindgen]
pub fn on_mouse_down(x: f64, y: f64, button: u8) {
    mark_dirty();
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
    mark_dirty();
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
    mark_dirty();
    DEMO_APP.with(|cell| {
        if let Some(app) = cell.borrow_mut().as_mut() {
            app.on_mouse_wheel(x, y, delta_y);
        }
    });
}

#[wasm_bindgen]
pub fn on_mouse_leave() {
    mark_dirty();
    DEMO_APP.with(|cell| {
        if let Some(app) = cell.borrow_mut().as_mut() {
            app.on_mouse_leave();
        }
    });
}

#[wasm_bindgen]
pub fn on_key_down(key_str: &str, shift: bool, ctrl: bool, alt: bool) {
    mark_dirty();
    if let Some(key) = parse_js_key(key_str) {
        let mods = Modifiers { shift, ctrl, alt };
        DEMO_APP.with(|cell| {
            if let Some(app) = cell.borrow_mut().as_mut() {
                app.on_key_down(key, mods);
            }
        });
    }
}

/// Called by the JS animation loop each frame.  Returns `true` when the frame
/// needs to be re-rendered: an input event landed since the last render, a
/// continuously-animating widget (3-D cube) is visible, a text field has
/// focus (cursor blink), or a screenshot has been requested.
#[wasm_bindgen]
pub fn needs_repaint() -> bool {
    if NEEDS_REPAINT.with(|c| c.get()) { return true; }
    // Animation-driven: cube, focus, continuous-capture screenshot.
    let cube_on = CUBE_VISIBLE.with(|c| c.borrow().as_ref().map(|rc| rc.get()).unwrap_or(false));
    if cube_on { return true; }
    let ss_req = SCREENSHOT_REQUEST.with(|c| c.borrow().as_ref().map(|rc| rc.get()).unwrap_or(false));
    if ss_req { return true; }
    let has_focus = DEMO_APP.with(|c| c.borrow().as_ref().map(|a| a.has_focus()).unwrap_or(false));
    if has_focus { return true; }
    false
}

fn mark_dirty() { NEEDS_REPAINT.with(|c| c.set(true)); }

// ---------------------------------------------------------------------------
// Key parsing
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
