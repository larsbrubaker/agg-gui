//! WASM demo crate for agg-gui.
//!
//! The widget tree is rendered via `GlGfxCtx` (tess2 tessellation → WebGL2
//! draw calls) directly to the canvas.  A rotating 3D cube is drawn on top
//! each frame by `GlState`.
//!
//! UI is shared with the native target via `demo-ui`.
//!
//! WASM exports:
//! - `render(width, height)` — full-frame render (void; GL writes to canvas)
//! - `on_mouse_move/down/up/wheel/leave` — mouse events
//! - `on_key_down` — keyboard events

mod gl_resources;

use demo_gl::GlGfxCtx;
use gl_resources::{GlCubeWidget, GlState, CUBE_SCREEN_RECT};

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use agg_gui::{App, Font, InspectorNode, Key, Modifiers, MouseButton, Size, Widget};

// Embed the font at compile time.
const FONT_BYTES: &[u8] = include_bytes!("../../demo/assets/CascadiaCode.ttf");

fn make_font() -> Arc<Font> {
    Arc::new(Font::from_slice(FONT_BYTES).expect("embedded font is valid"))
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
}

/// Initialise panic hook so Rust panics appear in the browser console.
#[wasm_bindgen(start)]
pub fn wasm_start() {
    console_error_panic_hook::set_once();
}

fn ensure_demo_app() {
    DEMO_APP.with(|cell| {
        if cell.borrow().is_none() {
            let font = make_font();
            let (app, show_inspector, inspector_nodes, _hovered_bounds) =
                demo_ui::build_demo_ui(font, Box::new(GlCubeWidget::new()));
            SHOW_INSPECTOR.with(|c| *c.borrow_mut() = Some(Rc::clone(&show_inspector)));
            INSPECTOR_NODES.with(|c| *c.borrow_mut() = Some(Rc::clone(&inspector_nodes)));
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
// WASM event exports
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
