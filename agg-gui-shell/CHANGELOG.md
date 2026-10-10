# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Because the crate is pre-1.0, breaking changes are released in `0.MINOR.0` bumps.

## [Unreleased]

### Added

- `ShellConfig::force_fallback_adapter` / `with_force_fallback_adapter`: the
  window renders on wgpu's software (fallback) adapter, for a user whose GPU
  driver is broken; a device rebuilt after a loss keeps the choice. With no
  fallback adapter, `run` ends with
  `ShellError::Gpu(GpuInitError::NoFallbackAdapter)`.

- `WindowsLcdDisplayEnvironmentProvider`: agg-sharp's Windows reader for LCD
  subpixel detection (`SPI_GETFONTSMOOTHING`, `SPI_GETFONTSMOOTHINGTYPE`,
  `SPI_GETFONTSMOOTHINGORIENTATION`, `SM_REMOTESESSION` and the primary
  display's rotation), feeding `agg_gui::lcd_display_detection`. Off Windows it
  answers "cannot say". The `windows-sys` dependency gains `Win32_Graphics_Gdi`.

- The device is built within agg-sharp's 15 s start-up budget (`run` ends with
  `ShellError::Gpu(GpuInitError::StartupTimedOut)` instead of hanging), and
  the close releases it within the 5 s teardown budget
  (`Gpu::release_within_budget`). `GpuInitError` and `gpu_budget` are
  re-exported.

- winit `HoveredFile` / `HoveredFileCancelled` are forwarded to
  `App::on_file_drag_hover` / `App::on_file_drag_leave`: each hover carries
  every path of the drag seen so far, at the live cursor position, and a
  `CursorMoved` during the drag (where the platform reports one) re-sends the
  hover at the new position. A drop clears the drag.

### Changed

- `run` switches agg-gui to the system clipboard at startup
  (`agg_gui::clipboard::use_system_clipboard`, which agg-gui now requires
  before it touches the OS clipboard) and closes that connection
  (`release_system_clipboard`) when `run` returns, on every exit path, so copied text
  outlives the app under an X11 clipboard manager. Apps on the shell keep the
  system clipboard with no change.

- `Frame::needs_layout` is also `true` while a widget's
  `agg_gui::animation::request_layout()` is pending, so a request made during
  layout gets the next frame laid out even when the size, scale and
  invalidation epoch are unchanged.

## [0.5.2] - 2026-09-28

### Added

- `ShellError::Surface` — the window's surface stayed unusable for the whole
  swap-chain configure retry budget (see agg-gui-wgpu's
  `GpuConfig::surface_retry_budget`); the shell exits with it instead of
  showing a black window forever.

### Changed

- Frames are acquired with `Gpu::try_acquire_frame`. A refused frame arranges
  its retry from the `RetryWake` (`Now` → window redraw, `After` → a draw
  deadline, `OnEvent` → nothing) and drops the immediate draw request, so a
  reactive loop no longer sits in `ControlFlow::Poll` while the surface backs
  off. While a configure retry is backing off the loop waits for it
  (`WaitUntil`) even in `RedrawPolicy::Continuous` or with a capture pending.
- While the window is minimized *and* its surface is unconfigured (a
  configure-failure run in progress), the paint is skipped entirely — no
  acquire or configure, so no retry budget is spent against the minimized
  window — and continuous mode stops polling; restoring (a nonzero
  `Resized`) repaints and resumes the retries. A minimized window whose
  surface is configured paints and polls exactly as before.
- `WindowEvent::Occluded(false)` requests a redraw, since an occluded skip
  (`RetryWake::OnEvent`) schedules no retry of its own.

### Fixed

- Frames are presented through `WgpuGfxCtx::present`, which releases the
  context's stashed back-buffer handle first — the cause of the DX12 resize
  crash (`Surface::configure` panicking with "Invalid surface").

## [0.5.1] - 2026-08-26

### Added

- `ShellHost::on_close_requested` — called when the user asks the OS window to
  close (title-bar ×, Alt+F4). Return `false` to keep the app running; the
  seam an unsaved-changes gate needs (prompt Save / Discard / Cancel, or defer
  and finish the close from `on_idle` via `ShellControl::request_exit` once an
  asynchronous save lands). Defaults to `true`, so existing hosts are
  unchanged. Needed by AtomArtist's close gate; every document-editing app
  wants the same seam.
- `WindowEvent::DroppedFile` is now forwarded to `App::on_file_dropped`, one
  call per dropped file, at the live OS cursor position on Windows — winit's
  OLE drop path emits no `CursorMoved` during the drag and discards the drop
  point, so the tracked cursor is stale there (ported from AtomArtist's shell,
  which carried the `GetCursorPos` workaround). Other platforms use the
  tracked cursor.
- `ShellConfig::with_optional_features` — pass-through to
  `agg_gui_wgpu::GpuConfig::with_optional_features`: device features requested
  only when the adapter offers them.

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
