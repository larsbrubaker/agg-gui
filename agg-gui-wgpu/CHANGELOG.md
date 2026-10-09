# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Because the crate is pre-1.0, breaking changes are released in `0.MINOR.0` bumps.

## [Unreleased]

### Added

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
