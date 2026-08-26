# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Because the crate is pre-1.0, breaking changes are released in `0.MINOR.0` bumps.

## [0.5.0] - 2026-08-25

### Added

- **Initial release.** `agg-gui-shell` is the native platform shell —
  winit window, event loop, and wgpu present — extracted so apps stop copying
  a demo's event loop. Versioned in lockstep with `agg-gui` 0.5. It is the
  union of the two hand-rolled shells that existed in the agg-gui repo, each of
  which had fixes the other lacked:
  - `run` with a builder closure that constructs the app *after* the window and
    GPU exist, and a `ShellHost` trait for everything the app wants to do
    around a frame (per-frame tick with the previous frame's duration, custom
    frame body, GPU read-back before `present`, geometry changes, idle work,
    device-loss invalidation, exit hook).
  - `ShellConfig` — title (owned `String`), logical or physical initial size,
    minimum size, RGBA window icon, maximized, fullscreen-at-start,
    `RedrawPolicy`, `CopySrc` and present-mode passthrough, OS tooltip timings
    (Windows `SPI_GETMOUSEHOVERTIME`), and deterministic screenshot capture.
  - `WindowBoundsStore` + `sanitize_restored_window_size` — window-bounds
    persistence in physical pixels, sanitised against the real monitor and the
    GPU limit, saved only when the bounds changed and no mouse button is held.
  - Input: key-**up** as well as key-down, raw touch, cursor-leave, the
    shift→horizontal wheel remap, and no sign flipping of wheel deltas.
  - The agg-gui host waker, installed only after the window and GPU came up and
    cleared on every exit path, and `ShellControl::request_exit` /
    `request_relaunch`.
  - Device-loss recovery: `agg_gui_wgpu::Gpu::device_lost()` is polled every
    frame, and a lost device is rebuilt (device, surface, render context) with
    `ShellHost::on_gpu_rebuilt` telling the app to drop its own cached GPU
    resources.
  - Resize coalescing — a `Resized` delivered from a modal drag-resize loop is
    applied at the top of the next frame, never between surface acquire and
    present.

### Notes

- `winit` and `wgpu` types are part of this crate's public API, so their major
  versions are too: a `winit` 0.31 or `wgpu` 30 will be a breaking release here.
- The shell never calls `std::process::exit` and never prints; failures come
  back as `ShellError` and diagnostics go through the `log` facade.
