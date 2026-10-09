# agg-gui-wgpu

Hardware-accelerated [`wgpu`](https://crates.io/crates/wgpu) renderer for
[agg-gui](https://crates.io/crates/agg-gui).

`WgpuGfxCtx` implements agg-gui's `DrawCtx` on top of wgpu, so the same widget
tree that renders through the AGG software rasterizer renders on the GPU
instead.

| Target | Backend |
|---|---|
| Windows | Vulkan, DX12 |
| macOS / iOS | Metal |
| Linux / Android | Vulkan |
| WASM (`wasm32-unknown-unknown`) | WebGL2 |

## What's in here

- **`WgpuGfxCtx`** — the `DrawCtx` implementation. Fills, strokes, gradients,
  images, LCD subpixel text, compositing layers (transient and retained), and
  clipping, accumulated as deferred draw commands and flushed to a single
  command encoder in `end_frame`.
- **`custom_render`** — the hook a widget uses to run its own wgpu render
  pass(es) interleaved with the 2-D stream, on the same surface or layer
  texture. This is how a 3-D viewport widget plugs in.
- **`ssaa::SsaaFramebuffer`** — offscreen supersampled colour + depth target
  with a blit-to-surface that reuses the shared textured-quad pipeline.
- **Screenshot capture** — GPU-resident capture, full-surface read-back, and
  scaled / region read-back (both blocking for native Save/Copy and
  poll-based for the browser, where a blocking map would deadlock).
- **`gpu::Gpu`** — the device + surface bundle a native shell builds its swap
  chain on, including the surface-acquire recovery policy and the
  max-texture-dimension clamp.
- **`headless`** (native) — rendering with no window, for test harnesses:
  `HeadlessGpu::shared()` (one device per process, requested as `Gpu::new`
  requests a window's), `HeadlessTarget` (an offscreen texture in the shells'
  frame format with RGBA read-back), and `HeadlessFrame`, which runs a shell's
  frame around a paint closure or an `App` — custom render passes included.

## Usage

```rust,ignore
let mut ctx = WgpuGfxCtx::new(device, queue, surface_format, width, height);

// each frame
ctx.reset(width, height);
ctx.begin_frame(surface_view);
app.paint(&mut ctx);
ctx.end_frame();
ctx.present(surface_texture); // releases the ctx's frame handles, then presents
```

Always present via `ctx.present(...)` rather than `surface_texture.present()`:
the ctx may hold a clone of the back buffer for screenshots, and on DX12 a
clone that outlives present makes the next swap-chain resize fail.

Headless, in a test:

```rust,ignore
let gpu = HeadlessGpu::shared()?;            // Err(NoAdapter) on a GPU-less machine
let mut frame = HeadlessFrame::new(gpu, 1280, 800);
app.layout(Size::new(1280.0, 800.0));
frame.render_app(Color::white(), &mut app);
let rgba = frame.read_rgba()?;               // top row first, width * 4 bytes per row
```

Turn-key platform shells (winit event loop, browser canvas + rAF loop) live in
the agg-gui repo's `demo-wgpu` crate.

## License

MIT
