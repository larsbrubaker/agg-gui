Blit `data` as a textured quad whose four destination corners
are supplied explicitly. Caller is responsible for choosing the
corners (typically the projection of a 3-D rotated card onto
the 2-D viewport). `corners` is ordered **bottom-left,
bottom-right, top-right, top-left** in agg-gui's Y-up local
coordinate space, and is fed through the current CTM the same
way axis-aligned blits are.

Corner quads are always sampled with **linear** filtering,
whatever the image size, so callers drawing rotated or scaled
sprites get smooth edges instead of the jagged, shimmering ones
point sampling would give. (The axis-aligned
[`DrawCtx::draw_image_rgba_arc`] path keeps its crisp 1:1
sampling.)

Backends that can't render a perspective-distorted quad
(software fallback) fall back on the axis-aligned bounding
rect of the four corners.
