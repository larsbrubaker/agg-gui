# Porting update: agg-gui-wgpu + agg-gui-shell

*A migration guide for apps built on agg-gui — the rust-apps fleet (Antidote, atomartist, instant-astronomer, KeyInSight, Marbles, Solitaire, verovio-rust, LabelPrintServer) and, for parity tracking, MatterCAD/agg-sharp.*

## What changed

The GPU renderer and the native event loop are now **published crates** instead of demo code you copy:

| Crate | Version | Status | What it is |
|---|---|---|---|
| `agg-gui` | 0.5.0 | on crates.io | The widget/layout/text library. Unchanged role. |
| `agg-gui-wgpu` | 0.5.1 | on crates.io | The wgpu renderer, extracted from `demo-wgpu`: `WgpuGfxCtx`, the shared `Gpu` + surface-acquire policy, device-lost detection, present-mode fallback, SSAA, screenshots, the custom-render hook. |
| `agg-gui-shell` | 0.5.0 | on crates.io | The native winit event loop — the union of `native_shell` and `demo-native`'s hand-rolled loop, plus device-loss recovery and window-bounds persistence. Replaces every app's copied shell. |
| `demo-wgpu` | — | repo-only shim | Re-exports `agg-gui-wgpu`; `native_shell` is `#[deprecated]`, wrapping `agg-gui-shell`. Path deps keep compiling, but migrate off it. |

Why: every app hand-carried ~250+ lines of platform glue copied from the demos, and the copies diverged — each had bugs the others had fixed (frozen window after GPU reset in one lineage, key-up events never dispatched in the other, a maximized-restore bug in both). One implementation ends that drift.

## Migrating a Rust app

### 1. Fix your `agg-gui` requirement (check this even if you change nothing else)

Several apps pin an old version with a local override:

```toml
agg-gui = "0.2"                      # ← stale

[patch.crates-io]
agg-gui = { path = "../agg-gui/agg-gui" }
```

**A `[patch.crates-io]` entry is silently inert when the version requirement doesn't match the patched crate.** The local checkout is 0.5.x, so a `"0.2"` requirement resolves to the *crates.io 0.2 release* and your build quietly diverges from the checkout (this is why Solitaire currently fails to build). Set:

```toml
agg-gui = "0.5"

[patch.crates-io]
agg-gui = { path = "../agg-gui/agg-gui" }        # optional, for local dev
```

### 2. Replace `demo-wgpu` with `agg-gui-wgpu`

```toml
# before
demo-wgpu = { path = "../agg-gui/demo-wgpu", default-features = false }

# after
agg-gui-wgpu = "0.5.1"
```

API mapping (mechanical):

| Old (`demo_wgpu::`) | New (`agg_gui_wgpu::`) |
|---|---|
| `begin_frame(&mut ctx, view)` | `ctx.begin_frame(view)` — now a method |
| your hand-rolled `Gpu` / `gpu.rs` copy | `Gpu::new(target, (w, h), GpuConfig::new("label"))` |
| `gpu.device` / `gpu.surface_format` / `gpu.config.width` | `gpu.device()` / `gpu.surface_format()` / `gpu.config().width` |
| your `surface_acquire_action` / acquire match | `gpu.acquire_frame(\|\| window.request_redraw())` |
| unconditional `COPY_SRC` usage flags | `GpuConfig::with_copy_src(CopySrc::{Never, IfSupported, Required})` |
| — | `GpuConfig::with_present_mode(...)` (falls back to Fifo when unsupported) |
| — | `gpu.device_lost()` — latched from wgpu's device-lost callback |

Notes:

- `Gpu::new` takes `impl Into<wgpu::SurfaceTarget<'static>>` — the renderer has **no winit dependency**.
- The unified acquire policy is a strict superset of the old copies: `Lost`/`Outdated` reconfigure and retry once, then fall back to a requested redraw; `Timeout` skips the frame **and requests a redraw**; `Occluded`/`Validation` skip without spinning. Surface sizes are clamped to `max_texture_dimension_2d` on every configure.
- Public config/enum types are `#[non_exhaustive]` — construct via `GpuConfig::new(..)` + `with_*` builders, and put a `_ =>` arm in matches over `SurfaceAcquire` / `GpuInitError`.
- **`reflect` is opt-in.** If you use the inspector's typed editors, enable `features = ["reflect"]`; otherwise you just lost the `bevy_reflect` compile time for free.
- **MSRV is 1.87** (wgpu 29's requirement).

### 3. Copy the dev-profile settings (once per workspace)

The renderer's inlined hot path monomorphizes into *your* crate; at `opt-level = 0` frame times are ~10× worse:

```toml
[profile.dev]
opt-level = 1

[profile.dev.package."*"]
opt-level = 2
```

### 4. Delete your event loop — `agg-gui-shell` is live

```toml
agg-gui-shell = "0.5"      # re-exports wgpu and winit so your majors always match
```

```rust
use agg_gui_shell::{run, ShellConfig, NoHost};

run(
    ShellConfig::new("My App")
        .with_logical_size(1024.0, 768.0)
        .with_min_logical_size(800.0, 600.0),
    |init| {
        let app = build_my_app(init.size());   // device scale + surface size already known
        Ok((app, NoHost))                      // or your own ShellHost impl
    },
)?;
```

The shell owns: window creation (icon, min size, maximized/fullscreen start, paint-first-frame-then-show), the full input mapping (key down **and** up, touch, `CursorLeft`, modifiers, the `/40` pixel-wheel scaling and the do-not-negate-deltas convention, shift+wheel→horizontal), DPI/scale changes, cursor icons, the reactive `ControlFlow` ladder, `fullscreen::take_request`, the host waker for background-thread wakeups (`animation::signal_async_state_change` from any thread), **surface and device-loss recovery** (your `ShellHost::on_gpu_rebuilt` drops cached GPU resources), present-mode fallback, coalesced resize (safe against mid-frame drag-resize), window-bounds persistence behind the `WindowBoundsStore` trait (with the maximized-restore and DPI-ratchet bugs fixed and pinned by tests), OS tooltip timings on Windows, screenshot capture as `Result` (no `process::exit`), and exit/relaunch via `ShellControl`.

What stays yours, threaded through `ShellHost` hooks (`on_frame`, `paint` override, `after_paint`, `on_idle`, `on_geometry_changed`, `on_gpu_rebuilt`, `on_exit`): menus, file dialogs, crash reporting, single-instance, splash screens, tokio runtimes, inspector plumbing. `demo-native` is the reference port: a 196-line `main.rs` plus a `DemoHost`.

**Reference migrations:** LabelPrintServer (steps 1–2, deleted ~130 lines of GPU/policy code) and `demo-native` (full shell adoption) in this repo.

## Known upstream issues you may hit (report additions, don't fork)

- `Container` always fills the width it is offered (no fit-width mode) — hug-content layouts need a measure-and-pin workaround today.
- A `FlexColumn` inside a non-fit-height `Container` positions children against the measured height, not the painted one.
- Variable fonts (`fvar`/`gvar`) shape/tessellate pathologically slowly (>60 s for a test frame vs 8 s with a static font). Prefer static instances until fixed.
- `ScrollView` measures content at `f64::MAX / 2`, where `Container`'s fit-height accumulator loses all precision and reports ~zero height (content silently truncated instead of scrolled). Workaround: re-offer a finite huge height (e.g. `1.0e10`) via a wrapper widget; the real fix is a magnitude guard in `container.rs` like the one `flex.rs` already has.
- Deliberately deferred from `agg-gui-shell` 0.5.0 (MatterCAD-proven, planned): the **scratch render target** (paint destination during unpresentable frames) and the **`AGG_SMOKE_FRAMES` / `AGG_SMOKE_SCREENSHOT` smoke-run contract** as a `ShellConfig` option.

## MatterCAD / agg-sharp parity notes

- The Rust `SurfaceAcquire` policy (`Present` / `Reconfigure` / `SkipAndRetry` / `Skip`) is the canonical vocabulary for what `WebGpuSurfaceTarget.TryAcquire` + `AcquireCurrentTexture` do in C#. Parity comments should reference `agg-gui-wgpu/src/gpu.rs`.
- Device-loss recovery now exists on the Rust side (`Gpu::device_lost()` + the shell's rebuild path), modeled on `WebGpuControl.TryRecoverDevice`. Cross-reference both directions.
- The smoke-run contract stays shared; when it lands in `ShellConfig` (see above), align the env-var semantics with the three C# implementations.

## Publishing and contributing

- Versions ship on the 0.5 line; changelogs are Keep-a-Changelog in each crate.
- **Publish only via the `publish` GitHub Actions workflow** (`gh workflow run publish.yml -f crate=<name>`). The office network path drops HTTPS uploads over ~1.1 MB, so `cargo publish` from a dev box fails on any real crate — a network middlebox, not crates.io.
- Found a divergence between your app's copied glue and the shared crates? File it against agg-gui rather than patching your copy — the whole point is one implementation.
