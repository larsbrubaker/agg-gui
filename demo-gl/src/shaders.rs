#[cfg(target_arch = "wasm32")]
pub(crate) const SOLID_VERT: &str = "#version 300 es\nprecision mediump float;\nlayout(location=0)in vec2 a_pos;uniform vec2 u_resolution;void main(){vec2 ndc=(a_pos/u_resolution)*2.0-1.0;gl_Position=vec4(ndc,0.0,1.0);}";
#[cfg(not(target_arch = "wasm32"))]
pub(crate) const SOLID_VERT: &str = "#version 330 core\nlayout(location=0)in vec2 a_pos;uniform vec2 u_resolution;void main(){vec2 ndc=(a_pos/u_resolution)*2.0-1.0;gl_Position=vec4(ndc,0.0,1.0);}";

#[cfg(target_arch = "wasm32")]
pub(crate) const SOLID_FRAG: &str = "#version 300 es\nprecision mediump float;\nuniform vec4 u_color;out vec4 frag_color;void main(){frag_color=u_color;}";
#[cfg(not(target_arch = "wasm32"))]
pub(crate) const SOLID_FRAG: &str =
    "#version 330 core\nuniform vec4 u_color;out vec4 frag_color;void main(){frag_color=u_color;}";

// ── AA solid-colour pipeline (tess2 edge-flag halo strips) ──────────────────
//
// Same NDC math as `SOLID_*`, with an extra `a_alpha` attribute that gets
// interpolated across the halo quads so the fragment shader can multiply
// coverage into the source alpha.  Polygon interior vertices have alpha=1,
// halo "outer" vertices have alpha=0 — linear interpolation gives analytic
// edge coverage equivalent to 1-pixel MSAA, without requiring an MSAA
// framebuffer config.

#[cfg(target_arch = "wasm32")]
pub(crate) const AA_VERT: &str = "#version 300 es\nprecision mediump float;\nlayout(location=0)in vec2 a_pos;layout(location=1)in float a_alpha;uniform vec2 u_resolution;out float v_alpha;void main(){vec2 ndc=(a_pos/u_resolution)*2.0-1.0;gl_Position=vec4(ndc,0.0,1.0);v_alpha=a_alpha;}";
#[cfg(not(target_arch = "wasm32"))]
pub(crate) const AA_VERT: &str = "#version 330 core\nlayout(location=0)in vec2 a_pos;layout(location=1)in float a_alpha;uniform vec2 u_resolution;out float v_alpha;void main(){vec2 ndc=(a_pos/u_resolution)*2.0-1.0;gl_Position=vec4(ndc,0.0,1.0);v_alpha=a_alpha;}";

#[cfg(target_arch = "wasm32")]
pub(crate) const AA_FRAG: &str = "#version 300 es\nprecision mediump float;\nin float v_alpha;uniform vec4 u_color;out vec4 frag_color;void main(){frag_color=vec4(u_color.rgb,u_color.a*v_alpha);}";
#[cfg(not(target_arch = "wasm32"))]
pub(crate) const AA_FRAG: &str = "#version 330 core\nin float v_alpha;uniform vec4 u_color;out vec4 frag_color;void main(){frag_color=vec4(u_color.rgb,u_color.a*v_alpha);}";

// ── AA linear-gradient fill pipeline ────────────────────────────────────────
//
// Reuses the same `[x, y, alpha]` tessellation output as the AA solid path.
// The fragment shader maps screen-space pixels back through the current CTM
// and gradientTransform, computes SVG's linear-gradient parameter, applies the
// spread mode, then samples a 1-D ramp texture built from all gradient stops.

#[cfg(target_arch = "wasm32")]
pub(crate) const GRADIENT_VERT: &str = "#version 300 es\nprecision mediump float;\nlayout(location=0)in vec2 a_pos;layout(location=1)in float a_alpha;uniform vec2 u_resolution;out vec2 v_pos;out float v_alpha;void main(){vec2 ndc=(a_pos/u_resolution)*2.0-1.0;gl_Position=vec4(ndc,0.0,1.0);v_pos=a_pos;v_alpha=a_alpha;}";
#[cfg(not(target_arch = "wasm32"))]
pub(crate) const GRADIENT_VERT: &str = "#version 330 core\nlayout(location=0)in vec2 a_pos;layout(location=1)in float a_alpha;uniform vec2 u_resolution;out vec2 v_pos;out float v_alpha;void main(){vec2 ndc=(a_pos/u_resolution)*2.0-1.0;gl_Position=vec4(ndc,0.0,1.0);v_pos=a_pos;v_alpha=a_alpha;}";

#[cfg(target_arch = "wasm32")]
pub(crate) const GRADIENT_FRAG: &str = "#version 300 es\nprecision mediump float;\nin vec2 v_pos;in float v_alpha;uniform sampler2D u_ramp;uniform vec4 u_line;uniform vec4 u_screen_inv_a;uniform vec2 u_screen_inv_b;uniform vec4 u_gradient_inv_a;uniform vec2 u_gradient_inv_b;uniform int u_spread;uniform float u_global_alpha;out vec4 frag_color;vec2 aff(vec4 a,vec2 b,vec2 p){return vec2(p.x*a.x+p.y*a.z+b.x,p.x*a.y+p.y*a.w+b.y);}float spread(float t){if(u_spread==1){return 1.0-abs(mod(t,2.0)-1.0);}if(u_spread==2){return fract(t);}return clamp(t,0.0,1.0);}void main(){vec2 p=aff(u_screen_inv_a,u_screen_inv_b,v_pos);p=aff(u_gradient_inv_a,u_gradient_inv_b,p);vec2 a=u_line.xy;vec2 b=u_line.zw;vec2 d=b-a;float l2=max(dot(d,d),0.000001);float t=spread(dot(p-a,d)/l2);vec4 c=texture(u_ramp,vec2(t,0.5));frag_color=vec4(c.rgb,c.a*v_alpha*u_global_alpha);}";
#[cfg(not(target_arch = "wasm32"))]
pub(crate) const GRADIENT_FRAG: &str = "#version 330 core\nin vec2 v_pos;in float v_alpha;uniform sampler2D u_ramp;uniform vec4 u_line;uniform vec4 u_screen_inv_a;uniform vec2 u_screen_inv_b;uniform vec4 u_gradient_inv_a;uniform vec2 u_gradient_inv_b;uniform int u_spread;uniform float u_global_alpha;out vec4 frag_color;vec2 aff(vec4 a,vec2 b,vec2 p){return vec2(p.x*a.x+p.y*a.z+b.x,p.x*a.y+p.y*a.w+b.y);}float spread(float t){if(u_spread==1){return 1.0-abs(mod(t,2.0)-1.0);}if(u_spread==2){return fract(t);}return clamp(t,0.0,1.0);}void main(){vec2 p=aff(u_screen_inv_a,u_screen_inv_b,v_pos);p=aff(u_gradient_inv_a,u_gradient_inv_b,p);vec2 a=u_line.xy;vec2 b=u_line.zw;vec2 d=b-a;float l2=max(dot(d,d),0.000001);float t=spread(dot(p-a,d)/l2);vec4 c=texture(u_ramp,vec2(t,0.5));frag_color=vec4(c.rgb,c.a*v_alpha*u_global_alpha);}";

// ── Textured-quad pipeline (used by draw_image_rgba) ────────────────────────
//
// Same screen-space → NDC math as the solid pipeline, with an extra `a_uv`
// attribute and a single texture sampler binding.

#[cfg(target_arch = "wasm32")]
pub(crate) const TEX_VERT: &str = "#version 300 es\nprecision mediump float;\nlayout(location=0)in vec2 a_pos;layout(location=1)in vec2 a_uv;uniform vec2 u_resolution;out vec2 v_uv;void main(){vec2 ndc=(a_pos/u_resolution)*2.0-1.0;gl_Position=vec4(ndc,0.0,1.0);v_uv=a_uv;}";
#[cfg(not(target_arch = "wasm32"))]
pub(crate) const TEX_VERT: &str = "#version 330 core\nlayout(location=0)in vec2 a_pos;layout(location=1)in vec2 a_uv;uniform vec2 u_resolution;out vec2 v_uv;void main(){vec2 ndc=(a_pos/u_resolution)*2.0-1.0;gl_Position=vec4(ndc,0.0,1.0);v_uv=a_uv;}";

#[cfg(target_arch = "wasm32")]
pub(crate) const TEX_FRAG: &str = "#version 300 es\nprecision mediump float;\nin vec2 v_uv;uniform sampler2D u_tex;out vec4 frag_color;void main(){frag_color=texture(u_tex,v_uv);}";
#[cfg(not(target_arch = "wasm32"))]
pub(crate) const TEX_FRAG: &str = "#version 330 core\nin vec2 v_uv;uniform sampler2D u_tex;out vec4 frag_color;void main(){frag_color=texture(u_tex,v_uv);}";

#[cfg(target_arch = "wasm32")]
pub(crate) const LAYER_FRAG: &str = "#version 300 es\nprecision mediump float;\nin vec2 v_uv;uniform sampler2D u_tex;uniform float u_alpha;out vec4 frag_color;void main(){vec4 c=texture(u_tex,v_uv);frag_color=vec4(c.rgb*u_alpha,c.a*u_alpha);}";
#[cfg(not(target_arch = "wasm32"))]
pub(crate) const LAYER_FRAG: &str = "#version 330 core\nin vec2 v_uv;uniform sampler2D u_tex;uniform float u_alpha;out vec4 frag_color;void main(){vec4 c=texture(u_tex,v_uv);frag_color=vec4(c.rgb*u_alpha,c.a*u_alpha);}";

// ---------------------------------------------------------------------------
// LCD subpixel compositing pipeline
// ---------------------------------------------------------------------------
// Uploads a 3-channel `(cov_r, cov_g, cov_b)` coverage mask (from
// `agg_gui::text_lcd::rasterize_lcd_mask_*`) as an RGB(A) texture and
// composites per-channel onto the destination via **dual-source blending**:
//
//   dst.rgb = src.rgb * cov.rgb + dst.rgb * (1 - cov.rgb)
//
// The shader outputs the source colour on output 0 (index 0) and the
// coverage mask on output 0 (index 1).  The blend state uses
// `(GL_SRC1_COLOR, GL_ONE_MINUS_SRC1_COLOR)` so the hardware does the
// per-channel blend for us — giving correct LCD text against any
// background without the walk/sample-bg plumbing.
//
// Dual-source blend requires GL 3.3 core (which we already require) and
// is unavailable in base WebGL 2; the WASM build falls back to grayscale
// AA via a capability check.

#[cfg(target_arch = "wasm32")]
pub(crate) const LCD_VERT: &str = "#version 300 es\nprecision mediump float;\nlayout(location=0)in vec2 a_pos;layout(location=1)in vec2 a_uv;uniform vec2 u_resolution;out vec2 v_uv;void main(){vec2 ndc=(a_pos/u_resolution)*2.0-1.0;gl_Position=vec4(ndc,0.0,1.0);v_uv=a_uv;}";
#[cfg(not(target_arch = "wasm32"))]
pub(crate) const LCD_VERT: &str = "#version 330 core\nlayout(location=0)in vec2 a_pos;layout(location=1)in vec2 a_uv;uniform vec2 u_resolution;out vec2 v_uv;void main(){vec2 ndc=(a_pos/u_resolution)*2.0-1.0;gl_Position=vec4(ndc,0.0,1.0);v_uv=a_uv;}";

// Fragment: sample the mask, output src colour on index 0 and the
// coverage (scaled by src alpha so partial-alpha text fades correctly)
// on index 1.  Dual-source blending reads index 1 as SRC1, giving
// per-channel src-over: `dst = src * (cov * src.a) + dst * (1 - cov * src.a)`.
//
// Note on WebGL 2: dual-source isn't in the base spec, so we do a
// 3-pass color-masked fallback instead — same shader run three times
// with `u_channel` ∈ {0, 1, 2} selecting which subpixel alpha to use,
// and `glColorMask` restricting writes to one R/G/B channel per pass.
// Each pass uses standard `SRC_ALPHA, ONE_MINUS_SRC_ALPHA` blend; the
// net effect is per-channel src-over at the cost of 3× draw calls.
#[cfg(target_arch = "wasm32")]
pub(crate) const LCD_FRAG: &str = "#version 300 es\nprecision mediump float;\nin vec2 v_uv;uniform sampler2D u_mask;uniform vec4 u_color;uniform int u_channel;out vec4 frag_color;void main(){vec3 c=texture(u_mask,v_uv).rgb;float ch=(u_channel==0)?c.r:((u_channel==1)?c.g:c.b);frag_color=vec4(u_color.rgb,ch*u_color.a);}";
#[cfg(not(target_arch = "wasm32"))]
pub(crate) const LCD_FRAG: &str = "#version 330 core\nin vec2 v_uv;uniform sampler2D u_mask;uniform vec4 u_color;layout(location=0,index=0)out vec4 out_color;layout(location=0,index=1)out vec4 out_coverage;void main(){vec3 c=texture(u_mask,v_uv).rgb;out_color=vec4(u_color.rgb,1.0);out_coverage=vec4(c*u_color.a,1.0);}";

// LCD BACKBUFFER shader.  Composites an `LcdCoverage`-mode cached
// backbuffer (two RGB8 textures: premultiplied colour plane + per-channel
// alpha plane) onto the destination with per-channel src-over.
//
// Desktop GL 3.3 path: dual-source blend.  The shader emits two outputs,
// bound to blend indices 0 and 1:
//   - out_color    = (premult_color.rgb, max_alpha)     → index 0
//   - out_coverage = (per_channel_alpha.rgb, max_alpha) → index 1
// With `glBlendFuncSeparate(ONE, ONE_MINUS_SRC1_COLOR, ONE,
// ONE_MINUS_SRC1_ALPHA)` the hardware computes, per-channel:
//   dst.R_new = out_color.R + dst.R * (1 - out_coverage.R)
// which is premultiplied Porter-Duff src-over — and because each
// subpixel has its own `out_coverage` entry, LCD chroma survives the
// cache round-trip to the screen.
//
// WebGL 2 path: 3-pass color-masked fallback (see LCD_FRAG note).
// Per-pass output picks ONE channel's premultiplied colour + alpha
// and writes to ONE destination channel via `glColorMask`.  3× draw
// calls, but preserves full per-channel LCD chroma without needing
// `WEBGL_blend_func_extended`.
//
//   pass 0: u_channel=0 → out = (color.r, 0, 0, alpha.r); mask R
//   pass 1: u_channel=1 → out = (0, color.g, 0, alpha.g); mask G
//   pass 2: u_channel=2 → out = (0, 0, color.b, alpha.b); mask B
//
// Blend: `ONE, ONE_MINUS_SRC_ALPHA` — matches the premultiplied
// per-channel src-over the desktop dual-source path does.  Alpha-zero
// regions are written-but-no-op (out.ch = 0, blend factor = 1).
#[cfg(target_arch = "wasm32")]
pub(crate) const LCB_FRAG: &str = "#version 300 es\nprecision mediump float;\nin vec2 v_uv;uniform sampler2D u_color;uniform sampler2D u_alpha;uniform int u_channel;out vec4 frag_color;void main(){vec3 c=texture(u_color,v_uv).rgb;vec3 a=texture(u_alpha,v_uv).rgb;float cc=(u_channel==0)?c.r:((u_channel==1)?c.g:c.b);float aa=(u_channel==0)?a.r:((u_channel==1)?a.g:a.b);vec3 col=(u_channel==0)?vec3(cc,0.0,0.0):((u_channel==1)?vec3(0.0,cc,0.0):vec3(0.0,0.0,cc));frag_color=vec4(col,aa);}";
#[cfg(not(target_arch = "wasm32"))]
pub(crate) const LCB_FRAG: &str = "#version 330 core\nin vec2 v_uv;uniform sampler2D u_color;uniform sampler2D u_alpha;layout(location=0,index=0)out vec4 out_color;layout(location=0,index=1)out vec4 out_coverage;void main(){vec3 c=texture(u_color,v_uv).rgb;vec3 a=texture(u_alpha,v_uv).rgb;float ma=max(max(a.r,a.g),a.b);out_color=vec4(c,ma);out_coverage=vec4(a,ma);}";
