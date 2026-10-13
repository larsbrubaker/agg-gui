Intersect the clip with the current path (canvas-2D `clip()`), using
the current fill rule.

Everything drawn afterwards is masked by the path, with anti-aliased
edges, until the [`DrawCtx::restore`] that matches the
[`DrawCtx::save`] preceding this call.  Nests: a `clip_path` inside an
already-clipped region intersects with it.

**The default is a no-op: drawing is NOT clipped at all.**  There is no
bounding-box fallback — a backend that has not implemented path
clipping simply ignores the call, so callers that need to know must ask
[`DrawCtx::supports_clip_path`] and choose their own fallback.
