# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Because the crate is pre-1.0, breaking changes are released in `0.MINOR.0` bumps.

## [Unreleased]

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
