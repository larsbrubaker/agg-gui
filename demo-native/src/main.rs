//! Native GL demo for agg-gui.
//!
//! Renders via `GlGfxCtx` (tess2 → GL vertex buffers), matching the WASM path.
//! The UI is shared with the WASM target via `demo-ui`.

mod cube_widget;
use cube_widget::{GlCubeWidget, CUBE_SCREEN_RECT};

use std::cell::RefCell;
use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{App, CursorIcon, Font, Key as AggKey, Modifiers,
              MouseButton as AggMouseButton, Rect};
use winit::window::CursorIcon as WinitCursor;

use demo_gl::{GlGfxCtx, begin_frame, sync_inspector, render_app_frame};

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

    let template = ConfigTemplateBuilder::new().with_alpha_size(0);
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
    let cube_visible       = Rc::clone(&handles.cube_visible);
    let screen_size        = Rc::clone(&handles.screen_size);
    let frame_history      = Rc::clone(&handles.frame_history);
    let window_fullscreen  = Rc::clone(&handles.window_fullscreen);
    let window_maximized   = Rc::clone(&handles.window_maximized);
    let screenshot_request      = Rc::clone(&handles.screenshot_request);
    let handles_screenshot_image = Rc::clone(&handles.screenshot_image);
    let state_accessor          = handles.state;
    #[allow(unused_assignments, unused_mut)]
    let mut screenshot_counter: u32 = 0;
    // Auto-save machinery — every AboutToWait tick, hash the current state
    // and save when it differs AND no mouse button is held down (so we don't
    // thrash on disk mid-drag or mid-resize).
    let mut last_saved_state: String = String::new();
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

    // Publish the OS-reported device scale factor so the widget tree paints
    // at physical pixel density — tiny text on 2×/3× HiDPI monitors and
    // phone-browser emulation otherwise.  winit emits ScaleFactorChanged
    // when the window moves to a different monitor; we update it there too.
    agg_gui::set_device_scale(window.scale_factor());

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
    sync_inspector(&app, show_inspector.get(), &inspector_nodes, &hovered_bounds);
    render_frame(&mut app, &mut gl_ctx, &gl, win_w, win_h, last_frame_ms, &hovered_bounds);
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
                        sync_inspector(&app, show_inspector.get(),
                                       &inspector_nodes, &hovered_bounds);
                        render_frame(&mut app, &mut gl_ctx, &gl,
                                     win_w, win_h, last_frame_ms, &hovered_bounds);
                        gl_surface.swap_buffers(&gl_context).expect("swap_buffers");
                    }
                }
                Event::WindowEvent {
                    event: WindowEvent::CursorMoved { position, .. }, ..
                } => {
                    cursor_x = position.x;
                    cursor_y = position.y;
                    app.on_mouse_move(cursor_x, cursor_y);
                    apply_cursor(&window, agg_gui::current_cursor_icon());
                }
                Event::WindowEvent {
                    event: WindowEvent::CursorLeft { .. }, ..
                } => {
                    app.on_mouse_leave();
                }
                Event::WindowEvent {
                    event: WindowEvent::ModifiersChanged(mods_state), ..
                } => {
                    let s = mods_state.state();
                    current_mods = Modifiers {
                        shift: s.shift_key(),
                        ctrl:  s.control_key(),
                        alt:   s.alt_key(),
                        // Winit's `super_key` is the platform "command" key —
                        // Cmd on macOS, Windows key on Windows, Super on X11.
                        meta:  s.super_key(),
                    };
                }
                Event::WindowEvent {
                    event: WindowEvent::MouseInput { state, button, .. }, ..
                } => {
                    let btn = map_mouse_button(&button);
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
                        // F10 — dump widget tree + bounds as JSON next to the
                        // executable.  Overwrites on each press so successive
                        // dumps can be diffed to track layout changes.
                        if matches!(
                            key_event.logical_key,
                            WinitKey::Named(NamedKey::F10)
                        ) {
                            let path = state_file_path()
                                .parent()
                                .map(|p| p.join("widget-tree.json"))
                                .unwrap_or_else(|| std::path::PathBuf::from("widget-tree.json"));
                            let json = app.dump_tree_json();
                            match std::fs::write(&path, &json) {
                                Ok(_)  => eprintln!("dumped widget tree → {}", path.display()),
                                Err(e) => eprintln!("failed to write widget tree: {e}"),
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
                        if let Some(key) = map_key(&key_event.logical_key) {
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
                    let t0 = std::time::Instant::now();

                    // Sync inspector node snapshot before painting.
                    sync_inspector(&app, show_inspector.get(),
                                   &inspector_nodes, &hovered_bounds);

                    screen_size.set((win_w, win_h));
                    render_frame(&mut app, &mut gl_ctx, &gl,
                                 win_w, win_h, last_frame_ms, &hovered_bounds);

                    // Poll while the cube animates OR any widget is running a
                    // hover/transition animation; WaitUntil(500ms) when a text
                    // field has focus so the cursor blink fires; Wait otherwise.
                    elwt.set_control_flow(if cube_visible.get() || app.wants_animation_tick() {
                        ControlFlow::Poll
                    } else if app.has_focus() {
                        ControlFlow::WaitUntil(
                            std::time::Instant::now()
                                + std::time::Duration::from_millis(500),
                        )
                    } else {
                        ControlFlow::Wait
                    });

                    // Satisfy any pending screenshot request BEFORE buffer swap
                    // while the back buffer still holds this frame's pixels.
                    if screenshot_request.get() {
                        let (rgba, w, h) = gl_ctx.read_screenshot();
                        *handles_screenshot_image.borrow_mut() = Some((rgba, w, h));
                        screenshot_request.set(false);
                        screenshot_counter = screenshot_counter.wrapping_add(1);
                    }

                    gl_surface.swap_buffers(&gl_context).expect("swap_buffers");

                    last_frame_ms = t0.elapsed().as_secs_f64() * 1000.0;
                    frame_history.borrow_mut().push(last_frame_ms as f32);

                    // Auto-save when state changed AND no mouse button is
                    // held down.  Covers window open/close clicks, drag /
                    // resize releases, fullscreen / maximize transitions,
                    // and hotkey-driven toggles — but never saves mid-drag.
                    if mouse_buttons_down == 0 {
                        let s = serialize_state(&state_accessor,
                            (last_windowed_w, last_windowed_h));
                        if s != last_saved_state {
                            save_state_to_disk(&s);
                            last_saved_state = s;
                        }
                    }
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
    app:            &mut App,
    gl_ctx:         &mut GlGfxCtx,
    gl:             &glow::Context,
    w:              u32,
    h:              u32,
    frame_ms:       f64,
    hovered_bounds: &Rc<RefCell<Option<Rect>>>,
) {
    begin_frame(gl, w, h);
    CUBE_SCREEN_RECT.with(|r| r.set(Rect::default()));
    let hovered = *hovered_bounds.borrow();
    render_app_frame(gl_ctx, app, w, h, frame_ms, hovered);
}

// ---------------------------------------------------------------------------
// Input mapping helpers
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Cursor helpers
// ---------------------------------------------------------------------------

fn apply_cursor(window: &winit::window::Window, icon: CursorIcon) {
    if icon == CursorIcon::None {
        window.set_cursor_visible(false);
    } else {
        window.set_cursor_visible(true);
        window.set_cursor(agg_cursor_to_winit(icon));
    }
}

fn agg_cursor_to_winit(icon: CursorIcon) -> WinitCursor {
    match icon {
        CursorIcon::Default          => WinitCursor::Default,
        CursorIcon::None             => WinitCursor::Default, // handled above
        CursorIcon::ContextMenu      => WinitCursor::ContextMenu,
        CursorIcon::Help             => WinitCursor::Help,
        CursorIcon::PointingHand     => WinitCursor::Pointer,
        CursorIcon::Progress         => WinitCursor::Progress,
        CursorIcon::Wait             => WinitCursor::Wait,
        CursorIcon::Cell             => WinitCursor::Cell,
        CursorIcon::Crosshair        => WinitCursor::Crosshair,
        CursorIcon::Text             => WinitCursor::Text,
        CursorIcon::VerticalText     => WinitCursor::VerticalText,
        CursorIcon::Alias            => WinitCursor::Alias,
        CursorIcon::Copy             => WinitCursor::Copy,
        CursorIcon::Move             => WinitCursor::Move,
        CursorIcon::NoDrop           => WinitCursor::NoDrop,
        CursorIcon::NotAllowed       => WinitCursor::NotAllowed,
        CursorIcon::Grab             => WinitCursor::Grab,
        CursorIcon::Grabbing         => WinitCursor::Grabbing,
        CursorIcon::AllScroll        => WinitCursor::AllScroll,
        CursorIcon::ResizeHorizontal => WinitCursor::EwResize,
        CursorIcon::ResizeNeSw       => WinitCursor::NeswResize,
        CursorIcon::ResizeNwSe       => WinitCursor::NwseResize,
        CursorIcon::ResizeVertical   => WinitCursor::NsResize,
        CursorIcon::ResizeEast       => WinitCursor::EResize,
        CursorIcon::ResizeSouthEast  => WinitCursor::SeResize,
        CursorIcon::ResizeSouth      => WinitCursor::SResize,
        CursorIcon::ResizeSouthWest  => WinitCursor::SwResize,
        CursorIcon::ResizeWest       => WinitCursor::WResize,
        CursorIcon::ResizeNorthWest  => WinitCursor::NwResize,
        CursorIcon::ResizeNorth      => WinitCursor::NResize,
        CursorIcon::ResizeNorthEast  => WinitCursor::NeResize,
        CursorIcon::ResizeColumn     => WinitCursor::ColResize,
        CursorIcon::ResizeRow        => WinitCursor::RowResize,
        CursorIcon::ZoomIn           => WinitCursor::ZoomIn,
        CursorIcon::ZoomOut          => WinitCursor::ZoomOut,
    }
}

fn map_key(key: &WinitKey) -> Option<AggKey> {
    Some(match key {
        WinitKey::Named(NamedKey::ArrowUp)    => AggKey::ArrowUp,
        WinitKey::Named(NamedKey::ArrowDown)  => AggKey::ArrowDown,
        WinitKey::Named(NamedKey::ArrowLeft)  => AggKey::ArrowLeft,
        WinitKey::Named(NamedKey::ArrowRight) => AggKey::ArrowRight,
        WinitKey::Named(NamedKey::Enter)      => AggKey::Enter,
        WinitKey::Named(NamedKey::Space)      => AggKey::Char(' '),
        WinitKey::Named(NamedKey::Tab)        => AggKey::Tab,
        WinitKey::Named(NamedKey::Escape)     => AggKey::Escape,
        WinitKey::Named(NamedKey::Backspace)  => AggKey::Backspace,
        WinitKey::Named(NamedKey::Home)       => AggKey::Home,
        WinitKey::Named(NamedKey::End)        => AggKey::End,
        WinitKey::Named(NamedKey::Delete)     => AggKey::Delete,
        WinitKey::Named(NamedKey::Insert)     => AggKey::Insert,
        WinitKey::Named(NamedKey::PageUp)     => AggKey::Other("PageUp".into()),
        WinitKey::Named(NamedKey::PageDown)   => AggKey::Other("PageDown".into()),
        WinitKey::Character(s) => AggKey::Char(s.chars().next()?),
        _ => return None,
    })
}

fn map_mouse_button(b: &winit::event::MouseButton) -> AggMouseButton {
    match b {
        winit::event::MouseButton::Left   => AggMouseButton::Left,
        winit::event::MouseButton::Right  => AggMouseButton::Right,
        winit::event::MouseButton::Middle => AggMouseButton::Middle,
        winit::event::MouseButton::Other(n) => AggMouseButton::Other(*n as u8),
        _ => AggMouseButton::Other(255),
    }
}
