Same as [`draw_image_rgba`] but accepts an `Arc<Vec<u8>>` so the GL
backend can key its texture cache on the `Arc`'s pointer identity and
hold a `Weak` ref for automatic cleanup when the underlying buffer is
dropped — the pattern MatterCAD implements with C# `ConditionalWeakTable`.

Used by `Label` (and future glyph-atlas consumers) in tandem with the
crate-level [`image_cache`](crate::image_cache) so that rebuilt widget
trees with unchanged content never re-rasterize OR re-upload.

Default implementation: forward to [`draw_image_rgba`] via slice
borrow.  Software backends don't benefit from GPU texture caching so
the default is usually fine; the GL backend overrides.

To choose how the image is sampled (hard-edged pixels for a picture the
user zooms into, or smooth), call [`draw_image_rgba_arc_sampled`] with an
[`ImageSampling`]; this method draws as its `Auto`.
