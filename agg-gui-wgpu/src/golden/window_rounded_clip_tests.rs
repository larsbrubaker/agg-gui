//! Port of agg-sharp `Agg.GoldenImages/WindowRoundedClipTests.cs`: a `Window`
//! with a corner radius cuts whatever its title bar and content paint to its
//! rounded corners, on the software `GfxCtx` and through the wgpu backend
//! (with and without the window's own retained layer), while its shadow,
//! drawn outside the panel, is not cut. A square window draws straight onto
//! its parent with no panel layer; under a scaled parent the content still
//! stays inside the arc; and the clip does not anti-alias the border arc a
//! second time.
//!
//! C#'s panel (`window.TitleBar.Parent`) is the visible window inside its
//! grab border; agg-gui's `Window` bounds are that panel (the shadow and
//! resize zones lie outside), so the panel bounds here are the window's.
//! C#'s `GetPixel(x, y)` is bottom-up; the readbacks are top-row-first, so
//! [`px`] flips. C#'s `ClientArea.BackgroundColor = Red` is a content child
//! that fills its bounds red ([`Fill`]). C#'s `NodeEditor`-style zoomed
//! parent is a `scale(0.8)` applied before the tree paints. C#'s
//! `DoubleBuffer` on the window is `with_gl_backbuffer`, and on the panel
//! `Window::set_panel_layer`.

use agg_gui::color::Color;
use agg_gui::draw_ctx::DrawCtx;
use agg_gui::widget::Widget;
use agg_gui::widgets::Window;
use agg_gui::{Event, EventResult, Rect, Size};

use super::harness::{software_tree_with, GpuFrames};

const FRAME_WIDTH: u32 = 260;
const FRAME_HEIGHT: u32 = 200;

/// C#'s `new RectangleDouble(40, 30, 220, 150)`: left, bottom, right, top.
const PANEL: (f64, f64, f64, f64) = (40.0, 30.0, 220.0, 150.0);

/// A content child that fills its bounds with one colour (C#'s
/// `ClientArea.BackgroundColor`); also the frame root.
struct Fill {
    bounds: Rect,
    color: Option<Color>,
    children: Vec<Box<dyn Widget>>,
}

impl Fill {
    fn new(color: Option<Color>) -> Self {
        Self {
            bounds: Rect::default(),
            color,
            children: Vec::new(),
        }
    }
}

impl Widget for Fill {
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
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        if let Some(color) = self.color {
            ctx.set_fill_color(color);
            ctx.begin_path();
            ctx.rect(0.0, 0.0, self.bounds.width, self.bounds.height);
            ctx.fill();
        }
    }
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
}

fn font() -> std::sync::Arc<agg_gui::text::Font> {
    let bytes: &[u8] = include_bytes!("../../assets/fonts/NotoSans-Regular.ttf");
    std::sync::Arc::new(agg_gui::text::Font::from_slice(bytes).expect("parse the test font"))
}

fn red() -> Color {
    Color::from_rgb8(255, 0, 0)
}

/// A window over `PANEL` with a red content area, built by `style`.
fn window(style: impl FnOnce(Window) -> Window) -> Window {
    let (l, b, r, t) = PANEL;
    let w = Window::new("Rounded", font(), Box::new(Fill::new(Some(red()))))
        .with_bounds(Rect::new(l, b, r - l, t - b));
    style(w)
}

/// C#'s `BuildScene`: the window over a white root, its title bar red unless
/// `title_bar_red` is off, with a shadow only when rounded.
fn build_scene(corner_radius: f64, title_bar_red: bool) -> Window {
    window(|mut w| {
        w.set_body_color(Some(Color::from_rgb8(200, 220, 240)));
        w.set_corner_radius(corner_radius);
        w.set_shadow(corner_radius > 0.0);
        w.set_title_bar_color(Some(if title_bar_red {
            red()
        } else {
            Color::transparent()
        }));
        w
    })
}

/// The frame root: white, `FRAME_WIDTH`×`FRAME_HEIGHT`, holding `child`.
fn root(mut child: Window) -> Fill {
    child.layout(Size::new(FRAME_WIDTH as f64, FRAME_HEIGHT as f64));
    Fill {
        bounds: Rect::new(0.0, 0.0, FRAME_WIDTH as f64, FRAME_HEIGHT as f64),
        color: Some(Color::white()),
        children: vec![Box::new(child)],
    }
}

fn window_of(root: &mut Fill) -> &mut Window {
    root.children[0]
        .as_any_mut()
        .and_then(|a| a.downcast_mut::<Window>())
        .expect("the root holds the window")
}

/// C#'s `RenderSoftware`.
fn render_software(root: &mut Fill, before: &dyn Fn(&mut dyn DrawCtx)) -> Vec<u8> {
    software_tree_with(
        FRAME_WIDTH,
        FRAME_HEIGHT,
        Color::white(),
        false,
        root,
        before,
    )
}

/// Pixel `(x, y)` counted bottom-up as C#'s `GetPixel`.
fn px(image: &[u8], x: i32, y: i32) -> [i32; 3] {
    let row = FRAME_HEIGHT as i32 - 1 - y;
    let i = ((row * FRAME_WIDTH as i32 + x) * 4) as usize;
    [image[i] as i32, image[i + 1] as i32, image[i + 2] as i32]
}

fn is_red(p: [i32; 3]) -> bool {
    p[0] > 230 && p[1] < 25 && p[2] < 25
}

/// The panel's bounds in frame space at `scale`: left, bottom, right, top.
fn panel_bounds(window: &Window, scale: f64) -> (f64, f64, f64, f64) {
    let b = window.bounds();
    (
        b.x * scale,
        b.y * scale,
        (b.x + b.width) * scale,
        (b.y + b.height) * scale,
    )
}

fn left(r: (f64, f64, f64, f64)) -> i32 {
    r.0.round_ties_even() as i32
}
fn bottom(r: (f64, f64, f64, f64)) -> i32 {
    r.1.round_ties_even() as i32
}
fn right(r: (f64, f64, f64, f64)) -> i32 {
    r.2.round_ties_even() as i32 - 1
}
fn top(r: (f64, f64, f64, f64)) -> i32 {
    r.3.round_ties_even() as i32 - 1
}

/// C#'s `AssertCornersAreRounded`: at each corner of a radius-8 panel, one
/// pixel in along the diagonal is outside the arc, three in is the content's
/// red, the outermost row and column two pixels from the corner are past the
/// arc, the top arcs are anti-aliased, and the shadow below the panel is
/// still drawn.
fn assert_corners_are_rounded(image: &[u8], window: &Window, what: &str) {
    let panel = panel_bounds(window, 1.0);
    assert!(
        (panel.0 - panel.0.round()).abs() < 1e-9 && (panel.1 - panel.1.round()).abs() < 1e-9,
        "the probes assume a whole-pixel panel, it was {panel:?}"
    );
    let corners = [
        (left(panel), bottom(panel), 1, 1, "bottom-left"),
        (right(panel), bottom(panel), -1, 1, "bottom-right"),
        (left(panel), top(panel), 1, -1, "top-left"),
        (right(panel), top(panel), -1, -1, "top-right"),
    ];
    for (x, y, sx, sy, name) in corners {
        let outside = px(image, x + sx, y + sy);
        let inside = px(image, x + 3 * sx, y + 3 * sy);
        assert!(
            outside[0] - outside[1] < 40,
            "{what}: just outside the {name} arc shows the canvas or shadow, not content, it was {outside:?}"
        );
        for (ex, ey) in [(x + 2 * sx, y), (x, y + 2 * sy)] {
            let along = px(image, ex, ey);
            assert!(
                along[0] - along[1] < 40,
                "{what}: ({ex}, {ey}) on the {name} corner's edge is past the arc, it was {along:?}"
            );
        }
        assert!(
            is_red(inside),
            "{what}: just inside the {name} arc is the content's red, it was {inside:?}"
        );
        // Anti-aliased: only at the top, where the title bar reaches the arc.
        if sy > 0 {
            continue;
        }
        let mut blended = 0;
        let mut dump = String::new();
        for j in 0..8 {
            for i in 0..8 {
                let p = px(image, x + i * sx, y + j * sy);
                dump.push_str(&format!("{p:?}"));
                if p[0] - p[1] > 40 && p[1] > 25 {
                    blended += 1;
                }
            }
            dump.push(' ');
        }
        assert!(
            blended >= 3,
            "{what}: the {name} arc edge is a blend of content and background: {dump}"
        );
    }
    let center_x = ((panel.0 + panel.2) / 2.0) as i32;
    let below = px(image, center_x, bottom(panel) - 2);
    assert!(
        below[0] < 245 && (below[0] - below[1]).abs() < 10,
        "{what}: the shadow below the panel is still drawn, it was {below:?}"
    );
}

#[test]
fn software_content_follows_the_rounded_corners() {
    let mut root = root(build_scene(8.0, true));
    let image = render_software(&mut root, &|_| {});
    assert_corners_are_rounded(&image, window_of(&mut root), "software");
}

#[test]
fn gpu_content_follows_the_rounded_corners() {
    // Both with the window itself double-buffered and without: the panel's
    // rounded layer sits inside the window's own layer in the first.
    for window_buffered in [false, true] {
        let Some(mut frames) = GpuFrames::new(FRAME_WIDTH, FRAME_HEIGHT) else {
            return;
        };
        let mut root = root(build_scene(8.0, true).with_gl_backbuffer(window_buffered));
        let image = frames.paint(Color::white(), &mut root, &|_| {});
        assert_corners_are_rounded(
            &image,
            window_of(&mut root),
            &format!("GPU, window buffered {window_buffered}"),
        );
    }
}

/// C#'s `[Arguments]` rows `(gpu, wasRounded)`: a square window keeps drawing
/// as it always has, with no panel layer and its content reaching the very
/// corner pixels; a window whose radius goes back to 0 drops the layer.
fn a_square_window_is_not_clipped(gpu: bool, was_rounded: bool) {
    let mut root = root(build_scene(if was_rounded { 8.0 } else { 0.0 }, true));
    if was_rounded {
        render_software(&mut root, &|_| {});
        assert!(window_of(&mut root).paints_through_panel_layer());
        let w = window_of(&mut root);
        w.set_corner_radius(0.0);
        w.set_shadow(false);
    }
    let image = if gpu {
        let Some(mut frames) = GpuFrames::new(FRAME_WIDTH, FRAME_HEIGHT) else {
            return;
        };
        frames.paint(Color::white(), &mut root, &|_| {})
    } else {
        render_software(&mut root, &|_| {})
    };
    let w = window_of(&mut root);
    let panel = panel_bounds(w, 1.0);
    assert!(
        !w.paints_through_panel_layer(),
        "a square panel has no layer"
    );
    let spec = w.backbuffer_spec();
    assert!(
        spec.rounded_clip.is_none(),
        "a square window's layer is not clipped"
    );
    // The bottom-left corner is the content's red; the top-right the title
    // bar's.
    for (x, y) in [(left(panel), bottom(panel)), (right(panel), top(panel))] {
        let p = px(&image, x, y);
        assert!(
            is_red(p),
            "the square corner at ({x}, {y}) is content, it was {p:?}"
        );
    }
}

#[test]
fn a_square_window_is_not_clipped_software() {
    a_square_window_is_not_clipped(false, false);
}

#[test]
fn a_square_window_is_not_clipped_gpu() {
    a_square_window_is_not_clipped(true, false);
}

#[test]
fn a_square_window_is_not_clipped_software_was_rounded() {
    a_square_window_is_not_clipped(false, true);
}

/// Under a parent scale well away from 1 (a zoomed-out node editor) the
/// content still stays inside the arc.
#[test]
fn content_stays_inside_the_arc_when_the_parent_is_scaled() {
    const SCALE: f64 = 0.8;
    let mut root = root(build_scene(8.0, false));
    let image = render_software(&mut root, &|ctx| ctx.scale(SCALE, SCALE));
    let panel = panel_bounds(window_of(&mut root), 1.0);
    let l = (panel.0 * SCALE).round_ties_even() as i32;
    let r = (panel.2 * SCALE).round_ties_even() as i32 - 1;
    let b = (panel.1 * SCALE).round_ties_even() as i32;
    for (x, name) in [(l, "bottom-left"), (r, "bottom-right")] {
        let p = px(&image, x, b);
        assert!(
            p[0] - p[1] < 40,
            "the {name} corner pixel at 0.8x lies outside the arc, it was {p:?}"
        );
    }
    // And the probe is on the panel: a few pixels in along the diagonal is
    // the red content.
    let inside = px(&image, l + 4, b + 4);
    assert!(
        is_red(inside),
        "inside the scaled panel is content, it was {inside:?}"
    );
}

/// The panel's own rounded border is anti-aliased once, by its own
/// rasterizer; the clip must not attenuate the arc a second time. The ink in
/// the corner through the clip is compared with the same panel drawn without
/// its layer (so without the clip).
fn the_border_arc_is_not_cut_twice(gpu: bool) {
    let corner_ink = |clipped: bool| -> Option<i64> {
        let (l, b, r, t) = PANEL;
        let mut w = Window::new("Border", font(), Box::new(Fill::new(None)))
            .with_bounds(Rect::new(l, b, r - l, t - b));
        w.set_body_color(Some(Color::white()));
        w.set_corner_radius(8.0);
        // C#'s default transparent ShadowColor: no shadow in the corner.
        w.set_shadow(false);
        w.set_border(1.5, Some(Color::black()));
        w.set_panel_layer(clipped);
        let mut root = root(w);
        let image = if gpu {
            let mut frames = GpuFrames::new(FRAME_WIDTH, FRAME_HEIGHT)?;
            frames.paint(Color::white(), &mut root, &|_| {})
        } else {
            render_software(&mut root, &|_| {})
        };
        assert_eq!(
            window_of(&mut root).paints_through_panel_layer(),
            clipped,
            "the panel layer follows set_panel_layer"
        );
        let panel = panel_bounds(window_of(&mut root), 1.0);
        let mut ink = 0i64;
        for y in bottom(panel)..bottom(panel) + 8 {
            for x in left(panel)..left(panel) + 8 {
                ink += 255 - px(&image, x, y)[1] as i64;
            }
        }
        Some(ink)
    };
    let (Some(direct), Some(through_clip)) = (corner_ink(false), corner_ink(true)) else {
        return;
    };
    assert!(
        through_clip as f64 >= direct as f64 * 0.93,
        "the border arc through the clip has {through_clip} ink against {direct} drawn directly"
    );
}

#[test]
fn the_border_arc_is_not_cut_twice_software() {
    the_border_arc_is_not_cut_twice(false);
}

#[test]
fn the_border_arc_is_not_cut_twice_gpu() {
    the_border_arc_is_not_cut_twice(true);
}
