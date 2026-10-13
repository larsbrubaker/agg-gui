Composite a two-plane `LcdCoverage`-mode backbuffer onto the active
render target at `(dst_x, dst_y)` with size `(dst_w, dst_h)` (in
local coords).  Inputs are two `Arc<Vec<u8>>`, each 3 bytes per
pixel, **top-row-first**:

- `color`: premultiplied per-channel RGB.
- `alpha`: per-channel alpha (coverage).

The compositor applies per-channel premultiplied src-over:

```text
dst.ch := src.color_ch + dst.ch * (1 - src.alpha_ch)
```

which preserves LCD subpixel chroma through the cache round-trip.
Used by [`crate::widget::paint_subtree_backbuffered`] when a widget's
[`crate::widget::BackbufferMode::LcdCoverage`] cache is ready to
composite onto its parent.

`content_version` is a monotone, process-globally-unique content
revision (see `crate::widget::next_content_version`) stamped whenever
the published planes change. Backends that key a GPU texture cache on the
`Arc` buffer *identity* pair it with the pointer so an in-place strip edit
(which keeps the buffer address stable through `Arc::make_mut`) still
forces a re-upload. CPU backends ignore it.

**Default:** collapses the two planes into a single straight-alpha
RGBA8 image via [`crate::lcd_coverage::collapse_lcd_pixel`]
(Rec.709 luminance-weighted alpha over the ink's own straight colour)
and forwards to [`draw_image_rgba`].  Lossy of LCD chroma where
the three channel alphas diverge, but luminance-preserving.  Backends
that want full subpixel quality through the cache override this with a
two-texture shader path.

This default is live code, not a stub: any backend without its own
override (`GfxCtx`, `LcdGfxCtx` and wgpu have one) blits through it,
so an LCD backbuffer still lands there, chroma-free.
