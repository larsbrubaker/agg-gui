use super::*;

pub(super) fn draw_panel(ctx: &mut dyn DrawCtx, x: f64, y: f64, w: f64, h: f64, v: &Visuals) {
    ctx.set_fill_color(v.panel_fill);
    ctx.begin_path();
    ctx.rounded_rect(x, y, w, h, 5.0);
    ctx.fill();
    ctx.set_stroke_color(Color::rgba(
        v.text_color.r,
        v.text_color.g,
        v.text_color.b,
        0.18,
    ));
    ctx.set_line_width(1.0);
    ctx.begin_path();
    ctx.rounded_rect(x, y, w, h, 5.0);
    ctx.stroke();
}

pub(super) fn draw_raster_column(
    ctx: &mut dyn DrawCtx,
    pixels: &Result<Arc<Vec<u8>>, String>,
    img_w: u32,
    img_h: u32,
    zoom: f64,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    v: &Visuals,
) {
    match pixels {
        Ok(pixels) => {
            let (dx, dy, dw, dh) =
                native_rect(img_w as f64 * zoom, img_h as f64 * zoom, x, y, w, h);
            ctx.draw_image_rgba_arc(pixels, img_w, img_h, dx, dy, dw, dh);
        }
        Err(err) => draw_small_text(ctx, err, x + 8.0, y + h * 0.5, 9.0, v.text_dim),
    }
}

pub(super) fn draw_lcd_column(
    ctx: &mut dyn DrawCtx,
    pixels: &Result<SvgLcdPreview, String>,
    img_w: u32,
    img_h: u32,
    zoom: f64,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    v: &Visuals,
) {
    match pixels {
        Ok(pixels) => {
            let (dx, dy, dw, dh) =
                native_rect(img_w as f64 * zoom, img_h as f64 * zoom, x, y, w, h);
            ctx.draw_lcd_backbuffer_arc(&pixels.color, &pixels.alpha, img_w, img_h, dx, dy, dw, dh);
        }
        Err(err) => draw_small_text(ctx, err, x + 8.0, y + h * 0.5, 9.0, v.text_dim),
    }
}

pub(super) fn draw_hardware_column(
    ctx: &mut dyn DrawCtx,
    sample: &SvgSampleRender,
    zoom: f64,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    v: &Visuals,
) {
    let (dx, dy, _, _) = native_rect(
        sample.width as f64 * zoom,
        sample.height as f64 * zoom,
        x,
        y,
        w,
        h,
    );
    ctx.save();
    ctx.translate(dx, dy);
    ctx.scale(zoom, zoom);
    if let Err(err) = render_svg_at_size(sample.svg, ctx, sample.width, sample.height) {
        ctx.restore();
        draw_small_text(ctx, &err.to_string(), x + 8.0, y + h * 0.5, 9.0, v.text_dim);
        return;
    }
    ctx.restore();
}

pub(super) fn native_rect(
    src_w: f64,
    src_h: f64,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> (f64, f64, f64, f64) {
    (x + (w - src_w) * 0.5, y + (h - src_h) * 0.5, src_w, src_h)
}

pub(super) fn draw_small_text(
    ctx: &mut dyn DrawCtx,
    text: &str,
    x: f64,
    y: f64,
    size: f64,
    color: Color,
) {
    ctx.set_font_size(size);
    ctx.set_fill_color(color);
    ctx.fill_text(text, x, y);
}

pub(super) fn decode_png_rgba(data: &[u8]) -> Result<(Vec<u8>, u32, u32), String> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(data));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0_u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    let src = &buf[..info.buffer_size()];
    let rgba = match info.color_type {
        png::ColorType::Rgba => src.to_vec(),
        png::ColorType::Rgb => {
            let mut out = Vec::with_capacity(info.width as usize * info.height as usize * 4);
            for chunk in src.chunks_exact(3) {
                out.extend_from_slice(chunk);
                out.push(255);
            }
            out
        }
        png::ColorType::Grayscale => {
            let mut out = Vec::with_capacity(info.width as usize * info.height as usize * 4);
            for &v in src {
                out.extend_from_slice(&[v, v, v, 255]);
            }
            out
        }
        png::ColorType::GrayscaleAlpha => {
            let mut out = Vec::with_capacity(info.width as usize * info.height as usize * 4);
            for chunk in src.chunks_exact(2) {
                out.extend_from_slice(&[chunk[0], chunk[0], chunk[0], chunk[1]]);
            }
            out
        }
        other => return Err(format!("unsupported PNG color type: {other:?}")),
    };

    Ok((rgba, info.width, info.height))
}

pub(super) fn rgba_matches_reference(rendered: &[u8], reference: &[u8]) -> bool {
    agg_gui::compare_svg_rgba(
        rendered,
        reference,
        agg_gui::SvgCompareThresholds::default(),
    )
    .pass
}
