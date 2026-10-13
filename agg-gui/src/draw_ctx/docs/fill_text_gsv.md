**Do not call this from application code, ever.**

This is the built-in AGG Glyph-Stroke-Vector fallback font — a
stroked vector typeface that pre-dates AGG's real text stack. It
**bypasses every text-rendering facility this framework offers**:

- no font shaping (no kerning, no proper metrics, no UTF-8 fallback),
- no backbuffer caching (rasterised every frame from scratch),
- no LCD subpixel rendering (always grayscale outlines),
- no theme integration (ignores `Visuals::text_color`),
- no integration with [`crate::font_settings`] (system font,
  font-size scale, hinting, gamma, etc. all ignored).

It exists only as an internal bootstrap path so the framework can
draw the very first frame before a real [`crate::text::Font`] has
loaded, and so a handful of `agg-rust` reference demos that
specifically test the GSV path stay reproducible. Outside those
two contexts there is **no situation where calling this is
correct** — including for "quick debug text," diagnostics
overlays, perf labels, FPS counters, watermarks, anything.

Use a [`crate::widgets::Label`] widget instead. `Label` is
backbuffer-cached by default and uses LCD subpixel rendering
when the global toggle is on (which itself defaults to "on at
standard DPI, off at HiDPI" via [`crate::font_settings::lcd_enabled`]).
If you genuinely need imperative text inside a custom widget's
`paint`, call [`set_font`](Self::set_font) + [`fill_text`](Self::fill_text)
— that goes through the real text stack.

If you find yourself reaching for `fill_text_gsv`, you are
almost certainly looking at a bug in the calling code; do not
add it as a workaround. File an issue against agg-gui.
