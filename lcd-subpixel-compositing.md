# LCD Subpixel Rasterization with Preserved Alpha for Arbitrary Compositing

This is a genuinely interesting problem, and the core difficulty is well-known in the typography world: standard LCD subpixel rendering bakes the background color into the coverage values, which is why ClearType-style text traditionally requires knowing the destination color at rasterization time. To make it composable, you need to carry per-channel coverage all the way through to the final blend.

## The Core Insight

Standard alpha compositing uses a single coverage value per pixel. LCD subpixel rendering produces *three* coverage values per pixel (one per subpixel: R, G, B). The trick is to stop thinking of your output as "RGB color + alpha" and start thinking of it as **three independent coverage masks** that drive a per-channel Porter-Duff "over" operation at composite time.

The compositing equation you want is essentially a per-channel version of source-over:

```
dst.r = src.r * cov.r + dst.r * (1 - cov.r)
dst.g = src.g * cov.g + dst.g * (1 - cov.g)
dst.b = src.b * cov.b + dst.b * (1 - cov.b)
```

Where `src` is whatever color you want the shape to be (solid, gradient, pattern, doesn't matter) and `cov.{r,g,b}` are the three subpixel coverage values. This is the formulation Skia uses for its LCD glyph cache, and it's what makes the result composable onto arbitrary backgrounds.

## The Pipeline

**1. Rasterize at 3× horizontal resolution.**
Run AGG (or whatever rasterizer) with the path scaled 3× in X. So a target 100×40 pixel output becomes a 300×40 coverage buffer. Each output pixel will eventually consume three input coverage samples. You can do 3× vertical too for "vertical RGB" panels but horizontal is the common case.

**2. Generate a single-channel coverage buffer.**
At this stage you have an 8-bit (or higher) grayscale alpha buffer at 3× width. AGG's scanline rasterizer gives you exactly this — coverage values 0-255 representing what fraction of each high-res pixel the path covers. Don't apply gamma yet, or apply it carefully (more below).

**3. Apply a subpixel filter (this is the critical step).**
You can't just take samples 0, 1, 2 as R, G, B — that produces severe color fringing on edges. You need to low-pass filter across the three subpixels of each output pixel, plus typically one or two samples on each side. The classic FreeType/ClearType filter is a 5-tap filter applied per output channel:

```
For output pixel x, the three coverage values come from
input samples [3x-2, 3x-1, 3x, 3x+1, 3x+2, 3x+3, 3x+4]:

cov.r = (1*s[3x-2] + 2*s[3x-1] + 3*s[3x  ] + 2*s[3x+1] + 1*s[3x+2]) / 9
cov.g = (1*s[3x-1] + 2*s[3x  ] + 3*s[3x+1] + 2*s[3x+2] + 1*s[3x+3]) / 9
cov.b = (1*s[3x  ] + 2*s[3x+1] + 3*s[3x+2] + 2*s[3x+3] + 1*s[3x+4]) / 9
```

The exact weights are tunable — FreeType uses `[0x10, 0x40, 0x70, 0x40, 0x10]` by default; Skia has used `[0x1c, 0x40, 0x68, 0x40, 0x1c]`. Heavier filters reduce color fringing at the cost of sharpness. The constraint is that the weights for each channel must sum to the same total (so that fully-covered regions produce equal R/G/B coverage and remain colorless).

**4. Store the result as an RGB coverage triplet per pixel.**
This is your "preserved alpha" representation — three 8-bit channels where each is an independent coverage value. It is *not* an RGBA image in the conventional sense, and you should not try to collapse it to one. Some people call this "alpha mask" or "LCD mask" format to keep the distinction clear.

**5. Composite per-channel at draw time.**
When you blit, do the three-channel source-over shown above. Your `src` can be a solid color, a gradient sampled at the destination, a texture, anything — because the coverage mask is independent of the fill.

## On Gamma (don't skip this)

Naive linear blending of LCD-rendered text looks wrong, especially for light-on-dark. The principled approach:

1. Convert your source and destination colors from sRGB to linear light.
2. Do the three-channel `src * cov + dst * (1 - cov)` blend in linear space.
3. Convert back to sRGB.

If you can't afford full linearization, the FreeType/ClearType compromise is to apply a gamma correction (~1.8-2.2) to the coverage values themselves before storing them, which approximates correct blending for typical mid-gray backgrounds but produces visible weight shifts on extreme backgrounds. For a general-purpose system intended for arbitrary compositing, do it in linear light — the whole point of preserving the per-channel coverage is to get correct results everywhere.

## Practical Notes

For AGG specifically, you have two reasonable implementation paths. The simpler one is to render the path normally into a 3×-wide grayscale buffer using `agg::renderer_scanline_aa_solid` with a single-channel pixfmt, then run the filter as a post-process. The more integrated approach is to write a custom scanline renderer that consumes AGG's coverage spans directly and emits filtered RGB-coverage output — faster, but more code. Start with the post-process version.

Memory-wise, three 8-bit channels per pixel is fine for most uses. If you're caching glyphs you can compress runs of zero coverage aggressively since glyph bitmaps are mostly empty.

One subtle thing: if you ever need to *transform* the coverage buffer (rotate, scale a cached glyph), the per-channel structure stops being meaningful — the R/G/B assignment is tied to the physical subpixel layout of the destination display. So cache LCD-rendered shapes only at their final orientation, and fall back to grayscale AA for rotated or animated content.
