# agg-gui-shell

Turn-key native shell for [agg-gui](https://crates.io/crates/agg-gui): a
[`winit`](https://crates.io/crates/winit) window, the
[agg-gui-wgpu](https://crates.io/crates/agg-gui-wgpu) renderer presenting to it,
and the event loop in between.

```rust,ignore
use agg_gui_shell::{run, NoHost, ShellConfig};

fn main() -> Result<(), agg_gui_shell::ShellError> {
    run(
        ShellConfig::new("My App").with_logical_size(1280.0, 720.0),
        |_init| Ok((build_my_app(), NoHost)),
    )
}
```

The app is built by a closure that runs *after* the window and GPU exist, so
the widget tree sees the real device scale and surface size while it is being
constructed.

## What the shell owns

- **Window** — title, logical or physical initial size, minimum size, RGBA
  icon, maximized, borderless fullscreen. The first frame is painted into the
  still-hidden window and the window is shown afterwards, so start-up never
  flashes an OS-default white background.
- **Input** — mouse move/down/up, cursor-leave, modifiers, wheel (with the
  shift→horizontal remap and no sign flipping, so the OS scroll-direction
  preference is respected), keyboard **down and up**, and raw touch forwarded
  to agg-gui's gesture aggregation.
- **Redraw scheduling** — `Poll` while the app wants frames, `WaitUntil` for a
  scheduled deadline, `Wait` otherwise; or `RedrawPolicy::Continuous` for an
  app that wants every frame. The agg-gui host waker is installed so a
  background thread can wake a parked loop, and cleared on every exit path.
- **Surface health** — acquire recovery, resize coalescing (winit can deliver
  `Resized` from a modal drag-resize loop), present-mode fallback, and
  rebuilding the device after a **device loss** (TDR, driver reset, GPU
  removal, RDP session change), with a hook telling the app to drop its cached
  GPU resources.
- **Window-bounds persistence** — restores the saved size through a
  `WindowBoundsStore` the app implements, sanitised against the real monitor
  and the GPU's maximum texture dimension. Sizes round-trip in physical pixels;
  restoring them as logical ones is the DPI ratchet that eventually grows the
  window past what the GPU can allocate.
- **Deterministic screenshots** — paint N settle frames, read the surface back,
  write a PNG, exit. Failures come back as a `ShellError`; the shell never
  exits the process itself.

## What the app owns

Everything else, through the `ShellHost` trait: per-frame state ticks, a custom
frame body, GPU read-back between `end_frame` and `present`, geometry-change
notifications, idle-time work, and exit or relaunch. Every method has a
default, so `NoHost` is a complete implementation.

## Versioning note

`winit` and `wgpu` types appear in this crate's public API (present mode, the
render context, the window handed to the builder closure). Their major versions
are therefore part of this crate's public API: a `winit` 0.31 or `wgpu` 30 is a
breaking release here, and an app must use the same majors this crate does.

## License

MIT
