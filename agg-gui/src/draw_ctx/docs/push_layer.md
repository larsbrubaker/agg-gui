Begin a new transparent compositing layer of the given pixel dimensions.

All subsequent drawing (by this widget and its descendants) is redirected
into the new layer until [`pop_layer`] is called.  Layers nest: each
`push_layer` must be matched by exactly one `pop_layer`.

The current accumulated transform records the layer's screen-space origin;
drawing inside the layer uses a fresh local-space transform (origin 0,0).

Implementations that do not support layers (e.g. the GL path) may leave
this as a no-op — the widget renders pass-through into the parent target.
