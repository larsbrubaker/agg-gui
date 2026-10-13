Submit **pre-tessellated** AA triangles with per-vertex coverage
(`x`, `y`, `alpha`) and triangle indices.

This is the fast path for callers that tessellate their geometry
ONCE at load time (e.g. the Lion demo, SVG icons): they do the
`tessellate_path_aa` pass themselves, cache the vertex+index
buffers, then submit them every frame with only a cheap CPU
transform applied to the x/y components.  Compared to issuing
`move_to` / `line_to` / `fill` every frame, this keeps the polygon
set deterministic (no tess2 re-running on subtly-different
coordinates), avoids thousands of re-tessellations per frame, and
produces identical output regardless of the widget's transform.

Vertices are `(x_logical_pixels, y_logical_pixels, alpha_0_to_1)`.
`alpha` is multiplied into the supplied `color.a` in the AA shader
so halo-strip edge AA survives this fast path.

The software `GfxCtx` ignores the alpha attribute and rasterises
each triangle as a solid fill — correct but without edge AA, which
matches the software path's existing stroke/fill behaviour.
