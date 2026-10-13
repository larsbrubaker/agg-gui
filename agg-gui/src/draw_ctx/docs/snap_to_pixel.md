**Opt-in** pixel snapping.  Strips the fractional part of the current
CTM translation so subsequent integer-coordinate `rect` / `fill` /
`stroke` / `draw_image_rgba*` calls land exactly on the physical pixel
grid — no AA fringe on edges, no LINEAR-filter blur on 1:1 texture
blits.

Call this ONLY when the widget genuinely wants pixel-aligned drawing
(text backbuffers, pixel-alignment diagnostics, crisp UI strokes).
Sub-pixel positioning remains the default — e.g. a smooth-scrolling
panel or an animated marker may legitimately want a fractional offset.
Typical usage:
```ignore
ctx.save();
ctx.snap_to_pixel();
ctx.rect(0.0, 0.0, 10.0, 10.0);
ctx.fill();
ctx.restore();
```

Only the translation component is affected; rotations and non-uniform
scales pass through untouched (pixel alignment under those transforms
isn't well defined, and forcing a snap would visibly jitter rotated
content).
