# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Because the crate is pre-1.0, breaking changes are released in `0.MINOR.0` bumps.

## [0.5.0] - Unreleased

### Added

- First release: the browser counterpart of `agg-gui-shell`, extracted from
  `demo-wgpu`'s `web_shell` and merged with AtomArtist's hand-rolled web shell.
- `start(WebShellConfig, builder)` — canvas lookup, client-platform detection
  (`?agg_input=mobile|desktop` override), async GPU bring-up, then the app
  builder (after the GPU exists, like the native shell), and the
  `requestAnimationFrame` loop.
- `Backend::{WebGpu, WebGl2, PreferWebGpu}` — the consumer picks the browser
  graphics API; `PreferWebGpu` probes WebGPU before the canvas is bound so the
  WebGL2 fallback is clean. WebGPU is behind the default `webgpu` feature.
- `WebShellHost` mirroring `agg_gui_shell::ShellHost` (`on_frame`, `paint`,
  `after_paint`, `on_geometry_changed`, `on_idle`, `on_gpu_rebuilt`) plus
  web-only `on_tick` (every rAF) and `on_page_hide` (visibilitychange/pagehide
  flush).
- `RedrawPolicy::{Reactive, Continuous}` with scheduled-deadline wake-ups, and
  `FirstPaintGate` — the first frame always paints, fixing the
  blank-page-until-resize bug AtomArtist hit.
- DOM pointer (mouse / pen / multi-touch), wheel, pointer-leave and
  context-menu forwarding; keyboard + clipboard via `agg_gui::web_adapter`;
  window-level pointer-release resync behind `WebShellControl::pointer_idle`.
- DPR tracking and canvas backing-store sizing every tick, clamped to the
  device texture limit; surface-acquire recovery; device-loss rebuild.
- `WebShellConfig::offscreen_scene` — render through a copyable scene texture
  so `WgpuGfxCtx::capture_screenshot` works on the web.
- `LocalStorageSettings` / `MemorySettings` / `SettingsAutoSave` — optional
  diff-guarded, pointer-idle-gated settings persistence.
- `agg_gui::fullscreen` (with mobile orientation lock), `agg_gui::tilt` and
  `agg_gui::gamepad` plumbing, and a fatal-error panel in place of the canvas.
