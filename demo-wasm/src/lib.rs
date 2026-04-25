//! WASM demo crate for agg-gui.
//!
//! # Platform-split policy (kept identical across `demo-native`, `demo-wasm`, `demo-gl`)
//!
//! This crate is a **platform shell only** — it wires up WebGL2
//! resources, browser event forwarding, frame presentation, and
//! `localStorage` for state persistence.  It contains **no demo
//! content**: every widget tree, layout, and GL renderer the user
//! sees is shared.
//!
//! - **Widget / layout code** → `demo-ui`
//! - **GL renderers (shaders, geometry, draw calls)** → `demo-gl`
//!   (e.g. `demo_gl::GlCubeWidget`, the 3D Animation widget)
//! - **Platform shell (canvas + event forwarding + persistence
//!   backend)** → here (`demo-wasm`) and `demo-native`
//!
//! If you find yourself adding a widget, shader, or piece of demo
//! content in this file or `gl_resources.rs` — stop and put it in
//! `demo-ui` or `demo-gl` instead.  Native local testing is only
//! meaningful as a proxy for this deployed WASM build when both
//! targets share the same compiled demo content; duplicating into a
//! platform crate breaks that contract.
//!
//! WASM exports:
//! - `render(width, height)` — full-frame render (void; GL writes to canvas)
//! - `on_mouse_move/down/up/wheel/leave` — mouse events
//! - `on_key_down` — keyboard events

mod gl_resources;

use demo_gl::{begin_frame, render_app_frame, GlGfxCtx};
use gl_resources::{GlCubeWidget, GlState, CUBE_SCREEN_RECT};

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{App, Font, InspectorNode, Key, Modifiers, MouseButton, Rect, Size};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

// Embed the font family at compile time.  The primary font is CascadiaCode;
// Font Awesome 4 supplies the sidebar/button icons (private-use codepoints);
// NotoEmoji fills in true emoji.  Same fallback chain as the native harness.
const FONT_BYTES: &[u8] = include_bytes!("../../demo/assets/CascadiaCode.ttf");
const FA_BYTES: &[u8] = include_bytes!("../../demo/assets/fa.ttf");
const EMOJI_BYTES: &[u8] = include_bytes!("../../demo/assets/NotoEmoji-Regular.ttf");

fn make_font() -> Arc<Font> {
    let emoji = Font::from_slice(EMOJI_BYTES).expect("parse NotoEmoji-Regular.ttf");
    let fa = Font::from_slice(FA_BYTES)
        .expect("parse fa.ttf")
        .with_fallback(Arc::new(emoji));
    Arc::new(
        Font::from_slice(FONT_BYTES)
            .expect("parse CascadiaCode.ttf")
            .with_fallback(Arc::new(fa)),
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
    /// Mouse-buttons-currently-held counter.  Used to defer auto-save until
    /// the user releases the drag / resize so we don't hammer localStorage.
    static MOUSE_BUTTONS_DOWN: Cell<u32> = Cell::new(0);
    /// Shared auto-save tracker — compares fresh serialized state to the
    /// previously-persisted blob so we only touch localStorage when state
    /// has actually changed.  See `agg_gui::persistence::AutoSave`.
    static AUTO_SAVE: RefCell<agg_gui::persistence::AutoSave> =
        RefCell::new(agg_gui::persistence::AutoSave::new());
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
    static SCREENSHOT_IMAGE:   RefCell<Option<Rc<RefCell<Option<(Arc<Vec<u8>>, u32, u32)>>>>> = RefCell::new(None);
    /// Transient flag set by `render()` around the first pass of a capture
    /// frame so the preview pane hides itself (prevents recursive nesting).
    static SCREENSHOT_CAPTURING:  RefCell<Option<Rc<Cell<bool>>>>               = RefCell::new(None);
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

/// Unused legacy helper; retained for a brief transition while callers move
/// to the diff-based auto-save in `render()`.
#[allow(dead_code)]
fn save_state_wasm(accessor: &demo_ui::StateAccessor) {
    if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        let state = accessor.current_state();
        let _ = storage.set_item("agg-gui-demo-state", &state.serialize());
    }
}

fn ensure_demo_app() {
    DEMO_APP.with(|cell| {
        if cell.borrow().is_none() {
            let font = make_font();
            let initial_state = load_state_wasm();
            // Refresh button on the Render tab — WebGL2's `antialias`
            // attribute is only honoured at canvas-context creation time,
            // so the only way to apply a new MSAA choice is to reload the
            // page.  Keeping the reload here means demo-ui has no need to
            // import `web_sys`.
            //
            // `running_msaa` mirrors the `antialias` flag that `init_webgl2`
            // (runs later in the same tick) will hand to the browser:
            // `> 0` = antialias on.  Browsers don't expose the actual
            // sample count they chose, so we report a nominal `4` when on.
            let running_msaa_on = initial_state
                .as_ref()
                .map(|s| s.msaa_samples > 0)
                .unwrap_or(false);
            let running_msaa: u8 = if running_msaa_on { 4 } else { 0 };
            let platform = demo_ui::PlatformHooks::web(running_msaa, || {
                if let Some(win) = web_sys::window() {
                    let _ = win.location().reload();
                }
            });
            let (app, handles) = demo_ui::build_demo_ui(
                Arc::clone(&font),
                Box::new(GlCubeWidget::new()),
                "WebGL2",
                "Browser WebGL2",
                initial_state,
                platform,
            );
            SHOW_INSPECTOR.with(|c| *c.borrow_mut() = Some(Rc::clone(&handles.show_inspector)));
            INSPECTOR_NODES.with(|c| *c.borrow_mut() = Some(Rc::clone(&handles.inspector_nodes)));
            HOVERED_BOUNDS.with(|c| *c.borrow_mut() = Some(Rc::clone(&handles.hovered_bounds)));
            SCREEN_SIZE.with(|c| *c.borrow_mut() = Some(Rc::clone(&handles.screen_size)));
            FRAME_HISTORY.with(|c| *c.borrow_mut() = Some(Rc::clone(&handles.frame_history)));
            SCREENSHOT_REQUEST
                .with(|c| *c.borrow_mut() = Some(Rc::clone(&handles.screenshot_request)));
            SCREENSHOT_IMAGE.with(|c| *c.borrow_mut() = Some(Rc::clone(&handles.screenshot_image)));
            SCREENSHOT_CAPTURING
                .with(|c| *c.borrow_mut() = Some(Rc::clone(&handles.screenshot_capturing)));
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
    let gl_rc = GL_STATE.with(|cell| cell.borrow().as_ref().map(|s| s.gl_rc()));
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

    // WebGL2's `antialias` attribute is a boolean, fixed at context
    // creation time — the browser picks the sample count (typically 4×).
    // Read the persisted MSAA request directly so we don't require the
    // caller to thread state through, matching how `ensure_demo_app`
    // already independently reads from localStorage.  `msaa_samples > 0`
    // maps to `antialias: true`.
    let msaa_on = load_state_wasm()
        .map(|s| s.msaa_samples > 0)
        .unwrap_or(false);

    let attrs = js_sys::Object::new();
    let _ = js_sys::Reflect::set(
        &attrs,
        &JsValue::from_str("antialias"),
        &JsValue::from_bool(msaa_on),
    );

    let webgl2 = canvas
        .get_context_with_context_options("webgl2", &attrs)
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

    // ── 1. Update screen size for the backend panel ─────────────────────────
    SCREEN_SIZE.with(|c| {
        if let Some(ref rc) = *c.borrow() {
            rc.set((width, height));
        }
    });

    // ── 2. Paint widget tree through the shared capture orchestration ──────
    //
    // `run_frame_with_capture` does the single-render path in the common
    // case and the double-render capture path when `screenshot_request`
    // is set.  Each render pass begins by clearing the GL back buffer and
    // then paints the widget tree via `render_app_frame` (which also
    // syncs the inspector snapshot internally).
    CUBE_SCREEN_RECT.with(|r| r.set(agg_gui::Rect::default()));
    let show_inspector =
        SHOW_INSPECTOR.with(|c| c.borrow().as_ref().map(|r| r.get()).unwrap_or(false));
    let screenshot_request_rc = SCREENSHOT_REQUEST.with(|c| c.borrow().as_ref().map(Rc::clone));
    let screenshot_capturing_rc = SCREENSHOT_CAPTURING.with(|c| c.borrow().as_ref().map(Rc::clone));
    let screenshot_image_rc = SCREENSHOT_IMAGE.with(|c| c.borrow().as_ref().map(Rc::clone));
    let inspector_nodes_rc = INSPECTOR_NODES.with(|c| c.borrow().as_ref().map(Rc::clone));
    let hovered_bounds_rc = HOVERED_BOUNDS.with(|c| c.borrow().as_ref().map(Rc::clone));
    let gl_rc_for_clear = GL_STATE.with(|gl_cell| gl_cell.borrow().as_ref().map(|s| s.gl_rc()));
    if let (Some(req), Some(cap), Some(img), Some(nodes), Some(hb), Some(gl_rc)) = (
        screenshot_request_rc,
        screenshot_capturing_rc,
        screenshot_image_rc,
        inspector_nodes_rc,
        hovered_bounds_rc,
        gl_rc_for_clear,
    ) {
        GL_CTX.with(|ctx_cell| {
            let mut ctx_borrow = ctx_cell.borrow_mut();
            if let Some(gl_ctx) = ctx_borrow.as_mut() {
                agg_gui::screenshot::run_frame_with_capture(
                    &req,
                    &cap,
                    &img,
                    gl_ctx,
                    |gc| {
                        // Each pass clears the back buffer and then paints.
                        begin_frame(&gl_rc, width, height);
                        DEMO_APP.with(|app_cell| {
                            let mut app_borrow = app_cell.borrow_mut();
                            if let Some(app) = app_borrow.as_mut() {
                                render_app_frame(
                                    gc,
                                    app,
                                    width,
                                    height,
                                    frame_ms,
                                    show_inspector,
                                    &nodes,
                                    &hb,
                                );
                            }
                        });
                    },
                    |gc| gc.read_screenshot(),
                );
            }
        });
    }

    // ── 5. Push frame time to history so backend panel shows live CPU usage ───
    if frame_ms > 0.0 {
        FRAME_HISTORY.with(|c| {
            if let Some(ref rc) = *c.borrow() {
                rc.borrow_mut().push(frame_ms as f32);
            }
        });
    }

    // ── 7. Auto-save layout when state changes ─────────────────────────────
    // Same policy as native: diff the serialized state against the last
    // persisted blob, write only on change and only while no mouse button
    // is held.  The `AutoSave` helper in `agg_gui::persistence` owns the
    // diff-and-write logic; this shell only supplies the serializer and
    // the localStorage backend.
    FRAME_COUNT.set(FRAME_COUNT.get() + 1);
    let idle = MOUSE_BUTTONS_DOWN.get() == 0;
    STATE_ACCESSOR.with(|c| {
        if let Some(ref acc) = *c.borrow() {
            AUTO_SAVE.with_borrow_mut(|auto| {
                auto.tick(
                    idle,
                    || acc.current_state().serialize(),
                    |s| {
                        if let Some(storage) =
                            web_sys::window().and_then(|w| w.local_storage().ok().flatten())
                        {
                            let _ = storage.set_item("agg-gui-demo-state", s);
                        }
                    },
                );
            });
        }
    });

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
    use agg_gui::text::shape_and_flatten_text_via_agg;
    use agg_gui::{Color, Framebuffer, GfxCtx};

    let mut fb = Framebuffer::new(width, height);
    let font = make_font();
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::rgba(1.0, 1.0, 1.0, 1.0));
        ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, 1.0));

        let glyphs =
            shape_and_flatten_text_via_agg(&font, "TESTING FONT RENDERING", 24.0, 20.0, 40.0);

        for glyph_contours in &glyphs {
            ctx.begin_path();
            for contour in glyph_contours {
                if contour.len() < 2 {
                    continue;
                }
                for (i, &[x, y]) in contour.iter().enumerate() {
                    if i == 0 {
                        ctx.move_to(x as f64, y as f64);
                    } else {
                        ctx.line_to(x as f64, y as f64);
                    }
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
                    0,
                    0,
                    width as i32,
                    height as i32,
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

/// Publish the browser's `window.devicePixelRatio`.  JS calls this once at
/// init and again whenever the DPR changes (zoom, window moves to a
/// different-DPI screen on desktops).  The widget tree then paints at
/// physical pixel density instead of logical pixels — the difference
/// between "comfortable" text on a high-DPR phone and "miniature" text.
#[wasm_bindgen]
pub fn set_device_pixel_ratio(dpr: f64) {
    agg_gui::set_device_scale(dpr.max(0.5));
    mark_dirty();
}

// NO blanket `mark_dirty()` at the app-event boundary.  A mouse-move over
// inert canvas, a key that no focused widget consumes, a mouse-up that
// released over empty space — none of these should force a repaint on
// their own.  Widgets that change visible state in response to these
// events call `crate::animation::request_tick()` themselves; the tree-walk
// `needs_paint` path in `needs_repaint()` picks it up.
#[wasm_bindgen]
pub fn on_mouse_move(x: f64, y: f64) {
    DEMO_APP.with(|cell| {
        if let Some(app) = cell.borrow_mut().as_mut() {
            app.on_mouse_move(x, y);
        }
    });
    // Apply CSS cursor to the canvas element.  `agg_gui::web_adapter`
    // owns the `CursorIcon` → CSS style-string conversion so future
    // consumers drop into the same helper instead of rebuilding it.
    if let Some(window) = web_sys::window() {
        if let Some(doc) = window.document() {
            if let Some(el) = doc.get_element_by_id("canvas") {
                let style = agg_gui::web_adapter::cursor_style(agg_gui::current_cursor_icon());
                let _ = el.set_attribute("style", &style);
            }
        }
    }
}

#[wasm_bindgen]
pub fn on_mouse_down(x: f64, y: f64, button: u8) {
    MOUSE_BUTTONS_DOWN.set(MOUSE_BUTTONS_DOWN.get().saturating_add(1));
    let btn = match button {
        0 => MouseButton::Left,
        1 => MouseButton::Middle,
        2 => MouseButton::Right,
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
    MOUSE_BUTTONS_DOWN.set(MOUSE_BUTTONS_DOWN.get().saturating_sub(1));
    let btn = match button {
        0 => MouseButton::Left,
        1 => MouseButton::Middle,
        2 => MouseButton::Right,
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

// ── Multi-touch entry points ───────────────────────────────────────────────
//
// Each active touch is forwarded here by the JS harness.  The Rust side
// maintains the gesture-recogniser (`App::touch_state`) and publishes the
// frame-aggregated `MultiTouchInfo` to a thread-local that widgets read.
// Single-finger touches continue to flow through `on_mouse_*` so
// existing widgets work unchanged.

#[wasm_bindgen]
pub fn on_touch_start(id: u32, x: f64, y: f64, force: f64) {
    DEMO_APP.with(|cell| {
        if let Some(app) = cell.borrow_mut().as_mut() {
            let f = if force > 0.0 {
                Some(force as f32)
            } else {
                None
            };
            app.on_touch_start(
                agg_gui::TouchDeviceId(0),
                agg_gui::TouchId(id as u64),
                x,
                y,
                f,
            );
        }
    });
    mark_dirty();
}

#[wasm_bindgen]
pub fn on_touch_move(id: u32, x: f64, y: f64, force: f64) {
    DEMO_APP.with(|cell| {
        if let Some(app) = cell.borrow_mut().as_mut() {
            let f = if force > 0.0 {
                Some(force as f32)
            } else {
                None
            };
            app.on_touch_move(
                agg_gui::TouchDeviceId(0),
                agg_gui::TouchId(id as u64),
                x,
                y,
                f,
            );
        }
    });
    mark_dirty();
}

#[wasm_bindgen]
pub fn on_touch_end(id: u32) {
    DEMO_APP.with(|cell| {
        if let Some(app) = cell.borrow_mut().as_mut() {
            app.on_touch_end(agg_gui::TouchDeviceId(0), agg_gui::TouchId(id as u64));
        }
    });
    mark_dirty();
}

#[wasm_bindgen]
pub fn on_touch_cancel(id: u32) {
    DEMO_APP.with(|cell| {
        if let Some(app) = cell.borrow_mut().as_mut() {
            app.on_touch_cancel(agg_gui::TouchDeviceId(0), agg_gui::TouchId(id as u64));
        }
    });
    mark_dirty();
}

#[wasm_bindgen]
pub fn on_key_down(key_str: &str, shift: bool, ctrl: bool, alt: bool, meta: bool) {
    if let Some(key) = parse_js_key(key_str) {
        let mods = Modifiers {
            shift,
            ctrl,
            alt,
            meta,
        };
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
    if NEEDS_REPAINT.with(|c| c.get()) {
        return true;
    }
    // Pending capture (button click) — harness will consume on render.
    let ss_req =
        SCREENSHOT_REQUEST.with(|c| c.borrow().as_ref().map(|rc| rc.get()).unwrap_or(false));
    if ss_req {
        return true;
    }
    // Visibility-gated tree walk — a widget with in-flight animation,
    // pending hover transition, or scheduled cursor blink reports true
    // ONLY when it's actually visible on screen (hidden windows, closed
    // collapsing headers, non-selected tabs don't contribute).
    //
    // Includes the legacy thread-local `wants_tick` as a transitional
    // fallback for widgets that still call `crate::animation::request_tick`
    // instead of overriding `Widget::needs_paint`.
    let want = DEMO_APP.with(|c| {
        c.borrow()
            .as_ref()
            .map(|a| a.wants_animation_tick())
            .unwrap_or(false)
    });
    if want {
        return true;
    }
    false
}

fn mark_dirty() {
    NEEDS_REPAINT.with(|c| c.set(true));
}

// ---------------------------------------------------------------------------
// Key parsing
// ---------------------------------------------------------------------------

// DOM KeyboardEvent key-string → `Key` parser now lives in
// `agg_gui::web_adapter::key` so web hosts don't re-implement the
// same mapping.  Kept as a thin pass-through for clarity at call
// sites within this file.
fn parse_js_key(key: &str) -> Option<Key> {
    agg_gui::web_adapter::key(key)
}
