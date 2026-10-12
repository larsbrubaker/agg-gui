//! The swap-chain format choice every shell makes, native (`gpu.rs`'s
//! `Gpu::new`) and browser (agg-gui-web-shell's canvas configure) alike —
//! agg-sharp's `WebGpuRenderDevice.PickSurfaceFormat`.
//!
//! Pure, and built on every target, so both shells share one rule and the
//! choice is testable without a live surface. Its tests port agg-sharp's
//! `WebGpuSurfaceAcquireTests` format cases
//! (`Tests/Agg.Tests/Agg.WebGpuRender/WebGpuSurfaceAcquireTests.cs`).

/// Choose the surface format from the capability list (`caps.formats`).
///
/// 1. `Bgra8Unorm` whenever it is offered: the format the headless frame
///    renders in (`HEADLESS_FORMAT` on native), so a golden image and
///    the window — or a browser capture, whose canvas lists the two 8-bit
///    orders in a platform-dependent order — are the same pixels.
/// 2. Otherwise any non-sRGB format: the 2-D stack writes gamma-encoded
///    bytes, so an sRGB view would encode them a second time.
/// 3. Otherwise (an all-sRGB surface) the surface's own first preference.
///
/// `None` for an empty list: that adapter cannot present to that surface, and
/// inventing a format would only move the failure into wgpu validation.
pub fn choose_surface_format(formats: &[wgpu::TextureFormat]) -> Option<wgpu::TextureFormat> {
    if formats.contains(&wgpu::TextureFormat::Bgra8Unorm) {
        return Some(wgpu::TextureFormat::Bgra8Unorm);
    }
    formats
        .iter()
        .copied()
        .find(|f| !f.is_srgb())
        .or_else(|| formats.first().copied())
}

#[cfg(test)]
mod tests {
    use super::choose_surface_format;
    use wgpu::TextureFormat as F;

    #[test]
    fn bgra8_is_preferred_and_srgb_is_avoided() {
        // Bgra8Unorm keeps the golden images the same pixels the window shows.
        assert_eq!(
            choose_surface_format(&[F::Rgba8Unorm, F::Bgra8Unorm]),
            Some(F::Bgra8Unorm)
        );
        // Without Bgra8Unorm, any non-sRGB format beats the surface's own first
        // preference: the 2D stack already writes gamma-encoded bytes, so an
        // sRGB view would encode them a second time.
        assert_eq!(
            choose_surface_format(&[F::Bgra8UnormSrgb, F::Rgba8Unorm]),
            Some(F::Rgba8Unorm)
        );
        // All-sRGB surface: nothing better exists, so take the surface's first
        // preference.
        assert_eq!(
            choose_surface_format(&[F::Bgra8UnormSrgb, F::Rgba8UnormSrgb]),
            Some(F::Bgra8UnormSrgb)
        );
    }

    #[test]
    fn a_browser_shaped_capability_list_still_lands_on_bgra8() {
        // What a canvas offers: the two 8-bit RGBA orders, preferred format
        // first, and which one is first depends on the platform the browser is
        // running on. Both orders must still choose Bgra8Unorm.
        assert_eq!(
            choose_surface_format(&[F::Bgra8Unorm, F::Rgba8Unorm]),
            Some(F::Bgra8Unorm)
        );
        assert_eq!(
            choose_surface_format(&[F::Rgba8Unorm, F::Bgra8Unorm, F::Rgba16Float]),
            Some(F::Bgra8Unorm)
        );
    }

    #[test]
    fn rust_only_an_empty_capability_list_chooses_nothing() {
        assert_eq!(choose_surface_format(&[]), None);
    }
}
