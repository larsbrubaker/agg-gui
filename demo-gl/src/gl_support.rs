use glow::HasContext;

pub(crate) unsafe fn compile_program(
    gl: &glow::Context,
    vert_src: &str,
    frag_src: &str,
) -> Result<glow::Program, String> {
    let prog = gl
        .create_program()
        .map_err(|e| format!("create_program: {e}"))?;
    for (src, kind) in [
        (vert_src, glow::VERTEX_SHADER),
        (frag_src, glow::FRAGMENT_SHADER),
    ] {
        let sh = gl
            .create_shader(kind)
            .map_err(|e| format!("create_shader: {e}"))?;
        gl.shader_source(sh, src);
        gl.compile_shader(sh);
        if !gl.get_shader_compile_status(sh) {
            let log = gl.get_shader_info_log(sh);
            gl.delete_shader(sh);
            gl.delete_program(prog);
            return Err(format!("shader compile error: {log}"));
        }
        gl.attach_shader(prog, sh);
        gl.delete_shader(sh);
    }
    gl.link_program(prog);
    if !gl.get_program_link_status(prog) {
        let log = gl.get_program_info_log(prog);
        gl.delete_program(prog);
        return Err(format!("program link error: {log}"));
    }
    Ok(prog)
}

/// Compute a cache key for an RGBA image slice. Blends pointer, length,
/// dimensions, and first/last 8 bytes so a freed-and-reused pointer with fresh
/// content produces a different key. Cheap: no full-buffer hash.
pub(crate) fn texture_key(data: &[u8], w: u32, h: u32) -> u64 {
    let mut k: u64 = 0xcbf29ce484222325;
    let mix = |acc: u64, v: u64| -> u64 { acc.wrapping_mul(0x100000001b3).wrapping_add(v) };
    k = mix(k, data.as_ptr() as usize as u64);
    k = mix(k, data.len() as u64);
    k = mix(k, w as u64);
    k = mix(k, h as u64);
    if data.len() >= 16 {
        for &b in &data[..8] {
            k = mix(k, b as u64);
        }
        for &b in &data[data.len() - 8..] {
            k = mix(k, b as u64);
        }
    } else {
        for &b in data {
            k = mix(k, b as u64);
        }
    }
    k
}

/// Convert a Y-up screen-space bounding box to a GL scissor rectangle.
///
/// `gl.scissor(x, y, w, h)` uses window coordinates where y=0 is the bottom of
/// the framebuffer, identical to Y-up screen space.
pub(crate) fn compute_gl_scissor(lx: f64, by: f64, rx: f64, ty2: f64) -> [i32; 4] {
    // Clamp before casting: STRETCH-anchored children inside a ScrollView receive
    // f64::MAX/2 as available height during the measure pass, which would
    // overflow i32 when fed into GL scissor calls.
    const LO: f64 = i32::MIN as f64;
    const HI: f64 = i32::MAX as f64;
    let gl_x = lx.floor().clamp(LO, HI) as i32;
    let gl_y = by.floor().clamp(LO, HI) as i32;
    let gl_w = (rx - lx).ceil().clamp(0.0, HI) as i32;
    let gl_h = (ty2 - by).ceil().clamp(0.0, HI) as i32;
    [gl_x, gl_y, gl_w, gl_h]
}

/// Expand `contours` into stroke quads (two triangles per segment) with the
/// given half-width.
///
/// No longer used at runtime. Retained so regression tests keep documenting the
/// behavior replaced by AGG `ConvStroke` plus tess2.
#[cfg(test)]
fn build_stroke_quads(contours: &[Vec<[f32; 2]>], hw: f32) -> (Vec<[f32; 2]>, Vec<u32>) {
    let mut verts: Vec<[f32; 2]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    for contour in contours {
        if contour.len() < 2 {
            continue;
        }
        let n = contour.len();
        for i in 0..n {
            let a = contour[i];
            let b = contour[(i + 1) % n];
            if i + 1 == n && contour.first() != contour.last() {
                break;
            }
            let dx = b[0] - a[0];
            let dy = b[1] - a[1];
            let len = (dx * dx + dy * dy).sqrt();
            if len < 1e-6 {
                continue;
            }
            let nx = -dy / len * hw;
            let ny = dx / len * hw;

            let base = verts.len() as u32;
            verts.push([a[0] + nx, a[1] + ny]);
            verts.push([a[0] - nx, a[1] - ny]);
            verts.push([b[0] + nx, b[1] + ny]);
            verts.push([b[0] - nx, b[1] - ny]);
            indices.extend_from_slice(&[base, base + 1, base + 2, base + 1, base + 3, base + 2]);
        }
    }

    (verts, indices)
}

#[cfg(test)]
mod tests {
    use super::{build_stroke_quads, compute_gl_scissor};

    #[test]
    fn test_stroke_open_rect_missing_left_side() {
        let contour = vec![[0.0f32, 0.0f32], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        let (verts, _) = build_stroke_quads(&[contour], 0.5);
        assert_eq!(verts.len() / 4, 3);
    }

    #[test]
    fn test_stroke_closed_rect_has_all_four_sides() {
        let contour = vec![
            [0.0f32, 0.0f32],
            [10.0, 0.0],
            [10.0, 10.0],
            [0.0, 10.0],
            [0.0, 0.0],
        ];
        let (verts, _) = build_stroke_quads(&[contour], 0.5);
        assert_eq!(verts.len() / 4, 4);
    }

    #[test]
    fn test_scissor_y_uses_y_up_bottom_not_y_down_top() {
        let [_gl_x, gl_y, _gl_w, gl_h] = compute_gl_scissor(0.0, 184.0, 320.0, 650.0);
        assert_eq!(
            gl_y, 184,
            "gl_y must equal the Y-up bottom of the clip, not viewport_h - top"
        );
        assert_eq!(gl_h, 466);
    }

    #[test]
    fn test_scissor_covers_top_tree_rows() {
        let [_, gl_y, _, gl_h] = compute_gl_scissor(0.0, 184.0, 320.0, 650.0);
        let scissor_top = gl_y + gl_h;
        assert!(
            scissor_top >= 650,
            "scissor top ({scissor_top}) must reach y=650 to include top tree rows"
        );
    }
}
