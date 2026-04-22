//! Native GL demo for agg-gui.
//!
//! Renders via `GlGfxCtx` (tess2 → GL vertex buffers), matching the WASM path.
//!
//! # Platform-split policy (kept identical across `demo-native`, `demo-wasm`, `demo-gl`)
//!
//! This crate is a **platform shell only** — it wires up the OS window
//! (winit/glutin), the event loop, the device-scale source, and disk
//! I/O for state persistence.  It contains **no demo content**: every
//! widget tree, layout, and GL renderer the user sees is shared.
//!
//! - **Widget / layout code** → `demo-ui`
//! - **GL renderers (shaders, geometry, draw calls)** → `demo-gl`
//!   (e.g. `demo_gl::GlCubeWidget`, the 3D Animation widget below)
//! - **Platform shell (OS window, event loop, persistence backend)** →
//!   here (`demo-native`) and `demo-wasm`
//!
//! If you find yourself adding a widget, shader, or piece of demo
//! content in this file — stop and put it in `demo-ui` or `demo-gl`
//! instead.  Local native testing is only meaningful as a proxy for
//! the deployed WASM build when both targets share the same compiled
//! demo content; duplicating into a platform crate breaks that
//! contract.

// 3-D Animation widget lives in `demo-gl` (shared with `demo-wasm`)
// — this keeps demo content identical between native and browser
// builds and ensures local testing exercises the same compiled code
// the deployed WASM bundle runs.  No platform-specific GL renderer
// here; the platform shell only wires up the OS window + event loop.
use demo_gl::{GlCubeWidget, CUBE_SCREEN_RECT};

use std::cell::RefCell;
use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{App, Font, Modifiers, Rect};
use agg_gui::winit_adapter;

use demo_gl::{GlGfxCtx, begin_frame, render_app_frame};

use glow::HasContext;
use glutin::config::ConfigTemplateBuilder;
use glutin::context::{ContextApi, ContextAttributesBuilder, Version};
use glutin::display::GetGlDisplay;
use glutin::prelude::*;
use glutin::surface::{GlSurface, SurfaceAttributesBuilder, WindowSurface};
use glutin_winit::DisplayBuilder;
use raw_window_handle::HasWindowHandle;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, Event, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::keyboard::{Key as WinitKey, NamedKey};
use winit::window::{Fullscreen, WindowAttributes};

const FONT_BYTES:  &[u8] = include_bytes!("../../demo/assets/CascadiaCode.ttf");
const FA_BYTES:    &[u8] = include_bytes!("../../demo/assets/fa.ttf");
const EMOJI_BYTES: &[u8] = include_bytes!("../../demo/assets/NotoEmoji-Regular.ttf");

// ---------------------------------------------------------------------------
// State persistence helpers
// ---------------------------------------------------------------------------

fn state_file_path() -> std::path::PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(".agg-gui-demo-state")))
        .unwrap_or_else(|| std::path::PathBuf::from(".agg-gui-demo-state"))
}

fn load_saved_state() -> Option<demo_ui::SavedState> {
    let path = state_file_path();
    let s = std::fs::read_to_string(&path).ok()?;
    demo_ui::SavedState::deserialize(&s)
}

/// Build the serialized form of the current state, substituting the
/// last-known windowed size when the window is currently fullscreen or
/// maximized (its inner_size is the monitor / maximized rect, which isn't
/// what we want to restore on the next launch).
fn serialize_state(
    accessor: &demo_ui::StateAccessor,
    last_windowed: (u32, u32),
) -> String {
    let mut state = accessor.current_state();
    if state.window_fullscreen || state.window_maximized {
        state.window_w = Some(last_windowed.0);
        state.window_h = Some(last_windowed.1);
    }
    state.serialize()
}

fn save_state_to_disk(text: &str) {
    let path = state_file_path();
    let _ = std::fs::write(&path, text);
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

fn main() {
    let event_loop = EventLoop::new().expect("EventLoop::new");

    // Pull saved window size / fullscreen out of the state file BEFORE building
    // the window so we can apply it as initial attributes.  Full UI state is
    // reloaded later (once fonts + GL context exist).
    let initial_state = load_saved_state();
    let (start_w, start_h) = match initial_state.as_ref() {
        Some(s) => (
            s.window_w.unwrap_or(1280),
            s.window_h.unwrap_or(720),
        ),
        None => (1280, 720),
    };
    let start_fullscreen = initial_state.as_ref()
        .map(|s| s.window_fullscreen)
        .unwrap_or(false);

    let start_maximized = initial_state.as_ref()
        .map(|s| s.window_maximized)
        .unwrap_or(false);

    // Create the window HIDDEN.  We want to finish our GL setup, apply any
    // pending maximize / fullscreen transition, and render the first real
    // frame before the user ever sees the window — otherwise Windows
    // briefly paints the OS-default white background plus a black margin
    // around the not-yet-resized GL surface.
    let mut window_attributes = WindowAttributes::default()
        .with_title("agg-gui — Demo (GL)")
        .with_inner_size(LogicalSize::new(start_w, start_h))
        .with_maximized(start_maximized)
        .with_visible(false);
    if start_fullscreen {
        window_attributes = window_attributes
            .with_fullscreen(Some(Fullscreen::Borderless(None)));
    }

    // Request 4× MSAA — tess2 produces aliased triangle edges, and without
    // multisampling the AGG-stroke outlines (rounded rects, shape strokes)
    // show staircase artefacts.  The GL driver picks the best config from
    // those matching this template.
    let template = ConfigTemplateBuilder::new()
        .with_alpha_size(0)
        .with_multisampling(4);
    let display_builder =
        DisplayBuilder::new().with_window_attributes(Some(window_attributes));

    let (window, gl_config) = display_builder
        .build(&event_loop, template, |configs| {
            // Pick the MAX-samples config — MSAA is what provides AA for the
            // direct-GL tessellated-glyph path (wrapped text paragraphs that
            // bypass the Label backbuffer cache).  MSAA samples live at
            // sub-pixel offsets within each pixel, so pixel-aligned integer
            // triangle edges still produce 0 % / 100 % coverage (no fringe)
            // as long as the CTM is integer — which `paint_subtree`'s
            // enforce-integer-bounds snap guarantees on widgets that opt in.
            configs
                .reduce(|a, b| if b.num_samples() > a.num_samples() { b } else { a })
                .expect("no suitable GL config")
        })
        .expect("DisplayBuilder::build");

    let window = window.expect("window");
    // Belt-and-suspenders — some platforms don't fully honour the initial
    // `with_fullscreen` / `with_maximized` attribute, so re-apply both after
    // the window is live.  Safe no-ops when they're already in that state.
    if start_fullscreen {
        window.set_fullscreen(Some(Fullscreen::Borderless(None)));
    } else if start_maximized {
        window.set_maximized(true);
    }
    let raw_window_handle = window.window_handle().expect("window_handle").as_raw();

    let context_attributes = ContextAttributesBuilder::new()
        .with_context_api(ContextApi::OpenGl(Some(Version::new(3, 3))))
        .build(Some(raw_window_handle));

    let gl_display = gl_config.display();
    let not_current_gl_context = unsafe {
        gl_display
            .create_context(&gl_config, &context_attributes)
            .expect("create_context")
    };

    let size = window.inner_size();
    let surface_attributes = SurfaceAttributesBuilder::<WindowSurface>::new().build(
        raw_window_handle,
        NonZeroU32::new(size.width.max(1)).unwrap(),
        NonZeroU32::new(size.height.max(1)).unwrap(),
    );

    let gl_surface = unsafe {
        gl_display
            .create_window_surface(&gl_config, &surface_attributes)
            .expect("create_window_surface")
    };

    let gl_context = not_current_gl_context
        .make_current(&gl_surface)
        .expect("make_current");

    // Wrap in Rc so GlGfxCtx can share the context.
    let gl = Rc::new(unsafe {
        glow::Context::from_loader_function_cstr(|s| gl_display.get_proc_address(s))
    });

    // Fallback chain: CascadiaCode → Font Awesome 4 (PUA icons) → NotoEmoji (emoji)
    let emoji_font = Font::from_slice(EMOJI_BYTES).expect("parse NotoEmoji-Regular.ttf");
    let fa_font    = Font::from_slice(FA_BYTES).expect("parse fa.ttf")
        .with_fallback(Arc::new(emoji_font));
    let font = Arc::new(
        Font::from_slice(FONT_BYTES).expect("parse CascadiaCode.ttf")
            .with_fallback(Arc::new(fa_font))
    );

    let init_w = size.width.max(1) as f32;
    let init_h = size.height.max(1) as f32;
    let mut gl_ctx = unsafe { GlGfxCtx::new(Rc::clone(&gl), init_w, init_h) };

    // Publish the OS device scale BEFORE `build_demo_ui` so first-run
    // defaults (LCD subpixel + baseline snapping) can consult it — both
    // are only useful on standard-DPI screens.  HiDPI displays already
    // have pixels small enough that LCD subpixel adds chromatic noise
    // for no real sharpness gain, and baseline snapping costs subpixel
    // positioning that HiDPI can otherwise express cleanly.
    agg_gui::set_device_scale(window.scale_factor());

    let (mut app, handles) = demo_ui::build_demo_ui(
        Arc::clone(&font),
        Box::new(GlCubeWidget::new()),
        "OpenGL 3.3",
        "native GL (glutin/winit)",
        initial_state,
    );
    let show_inspector     = Rc::clone(&handles.show_inspector);
    let inspector_nodes    = Rc::clone(&handles.inspector_nodes);
    let hovered_bounds     = Rc::clone(&handles.hovered_bounds);
    // `cube_visible` used to drive the ControlFlow decision; now the 3-D
    // cube's `Widget::needs_paint` returns true whenever it's visited by
    // the tree walk, which automatically skips when its Window is closed.
    let _cube_visible      = Rc::clone(&handles.cube_visible);
    let screen_size        = Rc::clone(&handles.screen_size);
    let frame_history      = Rc::clone(&handles.frame_history);
    let window_fullscreen  = Rc::clone(&handles.window_fullscreen);
    let window_maximized   = Rc::clone(&handles.window_maximized);
    let screenshot_request      = Rc::clone(&handles.screenshot_request);
    let handles_screenshot_image = Rc::clone(&handles.screenshot_image);
    let screenshot_capturing    = Rc::clone(&handles.screenshot_capturing);
    let state_accessor          = handles.state;
    #[allow(unused_assignments, unused_mut)]
    let mut screenshot_counter: u32 = 0;
    // Auto-save machinery — every AboutToWait tick, hash the current state
    // and save when it differs AND no mouse button is held down (so we don't
    // thrash on disk mid-drag or mid-resize).
    let mut auto_save = agg_gui::persistence::AutoSave::new();
    let mut mouse_buttons_down: u32 = 0;

    let mut cursor_x    = 0.0f64;
    let mut cursor_y    = 0.0f64;
    let mut last_frame_ms = 0.0f64;
    let mut win_w       = size.width.max(1);
    let mut win_h       = size.height.max(1);
    // Last size seen while the window was NOT fullscreen — what we persist
    // across restarts.  Seeded with the saved windowed size (or the default).
    let mut last_windowed_w: u32 = start_w;
    let mut last_windowed_h: u32 = start_h;
    // Tracks the live modifier state from ModifiersChanged events.
    let mut current_mods = Modifiers::default();

    // The window was created hidden.  Re-query its inner size — on most
    // platforms winit has by now applied any `with_maximized` /
    // `with_fullscreen` attribute AND our post-creation `set_fullscreen` /
    // `set_maximized` call, so this is the true canvas size of the first
    // visible frame.  Resize the GL surface to match and render one frame
    // BEFORE showing the window so the user never sees the OS-default
    // white-flash + black-border-around-small-GL-surface.
    let init_size = window.inner_size();
    if init_size.width > 0 && init_size.height > 0 {
        gl_surface.resize(
            &gl_context,
            NonZeroU32::new(init_size.width.max(1)).unwrap(),
            NonZeroU32::new(init_size.height.max(1)).unwrap(),
        );
        win_w = init_size.width;
        win_h = init_size.height;
    }
    screen_size.set((win_w, win_h));

    // (Device scale was already published above, before `build_demo_ui`,
    // so first-run defaults could consult it.  winit emits
    // ScaleFactorChanged when the window moves between monitors; we
    // update the global at that event below.)

    // Clear to the theme background first so any transparent regions in
    // the first paint (e.g. between widgets) are already theme-coloured.
    unsafe {
        let bg = agg_gui::current_visuals().bg_color;
        gl.clear_color(bg.r, bg.g, bg.b, 1.0);
        gl.clear(glow::COLOR_BUFFER_BIT);
    }
    // Full initial paint at the correct canvas size.  With `clamp_to_canvas`
    // removed from `Window::layout`, this is safe even if the reported size
    // hasn't yet caught up with the final maximize transition — saved
    // window positions aren't mutated during layout.
    render_frame(&mut app, &mut gl_ctx, &gl, win_w, win_h, last_frame_ms,
                 show_inspector.get(), &inspector_nodes, &hovered_bounds);
    let _ = gl_surface.swap_buffers(&gl_context);

    // Finally, reveal the window — its first visible frame is our content.
    window.set_visible(true);

    #[allow(deprecated)]
    event_loop
        .run(|event, elwt| {
            match event {
                Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => {
                    let s = serialize_state(&state_accessor,
                        (last_windowed_w, last_windowed_h));
                    save_state_to_disk(&s);
                    elwt.exit();
                }
                Event::WindowEvent {
                    event: WindowEvent::ScaleFactorChanged { scale_factor, .. }, ..
                } => {
                    // Window moved to a different-DPI monitor.  Update our
                    // scale factor so the next layout/paint/input pass uses
                    // the new value.
                    agg_gui::set_device_scale(scale_factor);
                }
                Event::WindowEvent {
                    event: WindowEvent::Resized(new_size), ..
                } => {
                    if new_size.width > 0 && new_size.height > 0 {
                        gl_surface.resize(
                            &gl_context,
                            NonZeroU32::new(new_size.width).unwrap(),
                            NonZeroU32::new(new_size.height).unwrap(),
                        );
                        win_w = new_size.width;
                        win_h = new_size.height;
                        screen_size.set((win_w, win_h));
                        // Resize is the reliable signal for fullscreen AND
                        // maximize/restore transitions — update both flags.
                        let is_full = window.fullscreen().is_some();
                        let is_max  = window.is_maximized();
                        window_fullscreen.set(is_full);
                        window_maximized.set(is_max);
                        if !is_full && !is_max {
                            last_windowed_w = win_w;
                            last_windowed_h = win_h;
                        }
                        // Render immediately so content tracks the drag handle.
                        render_frame(&mut app, &mut gl_ctx, &gl,
                                     win_w, win_h, last_frame_ms,
                                     show_inspector.get(),
                                     &inspector_nodes, &hovered_bounds);
                        gl_surface.swap_buffers(&gl_context).expect("swap_buffers");
                    }
                }
                Event::WindowEvent {
                    event: WindowEvent::CursorMoved { position, .. }, ..
                } => {
                    cursor_x = position.x;
                    cursor_y = position.y;
                    app.on_mouse_move(cursor_x, cursor_y);
                    winit_adapter::apply_cursor(&window, agg_gui::current_cursor_icon());
                }
                Event::WindowEvent {
                    event: WindowEvent::CursorLeft { .. }, ..
                } => {
                    app.on_mouse_leave();
                }
                Event::WindowEvent {
                    event: WindowEvent::ModifiersChanged(mods_state), ..
                } => {
                    current_mods = winit_adapter::modifiers(mods_state.state());
                }
                Event::WindowEvent {
                    event: WindowEvent::MouseInput { state, button, .. }, ..
                } => {
                    let btn = winit_adapter::mouse_button(button);
                    match state {
                        ElementState::Pressed  => {
                            mouse_buttons_down = mouse_buttons_down.saturating_add(1);
                            app.on_mouse_down(cursor_x, cursor_y, btn, current_mods);
                        }
                        ElementState::Released => {
                            mouse_buttons_down = mouse_buttons_down.saturating_sub(1);
                            app.on_mouse_up(cursor_x, cursor_y, btn, current_mods);
                        }
                    }
                }
                Event::WindowEvent {
                    event: WindowEvent::KeyboardInput { event: key_event, .. }, ..
                } => {
                    if key_event.state == ElementState::Pressed {
                        // F11 toggles borderless fullscreen at the OS level.
                        // We also flip the tracked fullscreen cell eagerly so
                        // the saved-state snapshot is right even if the
                        // subsequent Resized event hasn't landed yet.
                        if matches!(
                            key_event.logical_key,
                            WinitKey::Named(NamedKey::F11)
                        ) {
                            let now_full = window.fullscreen().is_some();
                            if now_full {
                                window.set_fullscreen(None);
                                window_fullscreen.set(false);
                            } else {
                                window.set_fullscreen(Some(Fullscreen::Borderless(None)));
                                window_fullscreen.set(true);
                            }
                            return;
                        }
                        // F9 — request a screenshot of the NEXT rendered
                        // frame.  The main loop polls this cell and captures
                        // after rendering.
                        if matches!(
                            key_event.logical_key,
                            WinitKey::Named(NamedKey::F9)
                        ) {
                            screenshot_request.set(true);
                            return;
                        }
                        if let Some(key) = winit_adapter::key(&key_event.logical_key) {
                            app.on_key_down(key, current_mods);
                        }
                    }
                }
                Event::WindowEvent {
                    event: WindowEvent::MouseWheel { delta, .. }, ..
                } => {
                    // Winit: LineDelta y > 0 = wheel up = scroll UP = negative delta.
                    // Treat shift+wheel as horizontal (common mouse-with-only-
                    // vertical-wheel convention).
                    let (mut dx, mut dy) = match delta {
                        winit::event::MouseScrollDelta::LineDelta(x, y) =>
                            (-(x as f64), -(y as f64)),
                        winit::event::MouseScrollDelta::PixelDelta(d) =>
                            (d.x / 40.0, d.y / 40.0),
                    };
                    if current_mods.shift && dx == 0.0 {
                        dx = dy;
                        dy = 0.0;
                    }
                    app.on_mouse_wheel_xy(cursor_x, cursor_y, dx, dy);
                }
                Event::AboutToWait => {
                    // Decide whether anything has actually changed since the
                    // last paint.  A plain mouse-move that didn't flip any
                    // widget's hover state, a key that no focused widget
                    // consumed, etc., leave all the signals clear — in that
                    // case we do NOT render.  Only paint when:
                    //   - A widget set the thread-local tick flag from its
                    //     event handler (hover change, press, drag, etc.).
                    //   - The visible widget tree reports pending work via
                    //     `needs_paint` — widgets like TextField (cursor
                    //     blink) compare their current phase to the one
                    //     last painted and report dirty when they diverge.
                    //   - A screenshot was requested (button / F9).
                    //
                    // Scheduled wakes (ControlFlow::WaitUntil below) just
                    // bring the loop back so `needs_paint` can be queried
                    // again; there is no host-side deadline bookkeeping.
                    let want_render = app.wants_animation_tick()
                        || screenshot_request.get();

                    if want_render {
                        let t0 = std::time::Instant::now();

                        screen_size.set((win_w, win_h));

                        // Shared screenshot orchestration: on a capture
                        // frame it double-renders (pass 1 hides the preview
                        // so captured pixels don't nest, pass 2 reveals the
                        // fresh image) via the two closures we supply.
                        let show_insp = show_inspector.get();
                        agg_gui::screenshot::run_frame_with_capture(
                            &screenshot_request,
                            &screenshot_capturing,
                            &handles_screenshot_image,
                            &mut gl_ctx,
                            |gc| render_frame(&mut app, gc, &gl,
                                              win_w, win_h, last_frame_ms,
                                              show_insp, &inspector_nodes,
                                              &hovered_bounds),
                            |gc| gc.read_screenshot(),
                        );
                        if screenshot_request.get() == false
                            && handles_screenshot_image.borrow().is_some()
                        {
                            // Counter is bumped on every frame that actually
                            // consumed a request — tracked for parity with
                            // pre-refactor behaviour.
                            screenshot_counter = screenshot_counter.wrapping_add(1);
                        }

                        gl_surface.swap_buffers(&gl_context).expect("swap_buffers");

                        last_frame_ms = t0.elapsed().as_secs_f64() * 1000.0;
                        frame_history.borrow_mut().push(last_frame_ms as f32);
                    }

                    // Visibility-gated ControlFlow for the NEXT wake-up.
                    // `wants_animation_tick` folds the tree walk (visible
                    // widgets only) with the legacy thread-local flag.
                    // Scheduled wakes come from the tree walk — a text
                    // field's cursor blink contributes a deadline ONLY when
                    // its enclosing window/tab/header is actually showing
                    // it.  With nothing dirty and no deadline, `Wait` means
                    // the loop idles until the next OS input event.
                    let want_next = app.wants_animation_tick()
                        || screenshot_request.get();
                    elwt.set_control_flow(if want_next {
                        ControlFlow::Poll
                    } else if let Some(t) = app.next_paint_deadline() {
                        ControlFlow::WaitUntil(t)
                    } else {
                        ControlFlow::Wait
                    });

                    // Auto-save via the shared `AutoSave` helper — same
                    // policy drives both native and wasm: diff current
                    // serialized state against last-saved, write only
                    // when they differ, and only while no mouse button
                    // is held (so drag / resize don't thrash disk).
                    auto_save.tick(
                        mouse_buttons_down == 0,
                        || serialize_state(&state_accessor,
                                           (last_windowed_w, last_windowed_h)),
                        |s| save_state_to_disk(s),
                    );
                }
                _ => {}
            }
        })
        .expect("event_loop.run");
}

// ---------------------------------------------------------------------------
// render_frame — GL path (no AGG framebuffer)
// ---------------------------------------------------------------------------

fn render_frame(
    app:             &mut App,
    gl_ctx:          &mut GlGfxCtx,
    gl:              &glow::Context,
    w:               u32,
    h:               u32,
    frame_ms:        f64,
    show_inspector:  bool,
    inspector_nodes: &Rc<RefCell<Vec<agg_gui::InspectorNode>>>,
    hovered_bounds:  &Rc<RefCell<Option<Rect>>>,
) {
    begin_frame(gl, w, h);
    CUBE_SCREEN_RECT.with(|r| r.set(Rect::default()));
    render_app_frame(gl_ctx, app, w, h, frame_ms,
                     show_inspector, inspector_nodes, hovered_bounds);
}

// All input (key/mouse-button/modifier) and cursor-icon mapping now
// lives in `agg_gui::winit_adapter` — imported above via
// `use agg_gui::winit_adapter`.
