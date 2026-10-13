Draw raw RGBA pixel data into `dst_rect` (Y-up local coordinates).

`data` must be `img_w * img_h * 4` bytes of tightly-packed RGBA8 data
in row-major order, **top-row first** (Y-down image storage convention).
The image is scaled to fit `(dst_x, dst_y, dst_w, dst_h)`, a rect in
LOCAL (logical) units that every backend maps through the current
transform: under a HiDPI `scale(2, 2)` a logical 10×10 rect covers 20×20
device pixels, so a 20×20 image (e.g. a CPU backbuffer rasterised at
physical resolution) blits texel-for-pixel.

**Alpha convention: straight (non-premultiplied).**  Every backend
treats `data` as straight alpha: the software [`crate::GfxCtx`]
premultiplies each sample before its premultiplied src-over
composite, and the wgpu backend uploads the texels premultiplied and
blends them with `One / OneMinusSrcAlpha`.  Render targets themselves
([`crate::Framebuffer`], layers) store **premultiplied** RGBA, so a
buffer rendered by agg-gui goes through
[`crate::framebuffer::unpremultiply_rgba_inplace`] (after
[`crate::Framebuffer::pixels_flipped`]) before it is passed here.
Colour operations defined on straight alpha (tinting, lightness
inversion) can therefore be applied directly to `data`.  The same
convention holds for `draw_image_rgba_arc` and
`draw_image_rgba_corners`.

Default implementation: no-op (GL path or software paths that do not
implement blitting can leave this as a placeholder).
