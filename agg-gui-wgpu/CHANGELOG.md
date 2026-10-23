# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Because the crate is pre-1.0, breaking changes are released in `0.MINOR.0` bumps.

## [Unreleased]

### Added

- Device generations: `next_device_generation()` hands out a process-wide,
  increasing `u64` that no two devices share. `Gpu::device_generation` and
  `HeadlessGpu::device_generation` report the one taken when the device was
  made (a device-loss rebuild's new `Gpu` gets a new one);
  `WgpuGfxCtx::set_device_generation` / `with_device_generation` record it on
  the context (a context with none records a fresh one of its own), and
  `WgpuGfxCtx::device_generation`, `HeadlessFrame::device_generation` and
  `WgpuCustomRenderCtx::device_generation` report it. A custom renderer keys
  its GPU state on it instead of `wgpu::Device` equality, which compares an id
  unique only within one `wgpu::Instance` (every agg-gui device has its own),
  so a rebuilt device could compare equal to the lost one.
- Frame completion times: each window frame's `end_frame` registers
  `queue.on_submitted_work_done` right after its submit and records the time
  until wgpu reports the frame done, without waiting.
  `WgpuCustomRenderCtx::frame` (and `WgpuGfxCtx::frame_number`) is the number
  of the frame being recorded; `WgpuCustomRenderCtx::last_frame_completion`
  (and `WgpuGfxCtx::last_frame_completion`) is the latest finished frame as a
  `FrameCompletion { frame, submit_to_done_ms }`: whole-window GPU time plus
  queue latency, an upper bound where wgpu reports finished work only at the
  next submit or poll (native, WebGL2). For a frame cost on GPUs without
  timestamp queries.
- `WgpuCustomRenderCtx::lost_frames`: how many frames the device has lost to
  GPU work wgpu rejected (validation errors from any pass, out-of-memory), so
  a custom render pass that keeps its finished picture between frames knows
  when a frame it recorded into never reached its texture. The count comes
  from the `UncapturedErrors` recorded with `WgpuGfxCtx::set_uncaptured_errors`
  / `with_uncaptured_errors` (also read by `WgpuGfxCtx::lost_frames`); the
  native and web shells record theirs. `UncapturedErrors` now builds for
  wasm32, and `UncapturedErrors::install_reporting` reports each error through
  the caller's own function.
- `WgpuGfxCtx::set_adapter` / `with_adapter` (and `adapter()`,
  `downlevel_capabilities()`): record the adapter the device came from.
  `WgpuCustomRenderCtx::adapter` and `WgpuCustomRenderCtx::downlevel` hand it
  and its `DownlevelCapabilities` to every custom render pass, so a 3-D view
  chooses its passes (independent blend, which WebGL2 lacks) without probing
  the device. `HeadlessFrame` records its adapter.
- `WgpuGfxCtx::texture_counts` / `SsaaFramebuffer::texture_counts` (a
  `TextureCounts` of textures made, waiting and destroyed) and
  `WgpuGfxCtx::retire_texture`, which destroys a caller's texture after the
  next frame's submit.
- `WgpuGfxCtx::begin_overlay_frame`: start a frame that paints over the
  target's current pixels (no clear is queued, and `reset` adds none), for a
  pass drawn on top of a finished frame before it is presented.
- `GpuMemoryHints` and `GpuConfig::with_memory_hints`: how a device's
  allocator reserves memory. Windows keep `Performance` by default.
- `HeadlessOptions` (`force_fallback_adapter`, `startup_budget`,
  `memory_hints`), `HeadlessGpu::shared_with_options`,
  `HeadlessGpu::create_with_options`, `HeadlessGpu::memory_hints`, and
  `HeadlessError::CreationPanicked`.
- Bounded GPU waits: `HeadlessFrame::try_render` / `try_render_app` return the
  outcome of the frame-pacing wait, `HeadlessError::GpuTimeout` and
  `HeadlessError::DeviceLost` (with wgpu's reason) report a hung or lost GPU,
  `HeadlessGpu::device_lost_reason` says why a device was lost, and
  `HeadlessTarget::with_wait_budget` / `HeadlessFrame::set_wait_budget` set the
  bound (`GPU_WAIT_BUDGET`, 30 s, by default).

### Changed

- Every texture the context replaces or lets go (evicted image and text
  textures, retained layers rebuilt at a new size, screenshot targets,
  per-frame gradient ramps and blend-mode copies, a resized `SsaaFramebuffer`'s
  attachments, a resized `HeadlessFrame`'s target) is destroyed explicitly
  after the submit that last used it, instead of waiting for the browser's
  garbage collector, so a canvas resized every frame no longer runs a phone
  out of GPU memory. Transient layers are pooled by size and reused frame to
  frame, so steady frames make no layer textures.
- Headless frame pacing, headless read-back, `WgpuGfxCtx::read_screenshot` and
  the blocking capture read-back wait at most their budget for the GPU instead
  of forever, so a GPU that hangs or loses its device fails the call (a
  logged warning from `HeadlessFrame::render`, an error from the rest) rather
  than parking the thread.

- **Breaking for consumers that set `default-features = false`:** wgpu's WebGL2 backend is the
  new `webgl` feature (on by default; it only affects wasm32), so such a consumer that renders
  through WebGL2 must now enable `webgl` (or enable wgpu's `webgl` itself). A WebGPU-only browser
  app leaves it off, which keeps wgpu's GLES backend, its GLSL shader writer and glow out of the
  wasm (about 2.4 MB of code).
- The headless device opens with `GpuMemoryHints::MemoryUsage` by default:
  on Vulkan (GTX 1070) the device alone reserved 198 MiB with `Performance`
  and 18 MiB with `MemoryUsage`, paid by every process of a per-test runner.
- A shared headless device that times out is no longer a failure for the rest
  of the process: each call waits until its own budget after the creation
  began, the request keeps running, and once it returns every later call
  gets the device. A hung request still times out every call, and a call past
  the deadline is told at once. A loaded machine (parallel builds) needed more
  than the 15 s default for an ordinary device request, which then failed
  every GPU test in the binary.
- A panic inside the shared headless device request is cached as
  `HeadlessError::CreationPanicked` for every caller, instead of unwinding
  into the first caller and being retried by the next call: the shared device
  gives every caller the same answer.

- `HeadlessFrame::render` (and `render_app`) keeps at most two frames on the
  GPU, as a window's double-buffered swap chain does: once a frame is
  submitted it waits until the frame before it has finished, and it returns
  the `wgpu::SubmissionIndex` that ends the frame. Unpaced, a headless harness
  ran as far ahead of the GPU as the backend allowed (Metal: 4096 outstanding
  command buffers), and several test processes sharing the GPU spent most of
  their time blocked in the driver (MatterCAD's headless UI tests: about 90%
  of the time in `-[MTLCommandQueue commandBuffer]`).

### Fixed

- LCD and gray text masks (and LCD backbuffers) larger than the GPU's
  `max_texture_dimension_2d` are drawn in tiles instead of failing validation:
  a single line a few thousand characters long no longer panics on wgpu's
  default error handler or loses the frame. Text that fits is one tile, as
  before.

- Images larger than the GPU's limit are shrunk to fit before upload and still
  cover their rectangle, drawing blurrier than the source.

- Layers are held to the GPU's limit: content past it is not drawn, instead of
  the layer failing validation.

- A `reset` after `begin_frame` (as the shells' `default_paint` makes) keeps
  the frame's clear instead of dropping it with the queued commands, so a
  window frame starts from the theme's background rather than the surface's
  old contents (transparent black in a browser, earlier frames' drawing under
  anything animated). `DrawCtx::clear` before anything is drawn replaces the
  frame's clear, so a paint that starts by clearing still gets its colour.
- The `lcb_flatten` pass (a two-plane LCD backbuffer blitted inside a transparent layer) emits the
  ink's own straight colour at the weighted alpha instead of the per-channel premultiplied colour
  plane, so non-black text collapses to one colour: a 0.2-gray label showed a channel spread of 29
  inside the layer, black showed none.

### Added

- `choose_surface_format`, the swap-chain format rule both shells share
  (agg-sharp `WebGpuRenderDevice.PickSurfaceFormat`): Bgra8Unorm whenever it
  is offered, else the first non-sRGB format, else the surface's first
  preference. `Gpu::new` now uses it, so a surface listing `Rgba8Unorm` before
  `Bgra8Unorm` configures `Bgra8Unorm` (the headless frame's format) where it
  used to take `Rgba8Unorm`.
- `UncapturedErrors` (native): a device's uncaptured-error handler that logs
  and counts wgpu errors instead of wgpu's default panic. `Gpu::new` installs
  it on the window's device (`Gpu::uncaptured_errors`), so a submission wgpu
  rejects — a custom renderer recording an invalid copy, say — costs that
  frame and the next frame starts whole, where it used to panic the app
  (agg-sharp `PresentFailureContainmentTests`). `HeadlessGpu` keeps wgpu's
  panicking default so tests fail loudly.

- Software rendering on request (native only), agg-sharp's
  `UseSoftwareAdapter`: `GpuConfig::force_fallback_adapter` /
  `with_force_fallback_adapter` makes `Gpu::new` demand wgpu's fallback
  adapter (WARP on Windows, lavapipe/llvmpipe on Linux; macOS has none) and
  run the same shaders on it, inside the same start-up budget. With no
  fallback adapter it returns `GpuInitError::NoFallbackAdapter`, whose text is
  written for the user. `HeadlessGpu::shared_with(force_fallback_adapter)`
  takes the same option (`HeadlessError::NoFallbackAdapter` when the platform
  has none). `Gpu::adapter` and `HeadlessGpu::adapter` report the adapter
  chosen as an `AdapterSummary` (name, backend, `is_fallback`), and
  `Gpu::adapter_info` gives wgpu's full `AdapterInfo`; `Gpu::new` logs it at
  `info`.

- `headless` (native only): paint a whole widget tree on the GPU with no
  window and read it back. `HeadlessGpu::shared()` returns one device and
  queue for the process, requested with `Gpu::new`'s backends, features,
  limits and start-up budget, or a `HeadlessError` saying why there is none
  (`NoAdapter` on a machine without a GPU). `HeadlessTarget` is an offscreen
  texture in `HEADLESS_FORMAT` (`Bgra8Unorm`, the shells' frame format) or any
  8-bit RGBA/BGRA format, with `read_rgba` (RGBA8, top row first, no row
  padding) and `read_framebuffer` (an `agg_gui::Framebuffer`, bottom row
  first). `HeadlessFrame` pairs a `WgpuGfxCtx` with a target and runs a
  shell's frame around a paint closure (`render`) or an `App`
  (`render_app`), so blend modes, retained layers and `WgpuCustomRender`
  passes behave as on a window. The crate's own GPU tests now run on this
  device and target.

- `DrawCtx::set_blend_mode` works on the GPU: solid fills, solid strokes and
  grayscale text draw through every SVG compositing operator (`CompOp`) and
  match the software `GfxCtx` pixel for pixel at the same cover (within 2 per
  channel), as agg-sharp's `GpuCompOp` does for `Graphics2DGpu`. Each draw
  renders its cover into a transparent layer, then a composite pass reads a
  copy of the destination and applies `agg_rust::comp_op`'s formulas once per
  pixel; `Dst` draws nothing. Gradient fills, images and LCD text ignore the
  mode, as they do in software. The destination copy needs `COPY_SRC`:
  layers now have it, and when the surface handed over with
  `set_surface_texture` lacks it (or none was handed over), a frame that uses a
  blend mode renders into a readable proxy texture and blits it onto the
  surface. The mode is saved and restored with `save`/`restore` and carries
  into layers.

- `gpu_budget` (agg-sharp `GpuStartup` / `GpuTeardown`):
  `create_within_budget` and `drain_within_budget` run a device build or a GPU
  drain on their own thread within a wall-clock budget, with
  `GPU_STARTUP_BUDGET` (15 s) and `GPU_TEARDOWN_BUDGET` (5 s). `Gpu::new` runs
  its adapter and device requests within `GpuConfig::startup_budget` and
  returns the new `GpuInitError::StartupTimedOut` when it expires;
  `Gpu::release_within_budget` drains before releasing and leaks the device
  rather than wait past the budget. A device already lost, or one whose drain
  fails (wgpu 29 panics when it polls a lost device; the panic is caught), is
  released without waiting. When the OS refuses a thread, the budgeted work
  runs inline.

### Changed

- `clip_path` masks fade over `AA_HALO_WIDTH` (half a pixel, agg-sharp's
  `HaloWidth`) like every GPU fill, instead of a full pixel, so a clip edge
  and a fill edge on the same outline match.
- Vertex, index and uniform data reach the GPU with one `queue.write_buffer`
  per arena chunk per frame instead of one per allocation (about three per
  draw command). wgpu backs every `write_buffer` with a staging buffer it
  creates and destroys, which on Metal cost about half of an idle repaint's
  main-thread time plus a third more in `device.poll`; the arena now stages
  each chunk's bytes on the CPU and uploads them before the frame's submit.

- `Gpu::new` builds its adapter and device on a separate thread and can now
  return `GpuInitError::StartupTimedOut` (new variant) when
  `GpuConfig::startup_budget` (new field) expires. `GpuInitError` and
  `GpuConfig` are `#[non_exhaustive]`, so the new variant and field compile
  against existing code (no exhaustive match or struct literal is possible);
  the behaviour change is that a driver that never answers is now an error
  instead of a hang.

## [0.5.3] - 2026-09-28

### Added

- `DrawCtx::clip_path` on `WgpuGfxCtx`: the clip layer is composited through
  a tessellated mask mesh (new layer-mesh pipeline and masked layer pop).
  Requires agg-gui 0.5.1, which adds the trait method.
- `Gpu::try_acquire_frame() -> Result<FrameAcquire, SurfaceError>`, with
  `FrameAcquire::{Frame, Skip(RetryWake)}` and
  `RetryWake::{Now, After(Duration), OnEvent}`: the caller, which owns the
  event loop, arranges the retry wake. `RetryWake::OnEvent` means *no*
  self-scheduled retry — the window is `Occluded`, or the acquire hit a
  validation error — so a shell must wake itself from a window event (e.g.
  request a redraw on `WindowEvent::Occluded(false)`); this is the old
  "skip without requesting a redraw" behaviour, made explicit.
- `SurfaceError::ConfigureRetriesExhausted` — the swap chain stayed
  unconfigured for longer than the retry budget, over at least 12 failed
  attempts. Carries wgpu's last error text, the span and the attempt count.
- `Gpu::surface_configured()` — read-only: `false` while a configure-failure
  run is in progress, so a shell can hold off attempts that cannot succeed
  (agg-gui-shell uses it to stop painting a minimized window only then).
- `GpuConfig::surface_retry_budget` / `with_surface_retry_budget` (default
  10 s; `Duration::MAX` retries forever).
- `GpuInitError::ConfigureSurface` — the initial configure failed (not
  retried: it creates the swap chain, so failure is a real error).

### Changed

- A failed `Surface::configure` no longer panics the process. Every native
  configure (`Gpu::new`, `Gpu::resize`, the stale-swap-chain reconfigure
  during acquire) runs inside `Validation` and `Internal` error scopes and
  checks for device loss. A failure after start-up is logged (`warn` once
  per run, `debug` on repeats, `info` on recovery) and retried by
  `try_acquire_frame`: first retry immediate, then 50 ms doubling to a 1 s
  cap, no frame acquired from the unconfigured surface meanwhile.
- `Gpu::acquire_frame` is now a compatibility wrapper over
  `try_acquire_frame` (prefer the new method; it will be deprecated in 0.6).
  It schedules `RetryWake::After` through
  `agg_gui::animation::request_draw_after`, and panics with the
  `SurfaceError` once the retry budget is spent — where it previously
  panicked inside wgpu on the first failed configure.

### Fixed

- `draw_image_rgba_corners` sampled images under the mipmap threshold with
  nearest filtering, leaving rotated or scaled sprites jagged; corner quads
  now always sample linearly. The axis-aligned 1:1 arc blit is unchanged.
- DX12 resize crash (`In Surface::configure - Invalid surface` /
  `DXGI_ERROR_INVALID_CALL`, then `E_ACCESSDENIED` on swap-chain
  re-creation): `WgpuGfxCtx` kept a clone of the frame's surface texture for
  read-back and never dropped it, so the back buffer outlived `present()`.
  New `WgpuGfxCtx::present(frame)` releases that stash (and any unconsumed
  `begin_frame` view) and then presents; `release_frame_texture()` is the
  release half for shells that don't present a frame. `set_surface_texture`
  logs one `warn!` per context when it finds a texture still stashed from an
  earlier frame. Shells should present through `WgpuGfxCtx::present` rather
  than `SurfaceTexture::present`. Root cause found by Nick LeFors
  (larsbrubaker/agg-gui#6).

## [0.5.2] - 2026-08-26

### Added

- `GpuConfig::with_optional_features` — device features requested when, and
  only when, the adapter offers them (the set is masked against
  `adapter.features()` before `request_device`, so an absent feature degrades
  instead of failing device creation). Needed by AtomArtist's depth-peel
  renderer, which uses `FLOAT32_BLENDABLE` when present and falls back to
  half-float depth when not; general enough for any renderer with an
  adapter-dependent fast path.

## [0.5.1] - 2026-08-26

### Added

- `Gpu::device_lost()` — wgpu reports device loss (TDR, driver reset, GPU
  removal, RDP session change) out-of-band through a callback, so a shell that
  only inspects `get_current_texture` keeps rendering nothing forever
  afterwards. `Gpu` now installs that callback and latches a flag for the shell
  to poll once per frame; our own `Device::destroy` is not counted as a loss.
  Recovery is to build a fresh `Gpu` for the same window — `agg-gui-shell`
  does this and tells the app to drop its cached GPU resources.
- `pick_present_mode` — the surface-capability fallback used below, exposed
  because it is pure and useful to a shell configuring its own surface.

### Changed

- `Gpu::new` falls back to `PresentMode::Fifo` when the requested explicit
  present mode is not in the surface's capabilities, instead of configuring an
  unsupported mode. The `Auto*` modes pass through untouched — wgpu resolves
  those itself.

## [0.5.0] - 2026-08-26

### Added

- **Initial release.** `agg-gui-wgpu` is the wgpu renderer extracted out of the
  agg-gui repo's in-repo `demo-wgpu` crate so apps can depend on the renderer
  without pulling in demo content or a platform shell. Versioned in lockstep
  with `agg-gui` 0.5. It contains:
  - `WgpuGfxCtx` — the `DrawCtx` implementation, its pipelines and shaders, the
    per-frame buffer arena, texture caches, compositing layers, and LCD
    subpixel text.
  - `custom_render` — the `WgpuCustomRender` hook for widgets that record their
    own render passes into the frame.
  - `ssaa::SsaaFramebuffer` and `ssaa_linear_scale`.
  - Screenshot capture and read-back (full-surface, scaled, and region), plus
    `RectInPixels` and `LastEndFrameStats`.
  - `gpu::Gpu` — the device + surface bundle for a native window, with the
    surface-acquire recovery policy and the max-texture-dimension clamp.
- `WgpuGfxCtx::begin_frame(view)` — the frame's render target is now installed
  through a real method instead of a free function reaching into crate-private
  fields. `WgpuGfxCtx::surface_format()` exposes the configured target format
  so a custom renderer can match it.

### Changed

- The surface-acquire policy is now shared by both native shells rather than
  duplicated. The two copies disagreed about `Timeout`: it now skips the frame
  **and requests another redraw**, so a reactive event loop cannot wedge itself
  waiting for an event that never comes. `Outdated`/`Lost` still reconfigure and
  retry within the same frame, `Occluded`/`Validation` still skip silently.
- Surface configuration is clamped to the device's `max_texture_dimension_2d` on
  every `configure`, not just in the demo shell that had the clamp.
- `Gpu::new` returns a `Result` instead of panicking, and states its
  `COPY_SRC` requirement explicitly (`CopySrc::Never` / `IfSupported` /
  `Required`) rather than assuming the surface supports read-back. A surface
  that reports no formats or no alpha modes yields `GpuInitError::NoSurfaceFormats`
  / `NoAlphaModes` instead of an index panic.
- `GpuConfig`, `CopySrc`, `GpuInitError`, `SurfaceAcquire`, `WgpuPaintContext`
  and `WgpuCustomRenderCtx` are `#[non_exhaustive]` so they can grow without a
  breaking release. Build a `GpuConfig` with `GpuConfig::new(label)` plus
  `with_copy_src` / `with_present_mode` rather than a struct literal.
- The `agg-gui` dependency is taken with `default-features = false`; enable this
  crate's `reflect` feature to forward `agg-gui/reflect`.

### Removed

- `DrawCommand::DrawBarGrid`, the hard-coded variant for the demo's 3-D cube.
  The cube now uses the public `custom_render` hook like any other GPU widget.
