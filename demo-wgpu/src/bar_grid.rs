//! `BarGridWgpuRenderer` and `WgpuCubeWidget` — wgpu port of the 3-D bar-grid
//! animation widget.
//!
//! Mirrors the role of `bar_grid.rs` in `demo-gl`: both the renderer and the
//! widget live in this shared crate so that `demo-native` and `demo-wasm` use
//! exactly the same compiled bytes.
//!
//! # Pipeline
//!
//! - Single instanced draw: 36 indices × 128 instances (16 cols × 8 rows) per
//!   frame, in one render pass with depth testing.
//! - Per-vertex: position (`vec3`) + normal (`vec3`).
//! - Per-instance: grid coordinate `(col, row)` — drives the sine-field height
//!   in the vertex shader.
//! - Uniforms: `mat4 view_proj`, `f32 phase`, `vec2 grid_size`, light vector,
//!   four palette colours.  Packed into a single 16-byte-aligned struct to
//!   avoid multiple uniform buffers per frame.
//! - Depth attachment: `Depth32Float` texture matched to the bar-grid's
//!   screen rect; cleared each frame so AGG content beneath the widget is
//!   preserved.
//!
//! Animation phase comes from `web_time::Instant` so the renderer compiles +
//! runs identically on native and `wasm32-unknown-unknown`.
//!
//! # Theme integration
//!
//! `bar_palette_for_theme()` reads `agg_gui::current_visuals()` each frame so
//! a light/dark toggle recolours the bars on the next paint without rebuilding
//! the pipeline.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use agg_gui::draw_ctx::DrawCtx;
use agg_gui::event::{Event, EventResult};
use agg_gui::geometry::Rect;
use agg_gui::widget::Widget;
use agg_gui::{Size, TransAffine};
use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use crate::bar_grid_math::{look_at, mat4_mul, normalize3, perspective};
use crate::msaa::MsaaFramebuffer;
use crate::{DrawCommand, WgpuGfxCtx};

thread_local! {
    /// Set each frame by [`WgpuCubeWidget::paint`].  Mirrors the GL backend
    /// constant of the same name so platform shells with debug-overlay code
    /// compiled against either backend keep working.
    pub static CUBE_SCREEN_RECT: Cell<Rect> = Cell::new(Rect::default());
}

// ---------------------------------------------------------------------------
// Grid configuration + animation constants
// ---------------------------------------------------------------------------

const GRID_COLS: u32 = 16;
const GRID_ROWS: u32 = 8;
const BAR_HALF: f32 = 0.45;
const BAR_WAVE_SPEED: f64 = 1.4;

// ---------------------------------------------------------------------------
// WGSL shader source — translated from `demo-gl/src/bar_grid.rs`
// ---------------------------------------------------------------------------

const BAR_WGSL: &str = "
struct Uniforms {
    view_proj: mat4x4<f32>,
    light_dir: vec3<f32>,
    phase: f32,
    grid_size: vec2<f32>,
    _pad0: vec2<f32>,
    col_left: vec3<f32>,
    _pad1: f32,
    col_right: vec3<f32>,
    _pad2: f32,
    col_accent: vec3<f32>,
    _pad3: f32,
    peak_color: vec3<f32>,
    _pad4: f32,
}
@group(0) @binding(0) var<uniform> u: Uniforms;

struct VIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) grid: vec2<f32>,
}
struct VOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) v_world_pos: vec3<f32>,
    @location(1) v_normal: vec3<f32>,
    @location(2) v_uv: vec2<f32>,
    @location(3) v_height: f32,
}

@vertex fn vs_main(in: VIn) -> VOut {
    let freq: f32 = 0.55;
    let MAX_H: f32 = 2.10;
    let MIN_H: f32 = MAX_H * 0.4;

    let wave_unit = sin(in.grid.x * freq + in.grid.y * freq + u.phase) * 0.5 + 0.5;
    let height = mix(MIN_H, MAX_H, wave_unit);

    let local = vec3<f32>(in.pos.x, in.pos.y * height, in.pos.z);
    let world = local + vec3<f32>(
        in.grid.x - (u.grid_size.x - 1.0) * 0.5,
        0.0,
        in.grid.y - (u.grid_size.y - 1.0) * 0.5
    );

    var out: VOut;
    out.clip_pos = u.view_proj * vec4<f32>(world, 1.0);
    out.v_world_pos = world;
    out.v_normal = in.normal;
    out.v_uv = in.grid / max(u.grid_size - vec2<f32>(1.0), vec2<f32>(1.0));
    out.v_height = wave_unit;
    return out;
}

@fragment fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    var base = mix(u.col_left, u.col_right, in.v_uv.x);
    base = mix(base, u.col_accent, in.v_uv.y * 0.35);
    base = mix(base, u.peak_color, pow(in.v_height, 2.0) * 0.25);

    let n_dot_l = max(dot(normalize(in.v_normal), u.light_dir), 0.0);
    let lit = 0.45 + 0.55 * n_dot_l;

    return vec4<f32>(base * lit, 1.0);
}
";

// ---------------------------------------------------------------------------
// Uniform layout — 192 bytes total, all members 16-byte aligned
// ---------------------------------------------------------------------------

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct BarUniforms {
    view_proj: [f32; 16],
    light_dir: [f32; 3],
    phase: f32,
    grid_size: [f32; 2],
    _pad0: [f32; 2],
    col_left: [f32; 3],
    _pad1: f32,
    col_right: [f32; 3],
    _pad2: f32,
    col_accent: [f32; 3],
    _pad3: f32,
    peak_color: [f32; 3],
    _pad4: f32,
}

const _: () = assert!(std::mem::size_of::<BarUniforms>() == 160);

// ---------------------------------------------------------------------------
// Geometry + instance data
// ---------------------------------------------------------------------------

/// 24 vertices (4 per face) so each face carries its own flat normal — gives
/// clean shaded edges without smoothing artifacts a shared-vertex box would
/// produce under per-vertex lighting.
fn bar_box_verts() -> Vec<f32> {
    let h = BAR_HALF;
    let face = |verts: [[f32; 3]; 4], n: [f32; 3]| -> Vec<f32> {
        let mut out = Vec::with_capacity(24);
        for v in verts {
            out.extend_from_slice(&[v[0], v[1], v[2], n[0], n[1], n[2]]);
        }
        out
    };
    let mut v = Vec::with_capacity(24 * 6);
    v.extend(face(
        [[-h, 1.0, -h], [h, 1.0, -h], [h, 1.0, h], [-h, 1.0, h]],
        [0.0, 1.0, 0.0],
    ));
    v.extend(face(
        [[-h, 0.0, h], [h, 0.0, h], [h, 0.0, -h], [-h, 0.0, -h]],
        [0.0, -1.0, 0.0],
    ));
    v.extend(face(
        [[-h, 0.0, h], [h, 0.0, h], [h, 1.0, h], [-h, 1.0, h]],
        [0.0, 0.0, 1.0],
    ));
    v.extend(face(
        [[h, 0.0, -h], [-h, 0.0, -h], [-h, 1.0, -h], [h, 1.0, -h]],
        [0.0, 0.0, -1.0],
    ));
    v.extend(face(
        [[h, 0.0, h], [h, 0.0, -h], [h, 1.0, -h], [h, 1.0, h]],
        [1.0, 0.0, 0.0],
    ));
    v.extend(face(
        [[-h, 0.0, -h], [-h, 0.0, h], [-h, 1.0, h], [-h, 1.0, -h]],
        [-1.0, 0.0, 0.0],
    ));
    v
}

/// 6 faces × 2 triangles × 3 indices = 36, all under u16 max.
fn bar_box_indices() -> Vec<u16> {
    let mut idx = Vec::with_capacity(36);
    for face in 0..6u16 {
        let b = face * 4;
        idx.extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
    }
    idx
}

/// One vec2 per instance — `(col, row)`.  16 × 8 = 128 entries.
fn bar_instance_data() -> Vec<f32> {
    let mut out = Vec::with_capacity((GRID_COLS * GRID_ROWS) as usize * 2);
    for row in 0..GRID_ROWS {
        for col in 0..GRID_COLS {
            out.push(col as f32);
            out.push(row as f32);
        }
    }
    out
}

fn bar_wave_phase(elapsed_secs: f64) -> f32 {
    (elapsed_secs * BAR_WAVE_SPEED).rem_euclid(std::f64::consts::TAU) as f32
}

// ---------------------------------------------------------------------------
// BarGridWgpuRenderer
// ---------------------------------------------------------------------------

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

pub struct BarGridWgpuRenderer {
    /// 3-D bar pipeline — always `sample_count = 1`, since AA comes from
    /// SSAA (rendering into an oversized off-screen framebuffer and
    /// downsampling) instead of hardware MSAA.
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    vbo: wgpu::Buffer,
    ibo: wgpu::Buffer,
    instance_vbo: wgpu::Buffer,
    /// Lazy-allocated off-screen framebuffer (with depth) sized to
    /// `ssaa_scale × {widget_w, widget_h}`.  At `ssaa_scale = 1` it's the
    /// widget's own pixel rect (no AA); at `2` it's 4× the pixel count
    /// (label "4×"); at `4` it's 16× (label "16×").  The shared `tex_pipeline`
    /// blits this onto the surface with a linear sampler, which performs the
    /// downsample for free at 2× minification (a single bilinear tap is the
    /// 2×2 box) and an approximation at 4× minification.
    framebuffer: Option<MsaaFramebuffer>,
    surface_format: wgpu::TextureFormat,
    /// Linear scale of the off-screen framebuffer (1 = no AA, 2 = 4× SSAA,
    /// 4 = 16× SSAA).  Driven by [`crate::msaa::ssaa_linear_scale`] from the
    /// widget's UI-facing samples cell.
    ssaa_scale: u32,
    /// Animation start time — passed in by the widget so renderer
    /// rebuilds (e.g. an SSAA scale toggle) keep the bar wave phase continuous.
    start: web_time::Instant,
}

impl BarGridWgpuRenderer {
    pub fn new(
        device: &wgpu::Device,
        surface_format: wgpu::TextureFormat,
        ssaa_samples: u32,
        start: web_time::Instant,
    ) -> Self {
        let ssaa_scale = crate::msaa::ssaa_linear_scale(ssaa_samples);
        // Pipeline is single-sample regardless of SSAA factor.  AA happens
        // by the framebuffer being bigger than the screen rect, not by
        // hardware multisampling.
        let sample_count: u32 = 1;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("bar_grid"),
            source: wgpu::ShaderSource::Wgsl(BAR_WGSL.into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bar_grid_bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let opt_layouts: Vec<Option<&wgpu::BindGroupLayout>> = vec![Some(&bind_group_layout)];
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("bar_grid_layout"),
            bind_group_layouts: &opt_layouts,
            immediate_size: 0,
        });

        let vert_attrs = [
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x3,
                offset: 0,
                shader_location: 0,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x3,
                offset: 12,
                shader_location: 1,
            },
        ];
        let inst_attrs = [wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x2,
            offset: 0,
            shader_location: 2,
        }];

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("bar_grid_pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[
                    wgpu::VertexBufferLayout {
                        array_stride: 24,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &vert_attrs,
                    },
                    wgpu::VertexBufferLayout {
                        array_stride: 8,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &inst_attrs,
                    },
                ],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                // GL backend leaves face culling disabled (default in GL),
                // so the bar-box vertex winding produced for both Y-up faces
                // is rendered regardless of orientation.  Match that here —
                // turning back-face culling on under wgpu's CCW front-face
                // default drops every face that happens to be wound the
                // wrong way and produces a skeletal-looking grid.
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: sample_count.max(1),
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview_mask: None,
            cache: None,
        });

        let verts = bar_box_verts();
        let vbo = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("bar_grid_vbo"),
            contents: bytemuck::cast_slice(&verts),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let indices = bar_box_indices();
        let ibo = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("bar_grid_ibo"),
            contents: bytemuck::cast_slice(&indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        let instances = bar_instance_data();
        let instance_vbo = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("bar_grid_instance_vbo"),
            contents: bytemuck::cast_slice(&instances),
            usage: wgpu::BufferUsages::VERTEX,
        });

        Self {
            pipeline,
            bind_group_layout,
            vbo,
            ibo,
            instance_vbo,
            framebuffer: None,
            surface_format,
            ssaa_scale,
            start,
        }
    }

    /// Linear SSAA scale this renderer was built for (1 / 2 / 4).  Used by
    /// the cube widget to detect when a new toolbar setting needs a fresh
    /// renderer.
    pub fn ssaa_scale(&self) -> u32 {
        self.ssaa_scale
    }

    /// Lazy-allocate the off-screen framebuffer at `ssaa_scale × widget`
    /// pixels and resize it on widget changes.  Single-sample throughout —
    /// AA comes from the size, not the sample count.
    fn ensure_framebuffer(
        &mut self,
        device: &wgpu::Device,
        widget_w: u32,
        widget_h: u32,
    ) -> &mut MsaaFramebuffer {
        let scale = self.ssaa_scale.max(1);
        let w = widget_w.saturating_mul(scale).max(1);
        let h = widget_h.saturating_mul(scale).max(1);
        let needs_new = self.framebuffer.is_none();
        if needs_new {
            self.framebuffer = Some(MsaaFramebuffer::new(
                device,
                w,
                h,
                /* sample_count = */ 1,
                self.surface_format,
                /* with_depth = */ true,
            ));
        }
        let fb = self.framebuffer.as_mut().unwrap();
        fb.ensure_size(device, w, h);
        fb
    }

    /// Drive the bar-grid scene onto `target_view` using the caller's
    /// `encoder`.  No submission happens here — the deferred-flush owner
    /// finishes/submits the encoder once it has accumulated all the frame's
    /// passes.
    ///
    /// `target_size` is the active render target's full dimensions (surface
    /// or layer).  `screen_rect` is the bar grid's logical rect in Y-up
    /// pixels of that target; `parent_clip` is the framework scissor in the
    /// same coordinate space.
    ///
    /// `pipelines` is the shared 2-D pipeline collection — used for the
    /// blit pass that copies the (resolved) bar-grid output into
    /// `target_view`.  We can't auto-resolve MSAA into `target_view`
    /// directly because wgpu's resolve covers the full attachment area,
    /// which would clobber the 2-D content outside the bar-grid rect.
    pub(crate) fn draw(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        target_view: &wgpu::TextureView,
        target_size: (u32, u32),
        pipelines: &crate::pipelines::WgpuPipelines,
        screen_rect: Rect,
        parent_clip: Option<[i32; 4]>,
    ) {
        if screen_rect.width < 1.0 || screen_rect.height < 1.0 {
            return;
        }
        let [_, _, gl_w, gl_h] = pixel_rect(screen_rect);
        if gl_w <= 0 || gl_h <= 0 {
            return;
        }
        let widget_w = gl_w as u32;
        let widget_h = gl_h as u32;

        // Lazy-init / resize the off-screen framebuffer at `ssaa_scale ×
        // widget` pixels.  Memory scales with the visible 3-D area × the SSAA
        // factor (4× for "4×", 16× for "16×").
        let scale = self.ssaa_scale.max(1);
        let _ = self.ensure_framebuffer(device, widget_w, widget_h);
        let fb_w = widget_w.saturating_mul(scale).max(1);
        let fb_h = widget_h.saturating_mul(scale).max(1);

        // Aspect comes from the widget rect (not the SSAA-scaled framebuffer)
        // — the camera frames the same scene, we just sample it at higher
        // density before downsampling.
        let aspect = gl_w as f32 / gl_h.max(1) as f32;
        let proj = perspective(35_f32.to_radians(), aspect, 0.5, 100.0);
        let view = look_at([-7.0, 8.5, 11.0], [0.0, 0.5, 0.0], [0.0, 1.0, 0.0]);
        let view_proj = mat4_mul(proj, view);

        let palette = bar_palette_for_theme();
        let phase = bar_wave_phase(self.start.elapsed().as_secs_f64());
        let bar_uniforms = BarUniforms {
            view_proj,
            light_dir: normalize3([0.55, 0.85, 0.45]),
            phase,
            grid_size: [GRID_COLS as f32, GRID_ROWS as f32],
            _pad0: [0.0; 2],
            col_left: palette.left,
            _pad1: 0.0,
            col_right: palette.right,
            _pad2: 0.0,
            col_accent: palette.accent,
            _pad3: 0.0,
            peak_color: palette.peak,
            _pad4: 0.0,
        };
        let bar_ub = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("bar_grid_uniforms"),
            contents: bytemuck::bytes_of(&bar_uniforms),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let bar_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bar_grid_bg"),
            layout: &self.bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: bar_ub.as_entire_binding(),
            }],
        });

        // ── Pass 1: render bars into off-screen framebuffer ────────────────
        let fb = self.framebuffer.as_ref().unwrap();
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("bar_grid_render_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: fb.render_view(),
                    resolve_target: fb.resolve_target(),
                    depth_slice: None,
                    ops: wgpu::Operations {
                        // Clear to transparent — pixels the bars don't cover
                        // alpha-blend through to the 2-D backdrop in pass 2.
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: fb.depth_view().map(|dv| {
                    wgpu::RenderPassDepthStencilAttachment {
                        view: dv,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(1.0),
                            store: wgpu::StoreOp::Discard,
                        }),
                        stencil_ops: None,
                    }
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            // Render at the supersampled framebuffer size so geometry edges
            // are sampled at SSAA resolution; the blit pass below downsamples.
            pass.set_viewport(0.0, 0.0, fb_w as f32, fb_h as f32, 0.0, 1.0);
            pass.set_scissor_rect(0, 0, fb_w, fb_h);
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bar_bind_group, &[]);
            pass.set_vertex_buffer(0, self.vbo.slice(..));
            pass.set_vertex_buffer(1, self.instance_vbo.slice(..));
            pass.set_index_buffer(self.ibo.slice(..), wgpu::IndexFormat::Uint16);
            let instances = (GRID_COLS * GRID_ROWS) as u32;
            pass.draw_indexed(0..36, 0, 0..instances);
        }

        // ── Pass 2: composite onto the active 2-D target ───────────────────
        // Pick the right downsample at the SSAA scale:
        //   • scale 1 (Off):  no minification, bilinear is identity-equivalent
        //   • scale 2 (4×):   2× minification — single bilinear tap is the
        //                     exact 2×2 box average, no shader work needed
        //   • scale 4 (16×):  4× minification — single bilinear would only
        //                     average 4 of 16 source texels.  Switch to
        //                     `blit_downsample_4x_to` (4 bilinear taps in a
        //                     2×2 quadrant grid → exact 4×4 box).
        // Both methods alpha-blend through `BLEND_STANDARD` so transparent
        // pixels (where bars aren't covered) preserve the 2-D content
        // underneath.
        if self.ssaa_scale >= 4 {
            fb.blit_downsample_4x_to(
                device,
                encoder,
                target_view,
                target_size,
                screen_rect,
                parent_clip,
                pipelines,
            );
        } else {
            fb.blit_to(
                device,
                encoder,
                target_view,
                target_size,
                screen_rect,
                parent_clip,
                pipelines,
            );
        }

        let _ = (bar_ub, bar_bind_group);
    }
}

// ---------------------------------------------------------------------------
// WgpuCubeWidget
// ---------------------------------------------------------------------------

pub struct WgpuCubeWidget {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    /// Lazy-init renderer shared with the deferred draw command.  Wrapped in
    /// `Rc<RefCell<Option<>>>` so the widget can keep ownership while the
    /// `DrawCommand::DrawBarGrid` queued for this frame holds a clone of the
    /// `Rc` and reads the renderer back at execute time.
    renderer: Rc<RefCell<Option<BarGridWgpuRenderer>>>,
    /// Shared SSAA samples cell.  Values map to a linear framebuffer scale
    /// via [`crate::msaa::ssaa_linear_scale`]: `1`/`0` → 1× (Off), `4` → 2×
    /// (4× SSAA), `16` → 4× (16× SSAA).  The widget rebuilds the renderer
    /// when the resulting linear scale changes.  UI controls (Off / 4× /
    /// 16× toolbar at the top of the 3-D Animation window) write to the
    /// same cell — same `Rc<Cell<u8>>` the demo-ui state layer persists,
    /// so a tweak round-trips to disk for free.
    sample_count: Rc<Cell<u8>>,
    /// Animation start time — owned by the widget so it survives renderer
    /// rebuilds (the MSAA toggle drops + recreates the renderer to apply
    /// the new sample count).  Passing the same `start` to each new
    /// `BarGridWgpuRenderer` keeps the bar wave phase continuous, so the
    /// only visible change at a toggle is the AA itself.
    start: web_time::Instant,
}

impl Default for WgpuCubeWidget {
    fn default() -> Self {
        Self::new(Rc::new(Cell::new(0)))
    }
}

impl WgpuCubeWidget {
    /// Build a new cube widget bound to a shared SSAA samples `Rc<Cell<u8>>`.
    /// The cell stores the UI-facing pixel multiplier (`0`/`1` = Off,
    /// `4` = 4× SSAA, `16` = 16× SSAA).  Values get clamped on the read
    /// side by [`crate::msaa::ssaa_linear_scale`], so an old saved MSAA
    /// `8` (or any out-of-band value) maps to a sensible step instead of
    /// panicking.  The cell itself preserves the user's raw choice for
    /// state persistence.
    pub fn new(sample_count: Rc<Cell<u8>>) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            renderer: Rc::new(RefCell::new(None)),
            sample_count,
            start: web_time::Instant::now(),
        }
    }

    /// Borrow a clone of the shared sample-count cell.  UI controls that
    /// want to drive the MSAA setting (and have the persistence layer
    /// write through to disk) can grab a clone via this getter.
    pub fn sample_count_cell(&self) -> Rc<Cell<u8>> {
        Rc::clone(&self.sample_count)
    }
}

fn transformed_widget_rect(t: &TransAffine, width: f64, height: f64) -> Rect {
    let corners = [(0.0, 0.0), (width, 0.0), (width, height), (0.0, height)];
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for (mut x, mut y) in corners {
        t.transform(&mut x, &mut y);
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    }
    Rect::new(min_x, min_y, max_x - min_x, max_y - min_y)
}

fn pixel_rect(rect: Rect) -> [i32; 4] {
    let x0 = rect.x.floor() as i32;
    let y0 = rect.y.floor() as i32;
    let x1 = (rect.x + rect.width).ceil() as i32;
    let y1 = (rect.y + rect.height).ceil() as i32;
    [x0, y0, (x1 - x0).max(0), (y1 - y0).max(0)]
}

impl Widget for WgpuCubeWidget {
    fn type_name(&self) -> &'static str {
        "WgpuCubeWidget"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }

    fn layout(&mut self, available: Size) -> Size {
        available
    }

    fn needs_draw(&self) -> bool {
        true
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let t = ctx.transform();
        let screen_rect = transformed_widget_rect(&t, self.bounds.width, self.bounds.height);
        CUBE_SCREEN_RECT.with(|r| r.set(screen_rect));

        // Theme-aware backdrop — fills the gaps the bars don't cover.
        ctx.set_fill_color(ctx.visuals().window_fill);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, self.bounds.width, self.bounds.height);
        ctx.fill();

        // Backend-specific path: downcast to WgpuGfxCtx and queue a deferred
        // bar-grid draw.  On non-wgpu backends the downcast yields `None`
        // and the widget is just the placeholder fill above — the demo
        // still lays out and renders, the bars simply don't appear.
        if let Some(any) = ctx.as_any_mut() {
            if let Some(wgpu_ctx) = any.downcast_mut::<WgpuGfxCtx>() {
                // Read the shared SSAA-samples cell and convert to the
                // linear framebuffer scale.  Rebuild the renderer if the
                // resulting scale no longer matches the active one.  The UI
                // toolbar (Off / 4× / 16×) writes to this cell, so the
                // toggle takes effect on the next paint with no restart.
                let raw = self.sample_count.get() as u32;
                let desired_scale = crate::msaa::ssaa_linear_scale(raw);
                {
                    let mut slot = self.renderer.borrow_mut();
                    let needs_rebuild = match slot.as_ref() {
                        Some(r) => r.ssaa_scale() != desired_scale,
                        None => true,
                    };
                    if needs_rebuild {
                        *slot = Some(BarGridWgpuRenderer::new(
                            &wgpu_ctx.device,
                            wgpu_ctx.surface_format,
                            raw,
                            self.start,
                        ));
                    }
                }
                let parent_clip = wgpu_ctx.current_clip();
                wgpu_ctx.commands.push(DrawCommand::DrawBarGrid {
                    renderer: Rc::clone(&self.renderer),
                    screen_rect,
                    parent_clip,
                });
            }
        }
    }

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

// ---------------------------------------------------------------------------
// Theme-driven palette
// ---------------------------------------------------------------------------

struct BarPalette {
    left: [f32; 3],
    right: [f32; 3],
    accent: [f32; 3],
    peak: [f32; 3],
}

fn bar_palette_for_theme() -> BarPalette {
    let v = agg_gui::current_visuals();
    let bg = v.bg_color;
    let lum = 0.299 * bg.r + 0.587 * bg.g + 0.114 * bg.b;
    let dark = lum < 0.5;

    if dark {
        BarPalette {
            left: [0.18, 0.55, 0.95],
            right: [0.92, 0.32, 0.62],
            accent: [1.00, 0.78, 0.30],
            peak: [1.00, 1.00, 1.00],
        }
    } else {
        BarPalette {
            left: [0.10, 0.42, 0.85],
            right: [0.78, 0.18, 0.45],
            accent: [0.95, 0.55, 0.10],
            peak: [v.text_color.r, v.text_color.g, v.text_color.b],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cube_widget_rect_scales_logical_bounds_to_physical_pixels() {
        let transform = TransAffine::new_custom(2.0, 0.0, 0.0, 2.0, 40.0, 24.0);
        let rect = transformed_widget_rect(&transform, 100.0, 50.0);
        assert_eq!(rect.x, 40.0);
        assert_eq!(rect.y, 24.0);
        assert_eq!(rect.width, 200.0);
        assert_eq!(rect.height, 100.0);
    }

    #[test]
    fn pixel_rect_covers_fractional_physical_extent() {
        let rect = Rect::new(10.25, 20.5, 16.5, 10.25);
        assert_eq!(pixel_rect(rect), [10, 20, 17, 11]);
    }

    #[test]
    fn bar_wave_phase_stays_bounded_for_long_running_animation() {
        let short_elapsed = 12.345;
        let cycles = 100_000.0;
        let period = std::f64::consts::TAU / BAR_WAVE_SPEED;
        let long_elapsed = short_elapsed + period * cycles;

        let short_phase = bar_wave_phase(short_elapsed);
        let long_phase = bar_wave_phase(long_elapsed);

        assert!(long_phase >= 0.0);
        assert!(long_phase < std::f32::consts::TAU);
        assert!(
            (short_phase - long_phase).abs() < 0.0001,
            "phase drifted after many cycles: short={short_phase}, long={long_phase}"
        );
    }
}
