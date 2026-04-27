use super::*;

pub(crate) fn rasterize_linear_gradient_fill(
    fb: &mut Framebuffer,
    path: &mut PathStorage,
    gradient: &LinearGradientPaint,
    global_alpha: f32,
    mode: CompOp,
    clip: Option<(f64, f64, f64, f64)>,
    fill_rule: FillRule,
    transform: &TransAffine,
) {
    rasterize_sampled_fill(
        fb,
        path,
        |x, y| gradient.sample(x, y),
        global_alpha,
        mode,
        clip,
        fill_rule,
        transform,
    );
}

pub(crate) fn rasterize_radial_gradient_fill(
    fb: &mut Framebuffer,
    path: &mut PathStorage,
    gradient: &RadialGradientPaint,
    global_alpha: f32,
    mode: CompOp,
    clip: Option<(f64, f64, f64, f64)>,
    fill_rule: FillRule,
    transform: &TransAffine,
) {
    rasterize_sampled_fill(
        fb,
        path,
        |x, y| gradient.sample(x, y),
        global_alpha,
        mode,
        clip,
        fill_rule,
        transform,
    );
}

pub(crate) fn rasterize_pattern_fill(
    fb: &mut Framebuffer,
    path: &mut PathStorage,
    pattern: &PatternPaint,
    global_alpha: f32,
    mode: CompOp,
    clip: Option<(f64, f64, f64, f64)>,
    fill_rule: FillRule,
    transform: &TransAffine,
) {
    rasterize_sampled_fill(
        fb,
        path,
        |x, y| pattern.sample(x, y),
        global_alpha,
        mode,
        clip,
        fill_rule,
        transform,
    );
}

fn rasterize_sampled_fill<F>(
    fb: &mut Framebuffer,
    path: &mut PathStorage,
    mut sample: F,
    global_alpha: f32,
    _mode: CompOp,
    clip: Option<(f64, f64, f64, f64)>,
    fill_rule: FillRule,
    transform: &TransAffine,
) where
    F: FnMut(f64, f64) -> Color,
{
    let mut mask_fb = Framebuffer::new(fb.width(), fb.height());
    let white = Color::white().to_rgba8();
    rasterize_fill(
        &mut mask_fb,
        path,
        &white,
        CompOp::SrcOver,
        clip,
        fill_rule,
        transform,
    );

    let width = fb.width() as usize;
    let height = fb.height() as usize;
    let mask = mask_fb.pixels();
    let dst = fb.pixels_mut();

    for y in 0..height {
        for x in 0..width {
            let i = (y * width + x) * 4;
            let coverage = mask[i + 3] as f32 / 255.0;
            if coverage <= 0.0 {
                continue;
            }

            let mut lx = x as f64 + 0.5;
            let mut ly = y as f64 + 0.5;
            transform.inverse_transform(&mut lx, &mut ly);
            let mut src = sample(lx, ly);
            src.a *= coverage * global_alpha;

            let sa = src.a.clamp(0.0, 1.0);
            if sa <= 0.0 {
                continue;
            }
            let inv_sa = 1.0 - sa;
            let sr = src.r.clamp(0.0, 1.0) * sa;
            let sg = src.g.clamp(0.0, 1.0) * sa;
            let sb = src.b.clamp(0.0, 1.0) * sa;
            let da = dst[i + 3] as f32 / 255.0;

            dst[i] =
                ((sr + (dst[i] as f32 / 255.0) * inv_sa) * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
            dst[i + 1] =
                ((sg + (dst[i + 1] as f32 / 255.0) * inv_sa) * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
            dst[i + 2] =
                ((sb + (dst[i + 2] as f32 / 255.0) * inv_sa) * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
            dst[i + 3] = ((sa + da * inv_sa) * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
        }
    }
}
