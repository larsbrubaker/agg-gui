# agg-gui-web-shell

Turn-key browser shell for [agg-gui](https://crates.io/crates/agg-gui): an
HTML `<canvas>`, the [agg-gui-wgpu](https://crates.io/crates/agg-gui-wgpu)
renderer presenting to it through **WebGPU or WebGL2**, and the
`requestAnimationFrame` loop in between. The wasm counterpart of
[agg-gui-shell](https://crates.io/crates/agg-gui-shell), with the same host
trait shape so an app's native and web glue are near-identical.

```rust,ignore
use agg_gui_web_shell::{start, Backend, NoHost, WebShellConfig};
use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn main() {
    let config = WebShellConfig::new("canvas")
        .with_backend(Backend::WebGpu)
        .with_app_name("My App");
    // Failures are already on the console and in the fatal panel.
    start(config, |_init| Ok((build_my_app(), NoHost))).expect("web shell start");
}
```

The app is built by a closure that runs *after* the GPU exists
(`init.gpu()` hands over the device and queue for an app renderer).

## What the shell owns

- **Surface** — WebGPU, WebGL2, or WebGPU-with-WebGL2-fallback (`Backend`);
  device limits sized for the backend with the texture caps raised to the
  adapter's; required (`with_required_features`, an error when missing) and
  optional features; acquire recovery; WebGPU device-loss rebuild with
  backoff and an `on_gpu_rebuilt` hook (a lost WebGL2 context is reported as
  fatal, not rebuilt); optional offscreen scene so screenshots work.
- **Frame loop** — reactive (paint on clicks/keys/touch, widget
  invalidations, animation requests, due deadlines — plain mouse moves paint
  only when they change something) or continuous; layout skipped when nothing
  feeding it changed; the first frame is always painted, and
  `has_presented()` / the `after_present` hook tell the page when it is on
  screen.
- **Sizing** — canvas backing store = CSS size × `devicePixelRatio`, re-synced
  every tick (browser zoom changes DPR without a resize event); when that
  would exceed the GPU's texture limit the scale is reduced uniformly and
  pointer mapping follows it.
- **Errors** — every failure (sync or async) goes to `console.error`, the
  `with_on_fatal` hook, and a readable panel in place of the canvas
  (`with_app_name` / `with_fatal_message` for the wording; a built-in style
  with a dark variant, or your own class via `with_fatal_panel_class`).
  Calling `start` twice returns `WebShellError::AlreadyStarted`.
- **Input** — pointer events for mouse, pen and multi-touch, wheel (DOM deltas
  normalised to notches), pointer-leave, keyboard down/up and the clipboard
  bridge, cursor icon, `touch-action: none`.
- **Lifecycle** — `visibilitychange`/`pagehide` → `on_page_hide`, and a
  window-level pointer-release listener so a drag that ends outside the canvas
  can't wedge the pointer-idle guard an auto-save waits on.
- **Platform** — user-agent / `(pointer: coarse)` detection (override with
  `?agg_input=mobile`), fullscreen, device tilt, gamepads.

## What the app owns

Everything else, through `WebShellHost`: per-tick polling, per-frame state, a
custom frame body, read-back after `end_frame`, auto-save in `on_idle`, and the
page-hide flush. `LocalStorageSettings` + `SettingsAutoSave` cover the common
"one settings blob in localStorage" case.

Inside a host callback use the `&mut App` argument: the global `with_app`
returns `None` there (the app is already borrowed). `with_canvas`,
`mark_dirty`, `set_redraw_policy` and `has_presented` are safe anywhere.

## Targets

The runtime exists only on `wasm32-unknown-unknown`. On native targets the
crate builds its platform-neutral half (config, host trait, paint policy, DOM
math, settings store) so workspaces build and test everywhere.

## Versioning note

`wgpu` types appear in this crate's public API; a `wgpu` major bump is a
breaking release here. Use the re-exported `agg_gui_web_shell::wgpu` (and
`agg_gui`, `agg_gui_wgpu`, `web_sys`) so versions always match the shell's.
`Backend` and `RedrawPolicy` are `#[non_exhaustive]`.

## License

MIT
