//! WGSL shader sources for all rendering pipelines.
//!
//! A single WGSL source compiles for both native wgpu (Vulkan/DX12/Metal) and
//! wgpu's WebGL2 backend — no `#[cfg(target_arch = "wasm32")]` splits required.
//!
//! All shaders share the same NDC vertex math as the GL backend:
//!   `ndc = (pos / resolution) * 2.0 - 1.0`
//!
//! Uniform struct byte layouts must exactly match the corresponding Rust
//! `#[repr(C)]` structs defined in `pipelines.rs`.

// ---------------------------------------------------------------------------
// Solid color pipeline (flat fill / stroke)
// ---------------------------------------------------------------------------
// group(0) binding(0): SolidUniforms { resolution, pad, color }

pub(crate) const SOLID_WGSL: &str = "
struct SolidUniforms {
    resolution: vec2<f32>,
    pad: vec2<f32>,
    color: vec4<f32>,
}
@group(0) @binding(0) var<uniform> u: SolidUniforms;

struct VIn { @location(0) pos: vec2<f32> }
struct VOut { @builtin(position) clip_pos: vec4<f32> }

@vertex fn vs_main(in: VIn) -> VOut {
    let ndc = (in.pos / u.resolution) * 2.0 - 1.0;
    return VOut(vec4<f32>(ndc, 0.0, 1.0));
}
@fragment fn fs_main() -> @location(0) vec4<f32> {
    return u.color;
}
";

// ---------------------------------------------------------------------------
// AA solid color pipeline (tess2 edge-flag halo strips)
// ---------------------------------------------------------------------------
// Same uniforms as solid; extra per-vertex alpha attribute for analytic AA.
// group(0) binding(0): SolidUniforms { resolution, pad, color }

pub(crate) const AA_SOLID_WGSL: &str = "
struct SolidUniforms {
    resolution: vec2<f32>,
    pad: vec2<f32>,
    color: vec4<f32>,
}
@group(0) @binding(0) var<uniform> u: SolidUniforms;

struct VIn {
    @location(0) pos: vec2<f32>,
    @location(1) alpha: f32,
}
struct VOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) v_alpha: f32,
}

@vertex fn vs_main(in: VIn) -> VOut {
    let ndc = (in.pos / u.resolution) * 2.0 - 1.0;
    return VOut(vec4<f32>(ndc, 0.0, 1.0), in.alpha);
}
@fragment fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    return vec4<f32>(u.color.rgb, u.color.a * in.v_alpha);
}
";

// ---------------------------------------------------------------------------
// Gradient pipeline (linear + radial, SVG spread modes)
// ---------------------------------------------------------------------------
// group(0) binding(0): GradientUniforms (see pipelines.rs)
// group(1) binding(0): ramp texture, binding(1): sampler

pub(crate) const GRADIENT_WGSL: &str = "
struct GradientUniforms {
    resolution: vec2<f32>,
    pad0: vec2<f32>,
    line: vec4<f32>,
    radial: vec4<f32>,
    focal: vec2<f32>,
    pad1: vec2<f32>,
    screen_inv_a: vec4<f32>,
    screen_inv_b: vec2<f32>,
    pad2: vec2<f32>,
    gradient_inv_a: vec4<f32>,
    gradient_inv_b: vec2<f32>,
    kind: u32,
    spread_mode: u32,
    global_alpha: f32,
    _pad3a: f32,
    _pad3b: f32,
    _pad3c: f32,
}
@group(0) @binding(0) var<uniform> u: GradientUniforms;
@group(1) @binding(0) var u_ramp: texture_2d<f32>;
@group(1) @binding(1) var u_sampler: sampler;

struct VIn {
    @location(0) pos: vec2<f32>,
    @location(1) alpha: f32,
}
struct VOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) v_pos: vec2<f32>,
    @location(1) v_alpha: f32,
}

@vertex fn vs_main(in: VIn) -> VOut {
    let ndc = (in.pos / u.resolution) * 2.0 - 1.0;
    return VOut(vec4<f32>(ndc, 0.0, 1.0), in.pos, in.alpha);
}

fn aff(a: vec4<f32>, b: vec2<f32>, p: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(p.x*a.x + p.y*a.z + b.x, p.x*a.y + p.y*a.w + b.y);
}

fn apply_spread(t: f32) -> f32 {
    if u.spread_mode == 1u {
        return 1.0 - abs((t - 2.0 * floor(t * 0.5)) - 1.0);
    } else if u.spread_mode == 2u {
        return t - floor(t);
    }
    return clamp(t, 0.0, 1.0);
}

fn linear_t(p: vec2<f32>) -> f32 {
    let a = u.line.xy;
    let b = u.line.zw;
    let d = b - a;
    let len2 = max(dot(d, d), 0.000001);
    return dot(p - a, d) / len2;
}

fn radial_t(p: vec2<f32>) -> f32 {
    let c = u.radial.xy;
    let r = max(u.radial.z, 0.000001);
    let f = u.focal;
    let dv = p - f;
    let fc = f - c;
    let A = dot(dv, dv);
    if A <= 0.000001 { return 0.0; }
    let B = 2.0 * dot(fc, dv);
    let C = dot(fc, fc) - r * r;
    let disc = max(B*B - 4.0*A*C, 0.0);
    let k = (-B + sqrt(disc)) / (2.0 * A);
    if k > 0.000001 { return 1.0 / k; }
    return 0.0;
}

@fragment fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    var p = aff(u.screen_inv_a, u.screen_inv_b, in.v_pos);
    p = aff(u.gradient_inv_a, u.gradient_inv_b, p);
    var t: f32;
    if u.kind == 1u { t = radial_t(p); } else { t = linear_t(p); }
    let tc = apply_spread(t);
    let c = textureSample(u_ramp, u_sampler, vec2<f32>(tc, 0.5));
    return vec4<f32>(c.rgb, c.a * in.v_alpha * u.global_alpha);
}
";

// ---------------------------------------------------------------------------
// Textured quad pipeline (image blits, Label backbuffers)
// ---------------------------------------------------------------------------
// group(0) binding(0): TexUniforms { resolution, pad }
// group(1) binding(0): texture, binding(1): sampler

pub(crate) const TEX_WGSL: &str = "
struct TexUniforms {
    resolution: vec2<f32>,
    pad: vec2<f32>,
}
@group(0) @binding(0) var<uniform> u: TexUniforms;
@group(1) @binding(0) var u_tex: texture_2d<f32>;
@group(1) @binding(1) var u_sampler: sampler;

struct VIn {
    @location(0) pos: vec2<f32>,
    @location(1) uv: vec2<f32>,
}
struct VOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) v_uv: vec2<f32>,
}

@vertex fn vs_main(in: VIn) -> VOut {
    let ndc = (in.pos / u.resolution) * 2.0 - 1.0;
    return VOut(vec4<f32>(ndc, 0.0, 1.0), in.uv);
}
@fragment fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    return textureSample(u_tex, u_sampler, in.v_uv);
}
";

// ---------------------------------------------------------------------------
// Layer composite pipeline
// ---------------------------------------------------------------------------
// Reuses the TEX_WGSL vertex shader.
// group(0) binding(0): LayerUniforms
// group(1) binding(0): layer texture, binding(1): sampler

pub(crate) const LAYER_WGSL: &str = "
struct LayerUniforms {
    resolution: vec2<f32>,
    alpha: f32,
    mask_enabled: u32,
    layer_size: vec2<f32>,
    mask_radius: f32,
    pad0: f32,
    mask_rect: vec4<f32>,
}
@group(0) @binding(0) var<uniform> u: LayerUniforms;
@group(1) @binding(0) var u_tex: texture_2d<f32>;
@group(1) @binding(1) var u_sampler: sampler;

struct VIn {
    @location(0) pos: vec2<f32>,
    @location(1) uv: vec2<f32>,
}
struct VOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) v_uv: vec2<f32>,
}

@vertex fn vs_main(in: VIn) -> VOut {
    let ndc = (in.pos / u.resolution) * 2.0 - 1.0;
    return VOut(vec4<f32>(ndc, 0.0, 1.0), in.uv);
}

fn rounded_mask(p: vec2<f32>) -> f32 {
    let half_size = u.mask_rect.zw * 0.5;
    let r = min(u.mask_radius, min(half_size.x, half_size.y));
    let center = u.mask_rect.xy + half_size;
    let q = abs(p - center) - max(half_size - vec2<f32>(r), vec2<f32>(0.0));
    let dist = length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
    return clamp(0.5 - dist, 0.0, 1.0);
}

@fragment fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let c = textureSample(u_tex, u_sampler, in.v_uv);
    var mask = 1.0;
    if u.mask_enabled != 0u {
        mask = rounded_mask(in.v_uv * u.layer_size);
    }
    let a = u.alpha * mask;
    return vec4<f32>(c.rgb * a, c.a * a);
}
";

// ---------------------------------------------------------------------------
// LCD subpixel text pipeline (3-pass write-mask fallback)
// ---------------------------------------------------------------------------
// Three render pipelines created from this shader, differing only in the
// ColorTargetState.write_mask (RED / GREEN / BLUE).  Each pass sets u.channel
// to select which subpixel channel's coverage drives the output alpha.
// group(0) binding(0): LcdUniforms { resolution, channel, pad, color }
// group(1) binding(0): mask texture (RGB8 packed as RGBA8), binding(1): sampler

pub(crate) const LCD_WGSL: &str = "
struct LcdUniforms {
    resolution: vec2<f32>,
    channel: u32,
    pad: u32,
    color: vec4<f32>,
}
@group(0) @binding(0) var<uniform> u: LcdUniforms;
@group(1) @binding(0) var u_mask: texture_2d<f32>;
@group(1) @binding(1) var u_sampler: sampler;

struct VIn {
    @location(0) pos: vec2<f32>,
    @location(1) uv: vec2<f32>,
}
struct VOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) v_uv: vec2<f32>,
}

@vertex fn vs_main(in: VIn) -> VOut {
    let ndc = (in.pos / u.resolution) * 2.0 - 1.0;
    return VOut(vec4<f32>(ndc, 0.0, 1.0), in.uv);
}
@fragment fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let c = textureSample(u_mask, u_sampler, in.v_uv).rgb;
    var ch: f32;
    if u.channel == 0u { ch = c.r; }
    else if u.channel == 1u { ch = c.g; }
    else { ch = c.b; }
    return vec4<f32>(u.color.rgb, ch * u.color.a);
}
";

// ---------------------------------------------------------------------------
// LCD backbuffer pipeline (3-pass write-mask, two-plane input)
// ---------------------------------------------------------------------------
// Composites an LcdCoverage-mode cached backbuffer (premultiplied colour plane
// + per-channel alpha plane) with premultiplied src-over blend.
// group(0) binding(0): LcbUniforms { resolution, channel, pad }
// group(1) binding(0): color plane, binding(1): alpha plane, binding(2): sampler

pub(crate) const LCB_WGSL: &str = "
struct LcbUniforms {
    resolution: vec2<f32>,
    channel: u32,
    pad: u32,
}
@group(0) @binding(0) var<uniform> u: LcbUniforms;
@group(1) @binding(0) var u_color: texture_2d<f32>;
@group(1) @binding(1) var u_alpha: texture_2d<f32>;
@group(1) @binding(2) var u_sampler: sampler;

struct VIn {
    @location(0) pos: vec2<f32>,
    @location(1) uv: vec2<f32>,
}
struct VOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) v_uv: vec2<f32>,
}

@vertex fn vs_main(in: VIn) -> VOut {
    let ndc = (in.pos / u.resolution) * 2.0 - 1.0;
    return VOut(vec4<f32>(ndc, 0.0, 1.0), in.uv);
}
@fragment fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let c = textureSample(u_color, u_sampler, in.v_uv).rgb;
    let a = textureSample(u_alpha, u_sampler, in.v_uv).rgb;
    var cc: f32;
    var aa: f32;
    var col: vec3<f32>;
    if u.channel == 0u {
        cc = c.r; aa = a.r; col = vec3<f32>(cc, 0.0, 0.0);
    } else if u.channel == 1u {
        cc = c.g; aa = a.g; col = vec3<f32>(0.0, cc, 0.0);
    } else {
        cc = c.b; aa = a.b; col = vec3<f32>(0.0, 0.0, cc);
    }
    return vec4<f32>(col, aa);
}
";
