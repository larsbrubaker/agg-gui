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

## Usage

```rust,ignore
let mut ctx = WgpuGfxCtx::new(device, queue, surface_format, width, height);

// each frame
ctx.reset(width, height);
ctx.begin_frame(surface_view);
app.paint(&mut ctx);
ctx.end_frame();
surface_texture.present();
```

Turn-key platform shells (winit event loop, browser canvas + rAF loop) live in
the agg-gui repo's `demo-wgpu` crate.

## License

MIT
