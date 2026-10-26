//! `ImageSampling` — how an image blit picks colours when the image is drawn
//! at a size other than its own, the choice a caller hands to
//! [`DrawCtx::draw_image_rgba_arc_sampled`](super::DrawCtx::draw_image_rgba_arc_sampled).
//! Re-exported from `draw_ctx.rs`; each backend maps it onto its own sampler
//! (`agg-gui-wgpu`'s `image_blit.rs`, the software `gfx_ctx/axis_blit.rs`).

/// How an image blit samples its pixels.
///
/// `Auto` is what `draw_image_rgba_arc` does: the backend decides. The wgpu
/// backend draws images under 256 px on their longest side with hard-edged
/// pixels (backbuffers and icons blitted 1:1 stay crisp) and larger ones
/// smoothly with mipmaps (screenshots shrunk into a preview stay clean); the
/// software backend always draws hard-edged pixels.
///
/// `Nearest` asks for hard-edged pixels at any image size: a magnified image
/// shows each of its pixels as a solid square, so a user zooming in can see
/// exactly which pixels an image holds. Drawn smaller than its own size, a
/// large image may take each colour from one of its box-filtered mip levels
/// on the GPU; each screen pixel is still one texel's colour.
///
/// `Smooth` asks for blended (bilinear) sampling at any image size, for a
/// small image drawn larger without visible pixel steps. Only images of 256 px
/// or more carry a mip chain, so a small image drawn much smaller than its own
/// size can alias.
///
/// Backends that cannot choose (test recorders, the LCD coverage context)
/// draw every mode as `Auto`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ImageSampling {
    /// The backend's own choice; what `draw_image_rgba_arc` draws with.
    #[default]
    Auto,
    /// Hard-edged pixels: each screen pixel takes one image pixel's colour.
    Nearest,
    /// Bilinear blending between neighbouring image pixels.
    Smooth,
}
