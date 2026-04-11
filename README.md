# agg-gui

A Rust GUI framework built on [Anti-Grain Geometry (AGG)](https://crates.io/crates/agg-rust). AGG widgets render directly to the GL surface — there is no single global pixel buffer. Individual widgets that benefit from caching (e.g. text) may keep their own back-buffer, but the overall rendering model is direct-to-GL rasterization via AGG paths.

[![CI](https://github.com/larsbrubaker/agg-gui/actions/workflows/ci.yml/badge.svg)](https://github.com/larsbrubaker/agg-gui/actions/workflows/ci.yml)
[![Demo](https://github.com/larsbrubaker/agg-gui/actions/workflows/deploy-demo.yml/badge.svg)](https://larsbrubaker.github.io/agg-gui/)

## Live Demo

> **[Open interactive WASM demo →](https://larsbrubaker.github.io/agg-gui/)**

*A hero screenshot will be added here as the demo progresses.*

## Design Principles

- **Direct-to-GL rendering** — AGG paths rasterize directly to the GL surface; no shared RGBA framebuffer for the whole scene
- **Y-up coordinate system** — first-quadrant origin at bottom-left everywhere; a single conversion at event ingestion handles OS Y-down input
- **Full redraw every frame** — no dirty regions or partial updates
- **Widget back-buffers** — widgets like `TextWidget` may cache to a local pixel buffer, invalidating on content change
- **Clipping via Clipper2** — clip regions are computed with [clipper2-rust](https://crates.io/crates/clipper2-rust)

## Workspace

| Crate | Description |
|---|---|
| `agg-gui` | Core library — `GfxCtx`, `Framebuffer`, `Color`, geometry |
| `demo-native` | Windows WGL demo (winit 0.30 + glutin 0.32 + glow 0.13) |
| `demo-wasm` | WASM cdylib deployed to GitHub Pages |
| `demo/` | Frontend — TypeScript + Bun dev server on port 3001 |

## Getting Started

**Prerequisites:** Rust stable, [wasm-pack](https://rustwasm.github.io/wasm-pack/), [Bun](https://bun.sh/)

### Native demo

```sh
cargo run -p demo-native
```

### WASM dev server

```sh
# Build WASM (first time or after Rust changes)
wasm-pack build demo-wasm --dev --target web --out-dir ../demo/public/pkg --no-typescript

# Install JS deps (first time)
cd demo && bun install

# Start dev server with live reload on http://localhost:3001
bun run server.ts
```

### Run tests

```sh
cargo test -p agg-gui
```

## Development

Local path overrides for the underlying libraries are available by uncommenting the `[patch.crates-io]` block in the workspace `Cargo.toml`:

```toml
[patch.crates-io]
agg-rust = { path = "../../agg-rust" }
clipper2-rust = { path = "../../clipper2-rust" }
```

## Roadmap

- **Phase 1** ✅ — Framebuffer, AGG bridge, GL presentation, WASM demo
- **Phase 2** — Full `GfxCtx`: state stack, transforms, Clipper2 clipping, blend modes
- **Phase 3** — Text rendering (`fontdb` + `rustybuzz`)
- **Phase 4** — Widget base, event system, `Button`, `TextField`
- **Phase 5** — Layout widgets: `FlexRow`, `FlexColumn`, `ScrollView`, `Splitter`, `TabView`
- **Phase 6** — `TreeView` with drag-and-drop

The node graph widget is developed in a separate repository that depends on `agg-gui`.

## License

MIT
