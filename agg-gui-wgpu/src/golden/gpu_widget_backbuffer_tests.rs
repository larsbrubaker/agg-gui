//! Port of agg-sharp `Agg.GoldenImages/GpuWidgetBackbufferTests.cs`: a
//! double-buffered widget on the GPU keeps its pixels in a retained GPU
//! layer (agg-gui `BackbufferKind::GlFbo`, C#'s `BackbufferMode.GpuTexture`)
//! and composites the same as the widget drawn straight onto the frame;
//! a clean layer composites without repainting, an invalidated or resized
//! one repaints, a faded one blends through its alpha, translucent fills
//! and images inside it accumulate alpha correctly, and a change of
//! coordinate scale repaints it.
//!
//! C#'s `PaintCountingWidget` is a `GuiWidget` with `DoubleBuffer`; here it
//! is a test widget that asks for a retained GPU layer through
//! `Widget::backbuffer_spec` when double-buffered, as agg-gui's `Window`
//! does. C#'s `CoordinateScale = 2` is a `scale(2, 2)` on the frame before
//! the tree paints: the layer is sized from the transform's scale, so both
//! change the layer's pixel size.

use std::cell::Cell;
use std::rc::Rc;

use agg_gui::color::Color;
use agg_gui::draw_ctx::DrawCtx;
use agg_gui::widget::{BackbufferKind, BackbufferSpec, BackbufferState, Widget};
use agg_gui::{Event, EventResult, Insets, Rect, Size};

use super::harness::{at, GpuFrames};

const FRAME_W: u32 = 128;
const FRAME_H: u32 = 64;

/// C#'s `new ColorF(0.2f, 0.3f, 0.4f, 1)`.
fn background() -> Color {
    Color::rgba(0.2, 0.3, 0.4, 1.0)
}

type ExtraPaint = Box<dyn Fn(&mut dyn DrawCtx)>;

/// Counts its paints and draws opaque, translucent and anti-aliased ink, so a
/// lost alpha, a flip or a shift each shows.
struct PaintCountingWidget {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    background: Color,
    double_buffer: bool,
    alpha: f64,
    backbuffer: BackbufferState,
    paints: Rc<Cell<usize>>,
    extra: Option<ExtraPaint>,
}

impl Widget for PaintCountingWidget {
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }
    fn layout(&mut self, _available: Size) -> Size {
        Size::new(self.bounds.width, self.bounds.height)
    }
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        self.paints.set(self.paints.get() + 1);
        let (w, h) = (self.bounds.width, self.bounds.height);
        // A C# widget draws clipped to its bounds; agg-gui's direct paint
        // leaves that to the widget, and the layer clips by its size.
        ctx.save();
        ctx.clip_rect(0.0, 0.0, w, h);
        // C#'s OnDrawBackground.
        ctx.set_fill_color(self.background);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, w, h);
        ctx.fill();
        // FillRectangle(2, 2, 14, 10, Red): left, bottom, right, top.
        ctx.set_fill_color(Color::from_rgb8(255, 0, 0));
        ctx.begin_path();
        ctx.rect(2.0, 2.0, 12.0, 8.0);
        ctx.fill();
        ctx.set_fill_color(Color::from_rgba8(0, 160, 0, 200));
        ctx.begin_path();
        ctx.ellipse(
            w - 10.0,
            h - 9.0,
            7.0,
            6.0,
            0.0,
            0.0,
            2.0 * std::f64::consts::PI,
            false,
        );
        ctx.close_path();
        ctx.fill();
        ctx.set_stroke_color(Color::black());
        ctx.set_line_width(1.5);
        ctx.begin_path();
        ctx.move_to(0.0, 0.0);
        ctx.line_to(w, h);
        ctx.stroke();
        if let Some(extra) = &self.extra {
            extra(ctx);
        }
        ctx.restore();
    }
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
    fn backbuffer_spec(&mut self) -> BackbufferSpec {
        if !self.double_buffer {
            return BackbufferSpec::none();
        }
        BackbufferSpec {
            kind: BackbufferKind::GlFbo,
            cached: true,
            alpha: self.alpha,
            outsets: Insets::ZERO,
            rounded_clip: None,
        }
    }
    fn backbuffer_state_mut(&mut self) -> Option<&mut BackbufferState> {
        if self.double_buffer {
            Some(&mut self.backbuffer)
        } else {
            None
        }
    }
}

/// A plain root holding the child at (10, 12).
struct Root {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl Widget for Root {
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds;
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
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// C#'s `BuildTree`: the root and a counter of the child's paints.
fn build_tree(double_buffer: bool, width: f64) -> (Root, Rc<Cell<usize>>) {
    build_tree_with(
        double_buffer,
        width,
        Color::from_rgba8(255, 255, 255, 160),
        None,
    )
}

fn build_tree_with(
    double_buffer: bool,
    width: f64,
    background: Color,
    extra: Option<ExtraPaint>,
) -> (Root, Rc<Cell<usize>>) {
    let paints = Rc::new(Cell::new(0));
    let child = PaintCountingWidget {
        bounds: Rect::new(10.0, 12.0, width, 30.0),
        children: Vec::new(),
        background,
        double_buffer,
        alpha: 1.0,
        backbuffer: BackbufferState::new(),
        paints: Rc::clone(&paints),
        extra,
    };
    let root = Root {
        bounds: Rect::new(0.0, 0.0, FRAME_W as f64, FRAME_H as f64),
        children: vec![Box::new(child)],
    };
    (root, paints)
}

fn child_mut(root: &mut Root) -> &mut PaintCountingWidget {
    // The only child `build_tree` adds is a `PaintCountingWidget`.
    root.children[0]
        .as_any_mut()
        .and_then(|a| a.downcast_mut::<PaintCountingWidget>())
        .expect("the child is a PaintCountingWidget")
}

fn render(frames: &mut GpuFrames, root: &mut Root) -> Vec<u8> {
    frames.paint(background(), root, &|_| {})
}

/// C#'s `AssertImagesClose`: the worst RGB channel difference.
fn assert_images_close(expected: &[u8], actual: &[u8], what: &str, tolerance: i32) {
    assert_images_close_sized(expected, actual, (FRAME_W, FRAME_H), what, tolerance);
}

/// [`assert_images_close`] for a capture of `(w, h)`.
fn assert_images_close_sized(
    expected: &[u8],
    actual: &[u8],
    (w, h): (u32, u32),
    what: &str,
    tolerance: i32,
) {
    let mut worst = (0, 0, 0);
    for y in 0..h {
        for x in 0..w {
            let (a, b) = (at(expected, w, x, y), at(actual, w, x, y));
            let d = (0..3)
                .map(|c| (a[c] as i32 - b[c] as i32).abs())
                .max()
                .unwrap_or(0);
            if d > worst.0 {
                worst = (d, x, y);
            }
        }
    }
    let (d, x, y) = worst;
    assert!(
        d <= tolerance,
        "{what}: worst channel delta {d} at ({x}, {y}) top-row-first, expected {:?} got {:?}",
        at(expected, w, x, y),
        at(actual, w, x, y)
    );
}

#[test]
fn a_buffered_widget_on_the_gpu_matches_the_unbuffered_widget() {
    let Some(mut frames) = GpuFrames::new(FRAME_W, FRAME_H) else {
        return;
    };
    let (mut plain, _) = build_tree(false, 40.0);
    let expected = render(&mut frames, &mut plain);
    let (mut root, _) = build_tree(true, 40.0);
    let actual = render(&mut frames, &mut root);
    assert_eq!(
        child_mut(&mut root).backbuffer.repaint_count,
        1,
        "the widget painted into its GPU layer"
    );
    assert!(
        child_mut(&mut root).backbuffer.cache.pixels.is_none(),
        "the pixels live in the GPU texture, and there is no CPU copy of them"
    );
    assert_images_close(&expected, &actual, "a GPU backbuffered widget", 1);
}

#[test]
fn a_clean_widget_is_composited_without_repainting_and_invalidating_repaints() {
    let Some(mut frames) = GpuFrames::new(FRAME_W, FRAME_H) else {
        return;
    };
    let (mut plain, _) = build_tree(false, 40.0);
    let expected = render(&mut frames, &mut plain);

    let (mut root, paints) = build_tree(true, 40.0);
    render(&mut frames, &mut root);
    assert_eq!(paints.get(), 1);

    let retained = render(&mut frames, &mut root);
    assert_eq!(
        paints.get(),
        1,
        "a clean widget draws its retained texture and paints nothing"
    );
    assert_images_close(
        &expected,
        &retained,
        "the retained texture on a later frame",
        1,
    );

    child_mut(&mut root).backbuffer.dirty = true;
    render(&mut frames, &mut root);
    assert_eq!(paints.get(), 2);

    // Resizing repaints at the new size rather than stretching the old picture.
    plain.children[0].set_bounds(Rect::new(10.0, 12.0, 60.0, 30.0));
    root.children[0].set_bounds(Rect::new(10.0, 12.0, 60.0, 30.0));
    let expected = render(&mut frames, &mut plain);
    let resized = render(&mut frames, &mut root);
    assert_eq!(paints.get(), 3);
    assert_images_close(&expected, &resized, "the resized widget", 1);
}

#[test]
fn a_faded_widget_on_the_gpu_composites_through_its_layer() {
    let Some(mut frames) = GpuFrames::new(FRAME_W, FRAME_H) else {
        return;
    };
    let (mut root, _) = build_tree(true, 40.0);
    child_mut(&mut root).alpha = 0.5;
    let faded = render(&mut frames, &mut root);
    assert_eq!(child_mut(&mut root).backbuffer.repaint_count, 1);

    // The red square (off the black diagonal), opaque in the layer, lands
    // half way between red and the background. C#'s GetPixel is y up.
    let pixel = at(&faded, FRAME_W, 10 + 4, FRAME_H - 1 - (12 + 8));
    assert!(
        (pixel[0] as i32 - (255 + 51) / 2).abs() <= 2 && (pixel[2] as i32 - 102 / 2).abs() <= 2,
        "the faded red square is {pixel:?}"
    );
}

/// The straight-alpha draws — a translucent pixel-aligned fill and a partly
/// transparent image — over the widget's opaque background and over
/// transparent parts of the layer must accumulate alpha as
/// `a + dstA(1 - a)` inside the premultiplied layer.
#[test]
fn translucent_fills_and_images_inside_a_layer_match_the_unbuffered_widget() {
    let Some(mut frames) = GpuFrames::new(FRAME_W, FRAME_H) else {
        return;
    };
    // C#'s 12×12 image, SetPixel with y up; agg-gui images are top row first.
    let mut image = vec![0u8; 12 * 12 * 4];
    for y in 0..12usize {
        for x in 0..12usize {
            let i = ((11 - y) * 12 + x) * 4;
            image[i..i + 4].copy_from_slice(&[
                (20 * x) as u8,
                200,
                (20 * y) as u8,
                (20 * (x + y) + 10) as u8,
            ]);
        }
    }
    let image = Rc::new(image);
    for background_alpha in [255u8, 0] {
        let build = |double_buffer: bool| {
            let image = Rc::clone(&image);
            build_tree_with(
                double_buffer,
                40.0,
                Color::from_rgba8(240, 240, 250, background_alpha),
                Some(Box::new(move |ctx: &mut dyn DrawCtx| {
                    // FillRectangle(16, 4, 36, 20): left, bottom, right, top.
                    ctx.set_fill_color(Color::from_rgba8(0, 0, 255, 100));
                    ctx.begin_path();
                    ctx.rect(16.0, 4.0, 20.0, 16.0);
                    ctx.fill();
                    ctx.draw_image_rgba(&image, 12, 12, 22.0, 14.0, 12.0, 12.0);
                })),
            )
        };
        let (mut plain, _) = build(false);
        let expected = render(&mut frames, &mut plain);
        let (mut buffered, _) = build(true);
        let actual = render(&mut frames, &mut buffered);
        assert_images_close(
            &expected,
            &actual,
            &format!(
                "translucent fills and images in a layer, background alpha {background_alpha}"
            ),
            1,
        );
    }
}

/// A layer is painted at the coordinate scale of the moment, so moving
/// between scale 1 and a supersampled frame repaints it.
#[test]
fn a_coordinate_scale_change_repaints_the_layer() {
    let Some(mut frames) = GpuFrames::new(FRAME_W, FRAME_H) else {
        return;
    };
    let (mut root, paints) = build_tree(true, 40.0);
    render(&mut frames, &mut root);
    render(&mut frames, &mut root);
    assert_eq!(paints.get(), 1);

    frames.paint(background(), &mut root, &|ctx| ctx.scale(2.0, 2.0));
    assert_eq!(
        paints.get(),
        2,
        "the 1x layer is not sharp at coordinate scale 2"
    );

    let image = render(&mut frames, &mut root);
    assert_eq!(
        paints.get(),
        3,
        "the layer painted at scale 2 is twice the window's resolution"
    );

    let (mut plain, _) = build_tree(false, 40.0);
    let expected = render(&mut frames, &mut plain);
    assert_images_close(&expected, &image, "the layer back at scale 1", 1);
}

fn font() -> std::sync::Arc<agg_gui::text::Font> {
    let bytes: &[u8] = include_bytes!("../../assets/fonts/NotoSans-Regular.ttf");
    std::sync::Arc::new(agg_gui::text::Font::from_slice(bytes).expect("parse the test font"))
}

/// A frame root of `w`×`h` holding `child`.
fn root_with(w: u32, h: u32, child: Box<dyn Widget>) -> Root {
    Root {
        bounds: Rect::new(0.0, 0.0, w as f64, h as f64),
        children: vec![child],
    }
}

/// C#'s styled `WindowWidget` sets its body, corner radius, shadow and title
/// bar colours one by one; agg-gui's `Window` takes all of them from the
/// theme (rounded corners, drop shadow, title bar), so the port builds a
/// themed window and checks the same thing: drawn through its retained GPU
/// layer it matches the window drawn straight onto the frame, with no CPU
/// copy of its pixels.
#[test]
fn a_styled_window_widget_renders_through_its_gpu_layer() {
    let (w, h) = (FRAME_W * 2, FRAME_W);
    let Some(mut frames) = GpuFrames::new(w, h) else {
        return;
    };
    let build = |double_buffer: bool| {
        let content = Box::new(Root {
            bounds: Rect::default(),
            children: Vec::new(),
        });
        let mut window = agg_gui::widgets::Window::new("Styled", font(), content)
            .with_bounds(Rect::new(30.0, 20.0, 170.0, 90.0))
            .with_gl_backbuffer(double_buffer);
        window.layout(Size::new(w as f64, h as f64));
        root_with(w, h, Box::new(window))
    };
    let expected = frames.paint(Color::white(), &mut build(false), &|_| {});
    let mut root = build(true);
    let actual = frames.paint(Color::white(), &mut root, &|_| {});

    let inked = actual
        .chunks(4)
        .filter(|p| p[..3] != [255, 255, 255])
        .count();
    assert!(
        inked > 170 * 90 / 2,
        "the window is drawn ({inked} non-white pixels)"
    );
    let window = &mut root.children[0];
    assert_eq!(window.backbuffer_spec().kind, BackbufferKind::GlFbo);
    assert!(
        window.backbuffer_cache_mut().is_none(),
        "the window keeps no CPU backbuffer"
    );
    assert!(
        window
            .backbuffer_state_mut()
            .is_some_and(|s| s.repaint_count == 1),
        "the window painted into its GPU layer"
    );
    // 2, not 1: at the rounded corners the background, border and title bar
    // each lay a partial halo on the same pixel, and the layer stores that
    // stack in 8 bits before compositing it - one more rounding than drawing
    // direct, worth up to a level on a pixel only ~20% covered.
    assert_images_close_sized(
        &expected,
        &actual,
        (w, h),
        "a styled window through its GPU layer",
        2,
    );
}

/// With LCD text on, labels inside a rounded window's GPU layer are subpixel
/// (the window paints its opaque body into the layer first). The window
/// composites the same whether or not it buffers itself, and the label's
/// subpixel pixels match the software path's. One label crosses the
/// window's rounded corner: the LCD passes write colour, never alpha, so
/// they are only safe because the body fills the whole layer opaque.
#[test]
fn lcd_text_in_a_rounded_windows_gpu_layer_is_subpixel_and_matches_the_cpu_path() {
    use agg_gui::font_settings::{clear_lcd_enabled_override, set_lcd_enabled};
    use agg_gui::widgets::Label;

    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            clear_lcd_enabled_override();
        }
    }
    let _restore = Restore;
    let (w, h) = (FRAME_W * 2, FRAME_W);
    let Some(mut frames) = GpuFrames::new(w, h) else {
        return;
    };
    set_lcd_enabled(true);
    frames.set_lcd_mode(true);

    let label_at = |text: &str, x: f64, y: f64| -> Box<dyn Widget> {
        let mut label = Label::new(text, font())
            .with_font_size(12.0)
            .with_color(Color::black());
        let size = label.layout(Size::new(200.0, 90.0));
        label.set_bounds(Rect::new(x, y, size.width, size.height));
        Box::new(label)
    };
    // The label's screen rect: the window's content origin (20, 10) plus the
    // label's (20, 40).
    let text_rect = {
        let probe = label_at("Hello World", 0.0, 0.0);
        Rect::new(40.0, 50.0, probe.bounds().width, probe.bounds().height)
    };
    let build = |double_buffer: bool| {
        let content = Box::new(Root {
            bounds: Rect::default(),
            children: vec![
                label_at("Hello World", 20.0, 40.0),
                label_at("Corner", -6.0, -4.0),
            ],
        });
        let mut window = agg_gui::widgets::Window::new("LCD", font(), content)
            .with_bounds(Rect::new(20.0, 10.0, 200.0, 90.0 + 28.0))
            .with_gl_backbuffer(double_buffer);
        window.layout(Size::new(w as f64, h as f64));
        root_with(w, h, Box::new(window))
    };

    let plain = frames.paint(Color::white(), &mut build(false), &|_| {});
    let gpu = frames.paint(Color::white(), &mut build(true), &|_| {});
    let cpu =
        super::harness::software_tree_with(w, h, Color::white(), true, &mut build(true), &|_| {});

    let chroma = gpu
        .chunks(4)
        .any(|p| (p[0] as i32 - p[2] as i32).abs() > 24);
    assert!(
        chroma,
        "black LCD text on a white body has coloured fringes"
    );
    assert_images_close_sized(
        &plain,
        &gpu,
        (w, h),
        "a rounded window with LCD text through its own GPU layer",
        2,
    );

    // Over the label only: vector edges (the border's corner) tessellate on
    // the GPU and scanline in software, but the LCD text is the same
    // CPU-built mask on both. Rows are top-row-first.
    let mut worst = 0;
    for y in text_rect.y as u32..(text_rect.y + text_rect.height) as u32 {
        for x in text_rect.x as u32..(text_rect.x + text_rect.width) as u32 {
            let row = h - 1 - y;
            let (a, b) = (at(&cpu, w, x, row), at(&gpu, w, x, row));
            let d = (0..3)
                .map(|c| (a[c] as i32 - b[c] as i32).abs())
                .max()
                .unwrap_or(0);
            worst = worst.max(d);
        }
    }
    assert!(
        worst <= 3,
        "the GPU layer's LCD text matches the software path's (worst {worst})"
    );
}

/// C# gives a double-buffered widget on a software surface (no retained
/// layers) an RGBA CPU backbuffer. agg-gui's retained layers are GPU only
/// (`BackbufferKind::GlFbo`); on the software `GfxCtx`, which supports no
/// retained layers, such a widget paints straight into the frame, once, with
/// no layer and no CPU copy. Software surfaces have no compositor to save,
/// so the CPU copy C# keeps would only cost memory.
#[test]
fn software_surfaces_keep_the_rgba_backbuffer() {
    use agg_gui::framebuffer::Framebuffer;
    use agg_gui::gfx_ctx::GfxCtx;
    use agg_gui::widget::paint_subtree;

    let (mut root, paints) = build_tree(true, 40.0);
    let mut fb = Framebuffer::new(FRAME_W, FRAME_H);
    let mut ctx = GfxCtx::new(&mut fb);
    assert!(!ctx.supports_retained_layers());
    paint_subtree(&mut root, &mut ctx);
    assert_eq!(paints.get(), 1);
    let child = child_mut(&mut root);
    assert_eq!(child.backbuffer.repaint_count, 0, "no layer was painted");
    assert!(child.backbuffer.cache.pixels.is_none());
}
