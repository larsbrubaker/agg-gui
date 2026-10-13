Map from the current local coordinates to the ROOT render target's
**device pixels** (Y-up), even while drawing inside offscreen layers
(compositing layers, clip-path layers) whose local CTM was reset or
shifted.

The result applies the current in-layer CTM first, then adds every
enclosing layer's origin (each recorded in its parent's device
pixels), so it includes the App's effective scale (device × UX, see
[`crate::ux_scale::effective_scale`]), the on-screen keyboard's lift
translate and every layer offset — it is the ON-SCREEN position.
Callers that want root logical coordinates (the unlifted layout space
of events and the overlay queues) use
[`crate::widget::logical_root_transform`] rather than dividing by hand
(dividing by the device scale alone breaks whenever UX scale ≠ 1).
Global overlays and the paint clip stack
(`crate::widget::is_local_rect_in_paint_clip`) use this to reason
about app-level bounds.

"Root" is only the true root while painting through the frame's own
context. Inside a CPU-backbuffered subtree (`paint_subtree_backbuffered`
paints into a fresh `GfxCtx` / `LcdGfxCtx`) it is relative to the
backbuffer bitmap, and `LcdGfxCtx::push_layer` resets the CTM to
identity, so inside an LCD layer the effective scale is not included.
