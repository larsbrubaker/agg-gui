Render GPU content (3-D scene, video frame, etc.) inline at the correct
painter-order position.

`screen_rect` is the widget's screen-space rect in Y-up coordinates
(i.e. `ctx.transform()` origin + `widget.bounds().size`).

The GL implementation executes `painter.gl_paint()` immediately so that
any 2-D widgets painted after this call naturally overdraw the GPU
content — correct back-to-front ordering with no post-frame fixup.

The **software (`GfxCtx`) path is a no-op**: widgets should draw a 2-D
placeholder before calling this method so the software render has
something visible.
