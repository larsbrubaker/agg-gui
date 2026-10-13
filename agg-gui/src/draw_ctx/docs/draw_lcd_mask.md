Composite a pre-rasterized LCD subpixel mask onto the current
render target, mixing `src_color` into the destination through
per-channel coverage.

`mask` is three bytes per pixel (`cov_r`, `cov_g`, `cov_b`) as
produced by [`crate::text_lcd::rasterize_lcd_mask`].  The caller
specifies `(dst_x, dst_y)` in local coordinates (Y-up in our
convention) and `mask_w × mask_h` to tell the backend the mask's
dimensions.

Per-channel source-over blend:
```text
dst.r = src.r * mask.r + dst.r * (1 - mask.r)
dst.g = src.g * mask.g + dst.g * (1 - mask.g)
dst.b = src.b * mask.b + dst.b * (1 - mask.b)
```

**This is the universal "composite LCD text onto arbitrary bg"
primitive** — it replaces the prior walk / sample / pre-fill
approach.  Software ctx implements it as an inner-loop blend; the
GL ctx implements it via a dual-source-blend fragment shader.
Backends that haven't wired it yet use the default no-op, which
makes callers fall back to grayscale AA.
