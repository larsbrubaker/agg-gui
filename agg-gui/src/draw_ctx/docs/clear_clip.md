Replace every pixel inside the current clip with `color`, whatever
the transform (agg-sharp's `Graphics2D.Clear`, which fills the
clipping rect). With no clip set it fills the whole target. The
current path is discarded; every other piece of state is kept.

Implemented with drawing calls every backend has: under an identity
transform, a rectangle far larger than any target is filled in
`CompOp::Src` (copy) at full alpha, which the clip then trims to
exactly its own pixels.
