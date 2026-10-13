Append a circular arc with the pen down, like HTML canvas `arc()`.

After an open subpath (a `move_to` or any later segment) the arc
continues it: a straight line runs from the current point to the arc's
start, then the arc follows, so `move_to` / `line_to` / `arc_to` /
`close_path` builds one contour (a rounded corner fills and strokes as
part of its outline).  On an empty path or right after `close_path` the
arc starts a new subpath at its start point.  Angles are radians from
+x; `ccw == true` sweeps towards increasing angle (counter-clockwise in
Y-up space).
