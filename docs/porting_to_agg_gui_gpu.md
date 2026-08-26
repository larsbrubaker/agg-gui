# Porting to agg-gui-wgpu

*A migration guide for apps built on agg-gui — the rust-apps fleet (Antidote, atomartist, instant-astronomer, KeyInSight, Marbles, Solitaire, verovio-rust, LabelPrintServer) and, for parity tracking, MatterCAD/agg-sharp.*

## What changed

The GPU renderer and (soon) the native event loop are now **published crates** instead of demo code you copy:

| Crate | Version | Status | What it is |
|---|---|---|---|
| `agg-gui` | 0.5.0 | on crates.io | The widget/layout/text library. Unchanged role. |
| `agg-gui-wgpu` | 0.5.0 | on crates.io | The wgpu renderer, extracted from `demo-wgpu`: `WgpuGfxCtx`, the shared `Gpu` + surface-acquire policy, SSAA, screenshots, custom render hook. |
| `agg-gui-shell` | 0.5.x | in development | The native winit event loop (the union of `native_shell` and `demo-native`'s hand-rolled loop, plus device-loss recovery). Will replace every app's copied shell. |
| `demo-wgpu` | — | repo-only shim | Now re-exports `agg-gui-wgpu` plus the demo shells. Path-dependencies keep compiling, but treat it as deprecated. |

Why: every app hand-carried ~250+ lines of platform glue copied from the demos, and the copies diverged — each had bugs the others had fixed. Example: the surface-loss freeze (window permanently blank after a GPU driver reset or RDP reconnect) was fixed in `demo-native`, still present in `native_shell`, and inherited by every app that copied it. That class of drift ends here.

## Migrating a Rust app

### 1. Fix your `agg-gui` requirement (check this even if you change nothing else)

Several apps pin an old version with a local override:

```toml
agg-gui = "0.2"                      # ← stale

[patch.crates-io]
agg-gui = { path = "../agg-gui/agg-gui" }
```

**A `[patch.crates-io]` entry is silently inert when the version requirement doesn't match the patched crate.** The local checkout is 0.5.0, so a `"0.2"` requirement resolves to the *crates.io 0.2 release* and your build quietly diverges from the checkout (this is why Solitaire currently fails to build). Set:

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
agg-gui-wgpu = "0.5"

[patch.crates-io]
agg-gui-wgpu = { path = "../agg-gui/agg-gui-wgpu" }   # optional, for local dev
```

API mapping (mechanical):

| Old (`demo_wgpu::`) | New (`agg_gui_wgpu::`) |
|---|---|
| `begin_frame(&mut ctx, view)` | `ctx.begin_frame(view)` — now a method |
| your hand-rolled `Gpu` / `gpu.rs` copy | `Gpu::new(target, (w, h), GpuConfig::new("label"))` |
| `gpu.device` / `gpu.surface_format` / `gpu.config.width` | `gpu.device()` / `gpu.surface_format()` / `gpu.config().width` |
| your `surface_acquire_action` / acquire match | `gpu.acquire_frame(\|\| window.request_redraw())` |
| unconditional `COPY_SRC` usage flags | `GpuConfig::with_copy_src(CopySrc::{Never, IfSupported, Required})` |
| — | `GpuConfig::with_present_mode(...)` |

Notes:

- `Gpu::new` takes `impl Into<wgpu::SurfaceTarget<'static>>` — the renderer has **no winit dependency**.
- The unified acquire policy is a strict superset of both old copies: `Lost`/`Outdated` reconfigure and retry once, then fall back to a requested redraw; `Timeout` skips the frame **and requests a redraw** (some copies silently dropped it); `Occluded`/`Validation` skip without spinning. Surface sizes are clamped to `max_texture_dimension_2d` on every configure.
- Public config/enum types are `#[non_exhaustive]` — construct via `GpuConfig::new(..)` + `with_*` builders, and put a `_ =>` arm in matches over `SurfaceAcquire` / `GpuInitError`.
- **`reflect` is now opt-in.** `agg-gui-wgpu` pulls `agg-gui` with `default-features = false`; if you use the inspector's typed editors, enable `agg-gui-wgpu = { version = "0.5", features = ["reflect"] }` (it forwards to `agg-gui/reflect`). If you never used it, you just lost the `bevy_reflect` compile time for free.
- **MSRV is 1.87** (wgpu 29's requirement). `agg-gui` itself still claims 1.70.

### 3. Copy the dev-profile settings (once per workspace)

The renderer's inlined hot path monomorphizes into *your* crate; at `opt-level = 0` frame times are ~10× worse. Add to your workspace `Cargo.toml` if you haven't:

```toml
[profile.dev]
opt-level = 1

[profile.dev.package."*"]
opt-level = 2
```

### 4. When `agg-gui-shell` lands: delete your event loop

If you copied `native_shell.rs` or `demo-native`'s loop, plan to delete it. The shell crate owns: window creation (icon, min size, maximized, paint-first-frame-then-show), the full input mapping (key **down and up**, touch, `CursorLeft`, modifiers, the `/40` pixel-wheel scaling and the do-not-negate-deltas convention, shift+wheel→horizontal), DPI/scale changes, cursor icons, the reactive `ControlFlow` ladder (`Poll` / `WaitUntil(next_draw_deadline)` / `Wait`), `fullscreen::take_request`, the host waker for background-thread wakeups, surface *and* device-loss recovery, present-mode fallback, window-bounds persistence behind a small storage trait, and OS tooltip timings on Windows.

What stays yours: menus, file dialogs, crash reporting, single-instance, splash screens, tokio runtimes, inspector plumbing — threaded through the shell's hooks.

Until then, audit your copy against the two known bug classes: (a) key-up events never dispatched (`demo-native` lineage), (b) frozen window after surface loss (`native_shell` lineage — fixed upstream in commit `e431f35`, mirror it if you keep a copy in the interim).

**Reference migration:** LabelPrintServer did steps 1–2 and deleted ~130 lines of copied GPU/policy code; its shell file is now just the event loop. See `crates/label-print-server/src/native_ui/shell.rs` in that repo for the call-site shapes.

## MatterCAD / agg-sharp parity notes

MatterCAD doesn't consume these crates, but the two codebases deliberately track each other. What's converging:

- The Rust `SurfaceAcquire` policy (`Present` / `Reconfigure` / `SkipAndRetry` / `Skip`) is now the canonical vocabulary for what `WebGpuSurfaceTarget.TryAcquire` + `AcquireCurrentTexture` already do in C#. Parity comments should reference `agg-gui-wgpu/src/gpu.rs` rather than the old demo files.
- `agg-gui-shell` is adopting three behaviors MatterCAD proved out: full **device**-loss recovery (rebuild device + invalidate caches, `WebGpuControl.TryRecoverDevice`), the **scratch render target** so widget paint always has a legal destination, and **mid-frame resize** handling. When the Rust versions land, cross-reference them from the C# sources the same way the C# is referenced from Rust today.
- The smoke-run contract (`AGG_SMOKE_FRAMES` / `AGG_SMOKE_SCREENSHOT`) stays shared between both codebases.

## Publishing and contributing

- Versions ship on the 0.5 line; changelogs are Keep-a-Changelog in each crate.
- **Publish only via the `publish` GitHub Actions workflow** (`gh workflow run publish.yml -f crate=<name>` in the agg-gui repo). The office network path drops HTTPS uploads over ~1.1 MB, so `cargo publish` from a dev box fails on any real crate — this is a network middlebox, not crates.io.
- Found a divergence between your app's copied glue and the shared crates? File it against agg-gui rather than patching your copy — the whole point is one implementation.
