// comp_op.wgsl — the blend-mode composite of agg-gui-wgpu (see comp_op.rs).
//
// A draw made under a blend mode other than source-over first renders its
// coverage (the shape in opaque white, source-over) into a transparent layer
// the size of the target.  This full-screen pass then writes every pixel of
// the target once, from a copy of the destination, with the SVG compositing
// formulas of agg-rust's `comp_op.rs` (C++ AGG's `comp_op_adaptor_rgba`): the
// source colour at that pixel's cover, exactly as the software `GfxCtx`
// blends a solid span.  The maths runs on the 8-bit values the software
// renderer sees and rounds each channel back to a byte the same way
// (`Rgba8::from_double`), so the GPU and the software renderer agree pixel
// for pixel at the same cover.
//
// Port of agg-sharp `RenderGl/Renderer/CompOpComposite.wgsl`'s role
// (`GpuCompOp.cs`), with the formulas taken from the software blender this
// backend has to match.

struct CompOpUniforms {
    // The premultiplied source colour as bytes (0..255): `Rgba8::multiply`
    // of the straight colour by its alpha, done on the CPU as software does.
    source: vec4<f32>,
    // `agg_rust::comp_op::CompOp` discriminant.
    op: u32,
    // 0 = composite; 1 = copy `dest_tex` through unchanged (the proxy blit).
    mode: u32,
    // 1 when the target encodes sRGB on write: the composite works on the raw
    // bytes, so it decodes its result for the hardware to encode back.
    srgb: u32,
    pad: u32,
}

@group(0) @binding(0) var coverage_tex: texture_2d<f32>;
@group(0) @binding(1) var dest_tex: texture_2d<f32>;
@group(0) @binding(2) var<uniform> u: CompOpUniforms;

// One triangle covering the whole target; the scissor limits it to the clip.
@vertex fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let x = f32((i << 1u) & 2u);
    let y = f32(i & 2u);
    return vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
}

// C++ blender_base::get (`get` is reserved in WGSL) — bytes as 0..1, scaled by cover/255 below a full
// cover, and nothing at cover 0.
fn get_cover(c: vec4<f32>, cover: f32) -> vec4<f32> {
    if (cover <= 0.0) {
        return vec4<f32>(0.0);
    }
    var v = c / 255.0;
    if (cover < 255.0) {
        v = v * (cover / 255.0);
    }
    return v;
}

// C++ clip(rgba&) — alpha into 0..1 first, then each colour into 0..alpha.
fn clip_rgba(c: vec4<f32>) -> vec4<f32> {
    let a = clamp(c.a, 0.0, 1.0);
    return vec4<f32>(clamp(c.r, 0.0, a), clamp(c.g, 0.0, a), clamp(c.b, 0.0, a), a);
}

// Rgba8::multiply — fixed-point a * b / 255.
fn mul8(a: u32, b: u32) -> u32 {
    let t = a * b + 128u;
    return ((t >> 8u) + t) >> 8u;
}

// Rgba8::prelerp — p + q - p * a, in bytes.
fn prelerp8(p: u32, q: u32, a: u32) -> u32 {
    return (p + q - mul8(p, a)) & 255u;
}

fn overlay_calc(dca: f32, sca: f32, da: f32, sa: f32, sada: f32, d1a: f32, s1a: f32) -> f32 {
    if (2.0 * dca <= da) {
        return 2.0 * sca * dca + sca * d1a + dca * s1a;
    }
    return sada - 2.0 * (da - dca) * (sa - sca) + sca * d1a + dca * s1a;
}

fn hard_light_calc(dca: f32, sca: f32, da: f32, sa: f32, sada: f32, d1a: f32, s1a: f32) -> f32 {
    if (2.0 * sca < sa) {
        return 2.0 * sca * dca + sca * d1a + dca * s1a;
    }
    return sada - 2.0 * (da - dca) * (sa - sca) + sca * d1a + dca * s1a;
}

fn color_dodge_calc(dca: f32, sca: f32, da: f32, sa: f32, sada: f32, d1a: f32, s1a: f32) -> f32 {
    if (sca < sa) {
        return sada * min(1.0, (dca / da) * sa / (sa - sca)) + sca * d1a + dca * s1a;
    }
    if (dca > 0.0) {
        return sada + sca * d1a + dca * s1a;
    }
    return sca * d1a;
}

// agg-rust keeps C++'s `dca > da` test for the black-source case.
fn color_burn_calc(dca: f32, sca: f32, da: f32, sa: f32, sada: f32, d1a: f32, s1a: f32) -> f32 {
    if (sca > 0.0) {
        return sada * (1.0 - min(1.0, (1.0 - dca / da) * sa / sca)) + sca * d1a + dca * s1a;
    }
    if (dca > da) {
        return sada + dca * s1a;
    }
    return dca * s1a;
}

fn soft_light_calc(dca: f32, sca: f32, da: f32, sa: f32, sada: f32, d1a: f32, s1a: f32) -> f32 {
    let dcasa = dca * sa;
    if (2.0 * sca <= sa) {
        return dcasa - (sada - 2.0 * sca * da) * dcasa * (sada - dcasa) + sca * d1a + dca * s1a;
    }
    if (4.0 * dca <= da) {
        return dcasa + (2.0 * sca * da - sada) * ((((16.0 * dcasa - 12.0) * dcasa + 4.0) * dca * da) - dca * da)
            + sca * d1a + dca * s1a;
    }
    return dcasa + (2.0 * sca * da - sada) * (sqrt(dcasa) - dcasa) + sca * d1a + dca * s1a;
}

// The separable modes' shared alpha: Da' = Sa + Da - Sa.Da.
fn union_alpha(s: vec4<f32>, d: vec4<f32>) -> f32 {
    return d.a + s.a - s.a * d.a;
}

// One pixel through `op`: source bytes `sb` (premultiplied), destination bytes
// `db`, cover 1..255.  Returns the result as 0..1 before byte rounding.
fn blend(op: u32, sb: vec4<f32>, db: vec4<f32>, cover: f32) -> vec4<f32> {
    let d = db / 255.0;
    let s = get_cover(sb, cover);
    let s1a = 1.0 - s.a;
    let d1a = 1.0 - d.a;
    let sada = s.a * d.a;
    switch op {
        // Clear
        case 0u: {
            if (cover >= 255.0) {
                return vec4<f32>(0.0);
            }
            return get_cover(db, 255.0 - cover);
        }
        // Src
        case 1u: {
            if (cover >= 255.0) {
                return sb / 255.0;
            }
            return get_cover(db, 255.0 - cover) + s;
        }
        // Dst
        case 2u: {
            return d;
        }
        // SrcOver — C++ blender_rgba_pre: integer mult_cover, then prelerp.
        case 3u: {
            let c = u32(cover);
            let ca = mul8(u32(sb.a), c);
            let p = vec4<u32>(db);
            return vec4<f32>(
                f32(prelerp8(p.r, mul8(u32(sb.r), c), ca)),
                f32(prelerp8(p.g, mul8(u32(sb.g), c), ca)),
                f32(prelerp8(p.b, mul8(u32(sb.b), c), ca)),
                f32(prelerp8(p.a, ca, ca))) / 255.0;
        }
        // DstOver
        case 4u: {
            return d + s * d1a;
        }
        // SrcIn
        case 5u: {
            if (d.a > 0.0) {
                return get_cover(db, 255.0 - cover) + s * d.a;
            }
            return d;
        }
        // DstIn
        case 6u: {
            return get_cover(db, 255.0 - cover) + get_cover(db, cover) * (sb.a / 255.0);
        }
        // SrcOut
        case 7u: {
            return get_cover(db, 255.0 - cover) + s * d1a;
        }
        // DstOut
        case 8u: {
            return get_cover(db, 255.0 - cover) + get_cover(db, cover) * (1.0 - sb.a / 255.0);
        }
        // SrcAtop — agg-rust keeps C++'s typo: blue mixes the just-written green.
        case 9u: {
            let r = s.r * d.a + d.r * s1a;
            let g = s.g * d.a + d.g * s1a;
            let b = s.b * d.a + g * s1a;
            return vec4<f32>(r, g, b, d.a);
        }
        // DstAtop
        case 10u: {
            let sa = sb.a / 255.0;
            let rest = get_cover(db, 255.0 - cover);
            let rgb = rest.rgb + get_cover(db, cover).rgb * sa + s.rgb * d1a;
            return vec4<f32>(rgb, rest.a + s.a);
        }
        // Xor
        case 11u: {
            return vec4<f32>(s.rgb * d1a + d.rgb * s1a, s.a + d.a - 2.0 * s.a * d.a);
        }
        // Plus
        case 12u: {
            if (s.a <= 0.0) {
                return d;
            }
            let a = min(d.a + s.a, 1.0);
            return clip_rgba(vec4<f32>(min(d.rgb + s.rgb, vec3<f32>(a)), a));
        }
        // Minus
        case 13u: {
            if (s.a <= 0.0) {
                return d;
            }
            return clip_rgba(vec4<f32>(max(d.rgb - s.rgb, vec3<f32>(0.0)), union_alpha(s, d)));
        }
        // Multiply
        case 14u: {
            if (s.a <= 0.0) {
                return d;
            }
            return clip_rgba(vec4<f32>(s.rgb * d.rgb + s.rgb * d1a + d.rgb * s1a, union_alpha(s, d)));
        }
        // Screen
        case 15u: {
            if (s.a <= 0.0) {
                return d;
            }
            return clip_rgba(vec4<f32>(d.rgb + s.rgb - s.rgb * d.rgb, union_alpha(s, d)));
        }
        // Overlay
        case 16u: {
            if (s.a <= 0.0) {
                return d;
            }
            return clip_rgba(vec4<f32>(
                overlay_calc(d.r, s.r, d.a, s.a, sada, d1a, s1a),
                overlay_calc(d.g, s.g, d.a, s.a, sada, d1a, s1a),
                overlay_calc(d.b, s.b, d.a, s.a, sada, d1a, s1a),
                union_alpha(s, d)));
        }
        // Darken
        case 17u: {
            if (s.a <= 0.0) {
                return d;
            }
            let rgb = min(s.rgb * d.a, d.rgb * s.a) + s.rgb * d1a + d.rgb * s1a;
            return clip_rgba(vec4<f32>(rgb, union_alpha(s, d)));
        }
        // Lighten
        case 18u: {
            if (s.a <= 0.0) {
                return d;
            }
            let rgb = max(s.rgb * d.a, d.rgb * s.a) + s.rgb * d1a + d.rgb * s1a;
            return clip_rgba(vec4<f32>(rgb, union_alpha(s, d)));
        }
        // ColorDodge
        case 19u: {
            if (s.a <= 0.0) {
                return d;
            }
            if (d.a <= 0.0) {
                return s;
            }
            return clip_rgba(vec4<f32>(
                color_dodge_calc(d.r, s.r, d.a, s.a, sada, d1a, s1a),
                color_dodge_calc(d.g, s.g, d.a, s.a, sada, d1a, s1a),
                color_dodge_calc(d.b, s.b, d.a, s.a, sada, d1a, s1a),
                d.a + s.a - sada));
        }
        // ColorBurn
        case 20u: {
            if (s.a <= 0.0) {
                return d;
            }
            if (d.a <= 0.0) {
                return s;
            }
            return clip_rgba(vec4<f32>(
                color_burn_calc(d.r, s.r, d.a, s.a, sada, d1a, s1a),
                color_burn_calc(d.g, s.g, d.a, s.a, sada, d1a, s1a),
                color_burn_calc(d.b, s.b, d.a, s.a, sada, d1a, s1a),
                d.a + s.a - sada));
        }
        // HardLight
        case 21u: {
            if (s.a <= 0.0) {
                return d;
            }
            return clip_rgba(vec4<f32>(
                hard_light_calc(d.r, s.r, d.a, s.a, sada, d1a, s1a),
                hard_light_calc(d.g, s.g, d.a, s.a, sada, d1a, s1a),
                hard_light_calc(d.b, s.b, d.a, s.a, sada, d1a, s1a),
                d.a + s.a - sada));
        }
        // SoftLight
        case 22u: {
            if (s.a <= 0.0) {
                return d;
            }
            if (d.a <= 0.0) {
                return s;
            }
            return clip_rgba(vec4<f32>(
                soft_light_calc(d.r, s.r, d.a, s.a, sada, d1a, s1a),
                soft_light_calc(d.g, s.g, d.a, s.a, sada, d1a, s1a),
                soft_light_calc(d.b, s.b, d.a, s.a, sada, d1a, s1a),
                d.a + s.a - sada));
        }
        // Difference
        case 23u: {
            if (s.a <= 0.0) {
                return d;
            }
            let rgb = d.rgb + s.rgb - 2.0 * min(s.rgb * d.a, d.rgb * s.a);
            return clip_rgba(vec4<f32>(rgb, union_alpha(s, d)));
        }
        // Exclusion
        case 24u: {
            if (s.a <= 0.0) {
                return d;
            }
            let rgb = (s.rgb * d.a + d.rgb * s.a - 2.0 * s.rgb * d.rgb) + s.rgb * d1a + d.rgb * s1a;
            return clip_rgba(vec4<f32>(rgb, union_alpha(s, d)));
        }
        default: {
            return d;
        }
    }
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

@fragment fn fs_main(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let ip = vec2<i32>(pos.xy);
    let dest = textureLoad(dest_tex, ip, 0);
    if (u.mode == 1u) {
        return dest;
    }
    let db = floor(dest * 255.0 + 0.5);
    let cover = floor(textureLoad(coverage_tex, ip, 0).a * 255.0 + 0.5);
    var out = db / 255.0;
    // Software never visits a pixel the shape does not cover.
    if (cover > 0.0) {
        // Rgba8::from_double — round half up to a byte.
        let v = clamp(blend(u.op, u.source, db, cover), vec4<f32>(0.0), vec4<f32>(1.0));
        out = floor(v * 255.0 + 0.5) / 255.0;
    }
    if (u.srgb == 1u) {
        out = vec4<f32>(srgb_to_linear(out.rgb), out.a);
    }
    return out;
}
