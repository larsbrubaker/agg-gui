Same as [`draw_image_rgba_arc`] but with an explicit [`ImageSampling`]:
`Auto` draws exactly as `draw_image_rgba_arc` does, `Nearest` draws
hard-edged pixels at any image size (a picture the user zooms into shows
its real pixels as squares), `Smooth` blends neighbouring pixels at any
image size.

The GPU texture cache is keyed on the `Arc` alone, so drawing the same image
with different modes, or every frame, uploads it once; whether it carries a
mip chain depends only on its size.

The destination rect may be far larger than the visible area (a zoomed
picture inside a clip rect): only the pixels inside the active clip and the
target are sampled, so the cost follows what is visible, not the zoom.

Default implementation: ignore `sampling` and call [`draw_image_rgba_arc`],
so backends without a sampler choice keep drawing as before. The wgpu and
software `GfxCtx` backends override it.
