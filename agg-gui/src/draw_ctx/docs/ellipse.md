Append an elliptical arc to the current path, mirroring canvas-2D
`ellipse(cx, cy, rx, ry, rotation, start_angle, end_angle, ccw)`.

The arc starts with a `move_to` at the start point (callers place it
after `begin_path`, as with [`DrawCtx::circle`]).  Angles are measured
from the +x axis of the ellipse's own — i.e. `rotation`-rotated —
frame.  With `ccw == false` the arc sweeps towards increasing angle,
with `ccw == true` towards decreasing angle.

A **full turn** — the whole ellipse, with the contour closed — is drawn
only when the sweep covers a revolution *in the requested direction*:
`!ccw && end_angle - start_angle >= 2π`, or
`ccw && start_angle - end_angle >= 2π`.  Any other sweep is normalised
into `[0, 2π)` (`!ccw`) or `(-2π, 0]` (`ccw`), matching the canvas-2D
spec: `ellipse(.., 0.0, -2π, false)` is an empty arc, not a full
ellipse.

Implemented on top of [`DrawCtx::cubic_to`] with at most one Bézier
segment per quarter turn (see [`defaults::ellipse_ops`]), so every
backend gets it for free; backends with a native primitive may
override it.
