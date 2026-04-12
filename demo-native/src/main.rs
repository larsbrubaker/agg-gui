//! Native WGL demo for agg-gui — Phase 8 (GL cube).
//!
//! The entire demo UI (tab bar + content) is rendered by agg-gui via AGG →
//! Framebuffer → GL texture → full-screen quad. Winit events are forwarded
//! to a single [`App`] that owns a top-level TabView (Basics, Text, Layout, Tree).

mod cube_widget;
use cube_widget::{CubeGlRenderer, GlCubeWidget, CUBE_SCREEN_RECT};

use std::num::NonZeroU32;
use std::sync::Arc;

use std::cell::Cell;
use std::rc::Rc;

use agg_gui::{
    App, Button, Checkbox, Color, CompOp, Container, FlexColumn, FlexRow,
    Font, Framebuffer, GfxCtx, Key as AggKey, Label, Modifiers, MouseButton as AggMouseButton,
    NodeIcon, ProgressBar, RadioGroup, Rect, ScrollView, Separator, Size, SizedBox, Slider,
    Spacer, Splitter, Stack, TabView, TextField, TreeView, Widget, Window,
};
use agg_gui::event::{Event as AggEvent, EventResult as AggEventResult};

use glutin::config::ConfigTemplateBuilder;
use glutin::context::{ContextApi, ContextAttributesBuilder, Version};
use glutin::display::GetGlDisplay;
use glutin::prelude::*;
use glutin::surface::{GlSurface, SurfaceAttributesBuilder, WindowSurface};
use glutin_winit::DisplayBuilder;
use glow::HasContext;
use raw_window_handle::HasWindowHandle;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, Event, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::keyboard::{Key as WinitKey, NamedKey};
use winit::window::WindowAttributes;

const FONT_BYTES: &[u8] = include_bytes!("../../demo/assets/CascadiaCode.ttf");

// ---------------------------------------------------------------------------
// GL shaders — full-screen quad using gl_VertexID (no VBO required)
// ---------------------------------------------------------------------------

const VERT_SHADER: &str = r#"#version 330 core
out vec2 v_tex_coord;
void main() {
    float x = float((gl_VertexID & 1) * 2) - 1.0;
    float y = float((gl_VertexID >> 1) * 2) - 1.0;
    gl_Position = vec4(x, y, 0.0, 1.0);
    v_tex_coord = vec2((x + 1.0) * 0.5, (y + 1.0) * 0.5);
}
"#;

const FRAG_SHADER: &str = r#"#version 330 core
in vec2 v_tex_coord;
out vec4 frag_color;
uniform sampler2D u_texture;
void main() {
    frag_color = texture(u_texture, v_tex_coord);
}
"#;

// ---------------------------------------------------------------------------
// GlPresenter — uploads framebuffer → GL texture → full-screen quad
// ---------------------------------------------------------------------------

struct GlPresenter {
    gl: glow::Context,
    program: glow::Program,
    vao: glow::VertexArray,
    texture: glow::Texture,
    texture_width: u32,
    texture_height: u32,
}

impl GlPresenter {
    unsafe fn new(gl: glow::Context) -> Self {
        let program = gl.create_program().expect("create_program");
        let vert = gl.create_shader(glow::VERTEX_SHADER).unwrap();
        gl.shader_source(vert, VERT_SHADER);
        gl.compile_shader(vert);
        assert!(gl.get_shader_compile_status(vert), "vert: {}", gl.get_shader_info_log(vert));
        let frag = gl.create_shader(glow::FRAGMENT_SHADER).unwrap();
        gl.shader_source(frag, FRAG_SHADER);
        gl.compile_shader(frag);
        assert!(gl.get_shader_compile_status(frag), "frag: {}", gl.get_shader_info_log(frag));
        gl.attach_shader(program, vert);
        gl.attach_shader(program, frag);
        gl.link_program(program);
        assert!(gl.get_program_link_status(program), "link: {}", gl.get_program_info_log(program));
        gl.delete_shader(vert);
        gl.delete_shader(frag);

        let vao = gl.create_vertex_array().unwrap();

        let texture = gl.create_texture().unwrap();
        gl.bind_texture(glow::TEXTURE_2D, Some(texture));
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32);
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32);
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::LINEAR as i32);
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);

        Self { gl, program, vao, texture, texture_width: 0, texture_height: 0 }
    }

    unsafe fn update_texture(&mut self, fb: &Framebuffer) {
        let w = fb.width();
        let h = fb.height();
        self.gl.bind_texture(glow::TEXTURE_2D, Some(self.texture));
        if w != self.texture_width || h != self.texture_height {
            self.gl.tex_image_2d(
                glow::TEXTURE_2D, 0, glow::RGBA as i32, w as i32, h as i32,
                0, glow::RGBA, glow::UNSIGNED_BYTE, Some(fb.pixels()),
            );
            self.texture_width = w;
            self.texture_height = h;
        } else {
            self.gl.tex_sub_image_2d(
                glow::TEXTURE_2D, 0, 0, 0, w as i32, h as i32,
                glow::RGBA, glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(fb.pixels()),
            );
        }
    }

    unsafe fn present(&self) {
        self.gl.clear(glow::COLOR_BUFFER_BIT);
        self.gl.use_program(Some(self.program));
        self.gl.bind_vertex_array(Some(self.vao));
        self.gl.active_texture(glow::TEXTURE0);
        self.gl.bind_texture(glow::TEXTURE_2D, Some(self.texture));
        self.gl.draw_arrays(glow::TRIANGLE_STRIP, 0, 4);
    }
}

// ---------------------------------------------------------------------------
// TextDemoWidget — leaf widget that draws the Phase 3 text showcase
// ---------------------------------------------------------------------------

struct TextDemoWidget {
    bounds: Rect,
    font: Arc<Font>,
    children: Vec<Box<dyn Widget>>,
}

impl TextDemoWidget {
    fn new(font: Arc<Font>) -> Self {
        Self { bounds: Rect::default(), font, children: Vec::new() }
    }
}

impl Widget for TextDemoWidget {
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
        available
    }

    fn paint(&mut self, ctx: &mut GfxCtx) {
        let w = self.bounds.width;
        let h = self.bounds.height;
        let font = Arc::clone(&self.font);
        draw_text_tab(ctx, w, h, &font);
    }

    fn on_event(&mut self, _event: &AggEvent) -> AggEventResult { AggEventResult::Ignored }
}

// ---------------------------------------------------------------------------
// Widget tree — Phase 7: single TabView with all four tabs
// ---------------------------------------------------------------------------

fn build_demo_ui(font: Arc<Font>) -> App {
    let tab_view = TabView::new(Arc::clone(&font))
        .with_tab_bar_height(40.0)
        .with_font_size(13.0)
        .add_tab("Basics",  Box::new(build_basics_content(Arc::clone(&font))))
        .add_tab("Widgets", Box::new(build_widgets_content(Arc::clone(&font))))
        .add_tab("Text",    Box::new(TextDemoWidget::new(Arc::clone(&font))))
        .add_tab("Layout",  Box::new(build_layout_content(Arc::clone(&font))))
        .add_tab("Tree",    Box::new(build_tree_demo(Arc::clone(&font))));

    let window = build_cube_window(Arc::clone(&font));

    let root = Stack::new()
        .add(Box::new(tab_view))
        .add(Box::new(window));

    App::new(Box::new(root))
}

fn build_cube_window(font: Arc<Font>) -> Window {
    // The GlCubeWidget fills the window's content area. It draws a dark
    // placeholder in the AGG pass and records its screen rect; the GL cube
    // is drawn on top by CubeGlRenderer after the AGG texture is uploaded.
    let cube = GlCubeWidget::new();
    Window::new("3D Cube", font, Box::new(cube))
        .with_bounds(Rect::new(60.0, 120.0, 320.0, 280.0))
}

fn build_basics_content(font: Arc<Font>) -> Container {
    let font2 = Arc::clone(&font);
    let font3 = Arc::clone(&font);
    let font4 = Arc::clone(&font);
    let font5 = Arc::clone(&font);

    let mut root = Container::new()
        .with_background(Color::rgb(0.94, 0.94, 0.96))
        .with_padding(24.0);

    root.children_mut().push(Box::new(
        Button::new("Primary Action", Arc::clone(&font))
            .with_font_size(14.0)
            .on_click(|| println!("Primary")),
    ));
    root.children_mut().push(Box::new(
        Button::new("Secondary", Arc::clone(&font2))
            .with_font_size(14.0)
            .on_click(|| println!("Secondary")),
    ));
    root.children_mut().push(Box::new(
        Button::new("Destructive", Arc::clone(&font3))
            .with_font_size(14.0)
            .on_click(|| println!("Destructive")),
    ));
    root.children_mut().push(Box::new(
        TextField::new(Arc::clone(&font4))
            .with_font_size(14.0)
            .with_placeholder("Type something…"),
    ));
    root.children_mut().push(Box::new(
        TextField::new(Arc::clone(&font5))
            .with_font_size(14.0)
            .with_text("editable text")
            .with_placeholder("Another field"),
    ));
    root
}

fn build_widgets_content(font: Arc<Font>) -> ScrollView {
    let slider_val = Rc::new(Cell::new(0.42_f64));
    let cb1        = Rc::new(Cell::new(true));
    let cb2        = Rc::new(Cell::new(false));
    let cb3        = Rc::new(Cell::new(true));
    let radio_sel  = Rc::new(Cell::new(0_usize));

    let mut col = FlexColumn::new()
        .with_gap(20.0)
        .with_padding(24.0)
        .with_background(Color::rgb(0.94, 0.94, 0.96));

    // Buttons
    col.push(Box::new(Label::new("Buttons", Arc::clone(&font))
        .with_font_size(16.0)
        .with_color(Color::rgb(0.1, 0.1, 0.12))), 0.0);

    {
        let row = FlexRow::new()
            .with_gap(8.0)
            .add(Box::new(SizedBox::fixed(120.0, 34.0).with_child(Box::new(
                Button::new("Primary", Arc::clone(&font)).with_font_size(13.0).on_click(|| {})
            ))))
            .add(Box::new(SizedBox::fixed(120.0, 34.0).with_child(Box::new(
                Button::new("Secondary", Arc::clone(&font))
                    .with_font_size(13.0)
                    .with_theme(agg_gui::widgets::button::ButtonTheme {
                        background:         Color::rgba(0.22, 0.45, 0.88, 0.12),
                        background_hovered: Color::rgba(0.22, 0.45, 0.88, 0.22),
                        background_pressed: Color::rgba(0.22, 0.45, 0.88, 0.35),
                        label_color:        Color::rgb(0.22, 0.45, 0.88),
                        border_radius:      6.0,
                        focus_ring_color:   Color::rgba(0.22, 0.45, 0.88, 0.55),
                        focus_ring_width:   2.5,
                    })
                    .on_click(|| {})
            ))))
            .add(Box::new(SizedBox::fixed(120.0, 34.0).with_child(Box::new(
                Button::new("Danger", Arc::clone(&font))
                    .with_font_size(13.0)
                    .with_theme(agg_gui::widgets::button::ButtonTheme {
                        background:         Color::rgb(0.88, 0.25, 0.18),
                        background_hovered: Color::rgb(0.95, 0.32, 0.24),
                        background_pressed: Color::rgb(0.72, 0.18, 0.12),
                        label_color:        Color::white(),
                        border_radius:      6.0,
                        focus_ring_color:   Color::rgba(0.88, 0.25, 0.18, 0.55),
                        focus_ring_width:   2.5,
                    })
                    .on_click(|| {})
            ))));
        col.push(Box::new(row), 0.0);
    }

    col.push(Box::new(Separator::horizontal()), 0.0);

    // Checkboxes
    col.push(Box::new(Label::new("Checkboxes", Arc::clone(&font))
        .with_font_size(16.0)
        .with_color(Color::rgb(0.1, 0.1, 0.12))), 0.0);

    {
        let v1 = Rc::clone(&cb1);
        col.push(Box::new(Checkbox::new("Enable notifications", Arc::clone(&font), cb1.get())
            .on_change(move |v| { v1.set(v); })), 0.0);
        let v2 = Rc::clone(&cb2);
        col.push(Box::new(Checkbox::new("Dark mode", Arc::clone(&font), cb2.get())
            .on_change(move |v| { v2.set(v); })), 0.0);
        let v3 = Rc::clone(&cb3);
        col.push(Box::new(Checkbox::new("Send analytics", Arc::clone(&font), cb3.get())
            .on_change(move |v| { v3.set(v); })), 0.0);
    }

    col.push(Box::new(Separator::horizontal()), 0.0);

    // Slider
    col.push(Box::new(Label::new("Slider", Arc::clone(&font))
        .with_font_size(16.0)
        .with_color(Color::rgb(0.1, 0.1, 0.12))), 0.0);

    {
        let sv = Rc::clone(&slider_val);
        col.push(Box::new(Slider::new(slider_val.get(), 0.0, 1.0, Arc::clone(&font))
            .with_step(0.01)
            .on_change(move |v| { sv.set(v); })), 0.0);
    }

    col.push(Box::new(Separator::horizontal()), 0.0);

    // Radio
    col.push(Box::new(Label::new("Radio Group", Arc::clone(&font))
        .with_font_size(16.0)
        .with_color(Color::rgb(0.1, 0.1, 0.12))), 0.0);

    {
        let rs = Rc::clone(&radio_sel);
        col.push(Box::new(RadioGroup::new(
            vec!["Option A", "Option B", "Option C"],
            radio_sel.get(),
            Arc::clone(&font),
        ).on_change(move |i| { rs.set(i); })), 0.0);
    }

    col.push(Box::new(Separator::horizontal()), 0.0);

    // Progress Bar
    col.push(Box::new(Label::new("Progress Bar", Arc::clone(&font))
        .with_font_size(16.0)
        .with_color(Color::rgb(0.1, 0.1, 0.12))), 0.0);

    col.push(Box::new(ProgressBar::new(slider_val.get(), Arc::clone(&font))), 0.0);

    col.push(Box::new(Separator::horizontal()), 0.0);

    // Text Input
    col.push(Box::new(Label::new("Text Input", Arc::clone(&font))
        .with_font_size(16.0)
        .with_color(Color::rgb(0.1, 0.1, 0.12))), 0.0);

    col.push(Box::new(
        TextField::new(Arc::clone(&font))
            .with_font_size(14.0)
            .with_placeholder("Type something here…")
    ), 0.0);

    col.push(Box::new(SizedBox::fixed(0.0, 24.0)), 0.0);

    ScrollView::new(Box::new(col))
}

fn build_layout_content(font: Arc<Font>) -> TabView {
    TabView::new(Arc::clone(&font))
        .with_tab_bar_height(36.0)
        .with_font_size(13.0)
        .add_tab("Flex",   Box::new(build_flex_demo(Arc::clone(&font))))
        .add_tab("Scroll", Box::new(build_scroll_demo(Arc::clone(&font))))
        .add_tab("Split",  Box::new(build_split_demo(Arc::clone(&font))))
}

// ---------------------------------------------------------------------------
// Text tab draw helpers (Phase 3)
// ---------------------------------------------------------------------------

fn draw_text_tab(ctx: &mut GfxCtx, w: f64, h: f64, font: &Arc<Font>) {
    ctx.set_font(Arc::clone(font));
    // Fill only the widget's local area; ctx.clear() would erase the tab bar.
    ctx.set_fill_color(Color::rgb(0.94, 0.94, 0.96));
    ctx.begin_path();
    ctx.rect(0.0, 0.0, w, h);
    ctx.fill();

    let pad = (w.min(h) * 0.03).max(10.0);
    let gap = pad * 0.6;
    let col_w = (w - pad * 2.0 - gap) / 2.0;
    let row_h = (h - pad * 2.0 - gap) / 2.0;

    let panels = [
        (pad,               pad + row_h + gap, col_w, row_h),
        (pad + col_w + gap, pad + row_h + gap, col_w, row_h),
        (pad,               pad,               col_w, row_h),
        (pad + col_w + gap, pad,               col_w, row_h),
    ];
    for &(px, py, pw, ph) in &panels { draw_card(ctx, px, py, pw, ph); }
    { let (px, py, pw, ph) = panels[0]; draw_sizes_panel(ctx, px, py, pw, ph); }
    { let (px, py, pw, ph) = panels[1]; draw_measure_panel(ctx, px, py, pw, ph, font); }
    { let (px, py, pw, ph) = panels[2]; draw_multiline_panel(ctx, px, py, pw, ph, font); }
    { let (px, py, pw, ph) = panels[3]; draw_buttons_panel_text(ctx, px, py, pw, ph); }

    let lsize = (w * 0.012).clamp(9.0, 13.0);
    ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, 0.35));
    ctx.fill_text_gsv("agg-gui  Phase 3 — Text", pad, pad * 0.4, lsize);
}

fn draw_card(ctx: &mut GfxCtx, x: f64, y: f64, w: f64, h: f64) {
    ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, 0.08));
    ctx.set_blend_mode(CompOp::Multiply);
    ctx.begin_path(); ctx.rounded_rect(x + 2.0, y - 2.0, w, h, 10.0); ctx.fill();
    ctx.set_blend_mode(CompOp::SrcOver);
    ctx.set_fill_color(Color::rgb(1.0, 1.0, 1.0));
    ctx.begin_path(); ctx.rounded_rect(x, y, w, h, 10.0); ctx.fill();
}

fn panel_title_gsv(ctx: &mut GfxCtx, px: f64, py: f64, pw: f64, ph: f64, title: &str) {
    let size = (pw * 0.055).clamp(10.0, 16.0);
    ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, 0.55));
    ctx.fill_text_gsv(title, px + pw * 0.05, py + ph * 0.86, size);
}

fn draw_sizes_panel(ctx: &mut GfxCtx, px: f64, py: f64, pw: f64, ph: f64) {
    panel_title_gsv(ctx, px, py, pw, ph, "Font Sizes");
    let margin = pw * 0.06;
    let sizes: &[(f64, &str)] = &[
        (10.0, "Caption — 10px  The quick brown fox"),
        (13.0, "Body — 13px  The quick brown fox"),
        (18.0, "Subhead — 18px  The quick"),
        (24.0, "Heading — 24px  agg-gui"),
        (34.0, "Display — 34px  Aa"),
    ];
    let mut y = py + ph * 0.82;
    let adv = ph * 0.155;
    ctx.set_fill_color(Color::rgba(0.05, 0.05, 0.1, 0.85));
    for &(size, label) in sizes.iter() {
        ctx.set_font_size(size);
        ctx.fill_text(label, px + margin, y);
        y -= adv;
    }
}

fn draw_measure_panel(ctx: &mut GfxCtx, px: f64, py: f64, pw: f64, ph: f64, font: &Arc<Font>) {
    panel_title_gsv(ctx, px, py, pw, ph, "Measure Text");
    let margin = pw * 0.06;
    let font_size = (pw * 0.08).clamp(14.0, 26.0);
    ctx.set_font_size(font_size);
    let samples = ["Hello", "World!", "agg-gui", "Rust"];
    let col_w = (pw - margin * 2.0) / samples.len() as f64;
    let base_y = py + ph * 0.5;
    for (i, &word) in samples.iter().enumerate() {
        let x = px + margin + col_w * i as f64;
        let m = ctx.measure_text(word).unwrap_or_default();
        ctx.set_stroke_color(Color::rgba(0.6, 0.6, 0.65, 0.5)); ctx.set_line_width(1.0);
        ctx.begin_path(); ctx.move_to(x, base_y-2.0); ctx.line_to(x+m.width, base_y-2.0); ctx.stroke();
        ctx.set_stroke_color(Color::rgba(0.2, 0.5, 0.9, 0.35));
        ctx.begin_path(); ctx.move_to(x, base_y+m.ascent); ctx.line_to(x+m.width, base_y+m.ascent); ctx.stroke();
        ctx.set_stroke_color(Color::rgba(0.9, 0.3, 0.3, 0.35));
        ctx.begin_path(); ctx.move_to(x, base_y-m.descent); ctx.line_to(x+m.width, base_y-m.descent); ctx.stroke();
        ctx.set_fill_color(Color::rgba(0.2, 0.5, 0.9, 0.07));
        ctx.begin_path(); ctx.rect(x, base_y-m.descent, m.width, m.ascent+m.descent); ctx.fill();
        ctx.set_fill_color(Color::rgba(0.05, 0.05, 0.1, 0.88));
        ctx.fill_text(word, x, base_y);
    }
    let lsize = (pw * 0.032).clamp(7.0, 10.0);
    let ly = py + ph * 0.22;
    let lx = px + margin;
    ctx.set_font_size(lsize);
    ctx.set_fill_color(Color::rgba(0.2, 0.5, 0.9, 0.7));  ctx.fill_text("— ascent",   lx, ly);
    ctx.set_fill_color(Color::rgba(0.9, 0.3, 0.3, 0.7));  ctx.fill_text("— descent",  lx, ly - lsize * 1.5);
    ctx.set_fill_color(Color::rgba(0.5, 0.5, 0.55, 0.7)); ctx.fill_text("— baseline", lx, ly - lsize * 3.0);
    let _ = font;
}

fn draw_multiline_panel(ctx: &mut GfxCtx, px: f64, py: f64, pw: f64, ph: f64, font: &Arc<Font>) {
    panel_title_gsv(ctx, px, py, pw, ph, "Multi-line");
    let margin = pw * 0.06;
    let font_size = (pw * 0.055).clamp(11.0, 16.0);
    ctx.set_font_size(font_size);
    ctx.set_fill_color(Color::rgba(0.05, 0.05, 0.1, 0.85));
    let line_h = font.line_height_px(font_size) * 1.25;
    let x = px + margin;
    let lines = [
        "agg-gui renders text by", "shaping with rustybuzz,",
        "extracting outlines via", "ttf-parser, and feeding",
        "Bezier curves into AGG.", "",
        "No glyph atlas. Kerning", "and hinting are preserved.",
    ];
    let mut y = py + ph * 0.82;
    for line in lines.iter() {
        if !line.is_empty() { ctx.fill_text(line, x, y); }
        y -= line_h;
    }
}

fn draw_buttons_panel_text(ctx: &mut GfxCtx, px: f64, py: f64, pw: f64, ph: f64) {
    panel_title_gsv(ctx, px, py, pw, ph, "Text + Graphics");
    let margin = pw * 0.07;
    let btn_h = ph * 0.16;
    let btn_r = btn_h * 0.35;
    let bx = px + margin;
    let bw = pw - margin * 2.0;
    let buttons: &[(&str, Color, Color)] = &[
        ("Primary Action",  Color::rgb(0.22, 0.45, 0.88), Color::white()),
        ("Secondary",       Color::rgba(0.22, 0.45, 0.88, 0.12), Color::rgb(0.22, 0.45, 0.88)),
        ("Destructive",     Color::rgb(0.88, 0.25, 0.18), Color::white()),
        ("Disabled",        Color::rgba(0.0, 0.0, 0.0, 0.08), Color::rgba(0.0,0.0,0.0,0.3)),
    ];
    let spacing = (ph * 0.74) / buttons.len() as f64;
    let font_size = (btn_h * 0.38).clamp(10.0, 16.0);
    ctx.set_font_size(font_size);
    for (i, &(label, bg, fg)) in buttons.iter().enumerate() {
        let by = py + ph * 0.78 - i as f64 * spacing;
        ctx.set_fill_color(bg);
        ctx.set_blend_mode(CompOp::SrcOver);
        ctx.begin_path(); ctx.rounded_rect(bx, by - btn_h * 0.5, bw, btn_h, btn_r); ctx.fill();
        if let Some(m) = ctx.measure_text(label) {
            let tx = bx + (bw - m.width) * 0.5;
            let ty = by - m.ascent * 0.45 + m.descent * 0.45;
            ctx.set_fill_color(fg);
            ctx.fill_text(label, tx, ty);
        }
    }
}

fn build_tree_demo(font: Arc<Font>) -> TreeView {
    let mut tv = TreeView::new(Arc::clone(&font))
        .with_row_height(26.0)
        .with_font_size(13.0)
        .with_indent_width(18.0);

    let alpha = tv.add_root("Project Alpha", NodeIcon::Package);
    tv.expand(alpha);
    let src = tv.add_child(alpha, "src", NodeIcon::Folder);
    tv.expand(src);
    tv.add_child(src, "main.rs", NodeIcon::File);
    tv.add_child(src, "lib.rs", NodeIcon::File);
    let widgets_dir = tv.add_child(src, "widgets", NodeIcon::Folder);
    tv.expand(widgets_dir);
    tv.add_child(widgets_dir, "button.rs", NodeIcon::File);
    tv.add_child(widgets_dir, "scroll_view.rs", NodeIcon::File);
    tv.add_child(widgets_dir, "tree_view.rs", NodeIcon::File);
    let tests = tv.add_child(alpha, "tests", NodeIcon::Folder);
    tv.expand(tests);
    tv.add_child(tests, "integration.rs", NodeIcon::File);
    tv.add_child(tests, "unit.rs", NodeIcon::File);
    tv.add_child(alpha, "Cargo.toml", NodeIcon::File);

    let beta = tv.add_root("Project Beta", NodeIcon::Package);
    let bsrc = tv.add_child(beta, "src", NodeIcon::Folder);
    tv.add_child(bsrc, "app.rs", NodeIcon::File);
    tv.add_child(bsrc, "config.rs", NodeIcon::File);
    tv.add_child(beta, "Cargo.toml", NodeIcon::File);

    let gamma = tv.add_root("Project Gamma", NodeIcon::Package);
    let gsrc = tv.add_child(gamma, "src", NodeIcon::Folder);
    tv.add_child(gsrc, "main.rs", NodeIcon::File);
    tv.add_child(gsrc, "render.rs", NodeIcon::File);
    tv.add_child(gamma, "Cargo.toml", NodeIcon::File);

    tv
}

fn map_key(key: &WinitKey) -> Option<AggKey> {
    Some(match key {
        WinitKey::Named(NamedKey::ArrowUp)    => AggKey::ArrowUp,
        WinitKey::Named(NamedKey::ArrowDown)  => AggKey::ArrowDown,
        WinitKey::Named(NamedKey::ArrowLeft)  => AggKey::ArrowLeft,
        WinitKey::Named(NamedKey::ArrowRight) => AggKey::ArrowRight,
        WinitKey::Named(NamedKey::Enter)      => AggKey::Enter,
        WinitKey::Named(NamedKey::Space)      => AggKey::Char(' '),
        WinitKey::Named(NamedKey::Tab)        => AggKey::Tab,
        WinitKey::Named(NamedKey::Escape)     => AggKey::Escape,
        WinitKey::Named(NamedKey::Backspace)  => AggKey::Backspace,
        WinitKey::Character(s) => AggKey::Char(s.chars().next()?),
        _ => return None,
    })
}

fn build_flex_demo(font: Arc<Font>) -> FlexColumn {
    let fa = Arc::clone(&font);
    let fb = Arc::clone(&font);
    let fc = Arc::clone(&font);
    let fd = Arc::clone(&font);
    let fe = Arc::clone(&font);

    let row = FlexRow::new()
        .with_gap(8.0)
        .add_flex(Box::new(Button::new("One",   fa).with_font_size(13.0).on_click(|| println!("One"))), 1.0)
        .add_flex(Box::new(Button::new("Two",   fb).with_font_size(13.0).on_click(|| println!("Two"))), 1.0)
        .add_flex(Box::new(Button::new("Three", fc).with_font_size(13.0).on_click(|| println!("Three"))), 1.0);

    FlexColumn::new()
        .with_gap(12.0)
        .with_padding(20.0)
        .with_background(Color::rgb(0.94, 0.94, 0.96))
        .add(Box::new(SizedBox::new().with_height(36.0).with_child(Box::new(row))))
        .add(Box::new(SizedBox::new().with_height(36.0).with_child(Box::new(
            TextField::new(fd).with_font_size(14.0).with_placeholder("Search…"),
        ))))
        .add_flex(Box::new(Spacer::new()), 1.0)
        .add(Box::new(SizedBox::new().with_height(36.0).with_child(Box::new(
            Button::new("Confirm", fe).with_font_size(14.0).on_click(|| println!("Confirm")),
        ))))
}

fn build_scroll_demo(font: Arc<Font>) -> ScrollView {
    let mut col = FlexColumn::new()
        .with_gap(8.0)
        .with_padding(16.0)
        .with_background(Color::rgb(0.94, 0.94, 0.96));

    for i in 0..24u32 {
        let label = format!("Item {:02}", i + 1);
        col.push(
            Box::new(SizedBox::new().with_height(40.0).with_child(Box::new(
                Button::new(label, Arc::clone(&font))
                    .with_font_size(13.0)
                    .on_click(|| {}),
            ))),
            0.0,
        );
    }

    ScrollView::new(Box::new(col))
}

fn build_split_demo(font: Arc<Font>) -> Splitter {
    let fa = Arc::clone(&font);
    let fb = Arc::clone(&font);
    let fc = Arc::clone(&font);
    let fd = Arc::clone(&font);

    let left = FlexColumn::new()
        .with_gap(8.0)
        .with_padding(16.0)
        .with_background(Color::rgb(0.96, 0.96, 0.99))
        .add(Box::new(SizedBox::new().with_height(36.0).with_child(Box::new(
            Button::new("Left A", fa).with_font_size(13.0).on_click(|| println!("Left A")),
        ))))
        .add(Box::new(SizedBox::new().with_height(36.0).with_child(Box::new(
            Button::new("Left B", fb).with_font_size(13.0).on_click(|| println!("Left B")),
        ))))
        .add_flex(Box::new(Spacer::new()), 1.0);

    let right = FlexColumn::new()
        .with_gap(8.0)
        .with_padding(16.0)
        .with_background(Color::rgb(0.99, 0.96, 0.96))
        .add(Box::new(SizedBox::new().with_height(36.0).with_child(Box::new(
            TextField::new(fc).with_font_size(13.0).with_placeholder("Right field…"),
        ))))
        .add(Box::new(SizedBox::new().with_height(36.0).with_child(Box::new(
            Button::new("Action", fd).with_font_size(13.0).on_click(|| println!("Action")),
        ))))
        .add_flex(Box::new(Spacer::new()), 1.0);

    Splitter::new(Box::new(left), Box::new(right)).with_ratio(0.4)
}

fn map_mouse_button(b: &winit::event::MouseButton) -> AggMouseButton {
    match b {
        winit::event::MouseButton::Left   => AggMouseButton::Left,
        winit::event::MouseButton::Right  => AggMouseButton::Right,
        winit::event::MouseButton::Middle => AggMouseButton::Middle,
        winit::event::MouseButton::Other(n) => AggMouseButton::Other(*n as u8),
        _ => AggMouseButton::Other(255),
    }
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

fn main() {
    let event_loop = EventLoop::new().expect("EventLoop::new");

    let window_attributes = WindowAttributes::default()
        .with_title("agg-gui — Phase 8 Demo (GL Cube)")
        .with_inner_size(LogicalSize::new(1280u32, 720u32));

    let template = ConfigTemplateBuilder::new().with_alpha_size(0);
    let display_builder =
        DisplayBuilder::new().with_window_attributes(Some(window_attributes));

    let (window, gl_config) = display_builder
        .build(&event_loop, template, |configs| {
            configs
                .reduce(|a, b| if b.num_samples() > a.num_samples() { b } else { a })
                .expect("no suitable GL config")
        })
        .expect("DisplayBuilder::build");

    let window = window.expect("window");
    let raw_window_handle = window.window_handle().expect("window_handle").as_raw();

    let context_attributes = ContextAttributesBuilder::new()
        .with_context_api(ContextApi::OpenGl(Some(Version::new(3, 3))))
        .build(Some(raw_window_handle));

    let gl_display = gl_config.display();
    let not_current_gl_context = unsafe {
        gl_display
            .create_context(&gl_config, &context_attributes)
            .expect("create_context")
    };

    let size = window.inner_size();
    let surface_attributes = SurfaceAttributesBuilder::<WindowSurface>::new().build(
        raw_window_handle,
        NonZeroU32::new(size.width.max(1)).unwrap(),
        NonZeroU32::new(size.height.max(1)).unwrap(),
    );

    let gl_surface = unsafe {
        gl_display
            .create_window_surface(&gl_config, &surface_attributes)
            .expect("create_window_surface")
    };

    let gl_context = not_current_gl_context
        .make_current(&gl_surface)
        .expect("make_current");

    let gl = unsafe {
        glow::Context::from_loader_function_cstr(|s| gl_display.get_proc_address(s))
    };

    let font = Arc::new(Font::from_slice(FONT_BYTES).expect("parse CascadiaCode.ttf"));
    let mut presenter = unsafe { GlPresenter::new(gl) };
    let mut cube_renderer = unsafe { CubeGlRenderer::new(&presenter.gl) };
    let mut fb = Framebuffer::new(size.width.max(1), size.height.max(1));
    let mut app = build_demo_ui(Arc::clone(&font));

    // Last known cursor position (Y-down, physical pixels).
    let mut cursor_x = 0.0f64;
    let mut cursor_y = 0.0f64;

    render_frame(&mut app, &mut fb, &mut presenter, &mut cube_renderer);

    #[allow(deprecated)]
    event_loop
        .run(|event, elwt| {
            elwt.set_control_flow(ControlFlow::Poll);
            match event {
                Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => {
                    elwt.exit();
                }
                Event::WindowEvent {
                    event: WindowEvent::Resized(new_size), ..
                } => {
                    if new_size.width > 0 && new_size.height > 0 {
                        gl_surface.resize(
                            &gl_context,
                            NonZeroU32::new(new_size.width).unwrap(),
                            NonZeroU32::new(new_size.height).unwrap(),
                        );
                        unsafe {
                            presenter.gl.viewport(0, 0, new_size.width as i32, new_size.height as i32);
                        }
                        fb.resize(new_size.width, new_size.height);
                    }
                }
                Event::WindowEvent {
                    event: WindowEvent::CursorMoved { position, .. }, ..
                } => {
                    cursor_x = position.x;
                    cursor_y = position.y;
                    app.on_mouse_move(cursor_x, cursor_y);
                }
                Event::WindowEvent {
                    event: WindowEvent::CursorLeft { .. }, ..
                } => {
                    app.on_mouse_leave();
                }
                Event::WindowEvent {
                    event: WindowEvent::MouseInput { state, button, .. }, ..
                } => {
                    let btn = map_mouse_button(&button);
                    let mods = Modifiers::default();
                    match state {
                        ElementState::Pressed  => app.on_mouse_down(cursor_x, cursor_y, btn, mods),
                        ElementState::Released => app.on_mouse_up(cursor_x, cursor_y, btn, mods),
                    }
                }
                Event::WindowEvent {
                    event: WindowEvent::KeyboardInput { event: key_event, .. }, ..
                } => {
                    if key_event.state == ElementState::Pressed {
                        if let Some(key) = map_key(&key_event.logical_key) {
                            app.on_key_down(key, Modifiers::default());
                        }
                    }
                }
                Event::WindowEvent {
                    event: WindowEvent::MouseWheel { delta, .. }, ..
                } => {
                    // Winit: LineDelta y > 0 = wheel up = scroll content up = negative delta.
                    // PixelDelta: y > 0 = physical scroll down = positive delta.
                    let delta_y = match delta {
                        winit::event::MouseScrollDelta::LineDelta(_, y) => -(y as f64),
                        winit::event::MouseScrollDelta::PixelDelta(d) => d.y / 40.0,
                    };
                    app.on_mouse_wheel(cursor_x, cursor_y, delta_y);
                }
                Event::AboutToWait => {
                    render_frame(&mut app, &mut fb, &mut presenter, &mut cube_renderer);
                    unsafe { presenter.present() };
                    // Draw the GL cube on top of the uploaded AGG texture.
                    let cube_rect = CUBE_SCREEN_RECT.with(|r| r.get());
                    let h = fb.height() as f64;
                    let fw = fb.width() as i32;
                    let fh = fb.height() as i32;
                    unsafe { cube_renderer.draw_gl(&presenter.gl, cube_rect, h, fw, fh) };
                    gl_surface.swap_buffers(&gl_context).expect("swap_buffers");
                }
                _ => {}
            }
        })
        .expect("event_loop.run");
}

fn render_frame(
    app: &mut App,
    fb: &mut Framebuffer,
    presenter: &mut GlPresenter,
    _cube: &mut CubeGlRenderer,
) {
    let w = fb.width();
    let h = fb.height();
    app.layout(Size::new(w as f64, h as f64));
    {
        let mut ctx = GfxCtx::new(fb);
        app.paint(&mut ctx);

        let lsize = (w as f64 * 0.012).clamp(9.0, 13.0);
        ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, 0.3));
        ctx.fill_text_gsv("agg-gui  Phase 8 — GL Cube", 12.0, 6.0, lsize);
    }
    // Upload AGG framebuffer to GL texture (cube will overdraw its area next).
    unsafe { presenter.update_texture(fb) };
}
