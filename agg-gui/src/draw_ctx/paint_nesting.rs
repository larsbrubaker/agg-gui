//! [`PaintNesting`]: how many `save()` levels and layers a [`DrawCtx`](super::DrawCtx)
//! has open, read and restored by paint containment
//! (`widget/paint/contain.rs`) through `DrawCtx::paint_nesting` and
//! `DrawCtx::unwind_paint_nesting`. Re-exported from `draw_ctx`.

/// How deep a context's `save` and layer stacks are right now (see
/// [`DrawCtx::paint_nesting`](super::DrawCtx::paint_nesting)): `layers` is the number of open compositing
/// (and clip) layers, `saves` the number of open `save()` levels in the
/// innermost layer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PaintNesting {
    pub layers: usize,
    pub saves: usize,
}
