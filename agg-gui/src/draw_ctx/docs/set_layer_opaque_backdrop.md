Declare that the current layer is now covered by opaque content, so
text drawn into it lands on destination alpha 1.

This re-enables LCD subpixel text inside the layer. Subpixel
coverage is only meaningful against an opaque destination: a fresh
layer texture is cleared to alpha 0, and the per-channel composite
writes colour without alpha, so glyphs over a transparent layer
would stay at alpha 0 and blend additively toward white on pop.
Once an opaque body has been painted, that hazard is gone and
subpixel rendering is as correct as it is on the backbuffer.

Call it *after* filling the layer, not at push time. Widgets that
composite genuinely translucent content (an opacity fade) must not
call it; they keep the grayscale, alpha-writing text path.

No-op by default and at the top level, where the target is already
opaque.
