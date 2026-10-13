//! The rich editor's Raw block hosts draw in the editor's theme colours, not the app's visuals
//! (no C# counterpart: C#'s hosts read the same ThemeConfig as the editor).  A fenced code block
//! inside a quote is a Raw block, so it is shown by a `MarkdownView` host: in a dark editor it
//! paints the dark theme's text colour on the dark theme's code shading, whatever visuals the app
//! runs under, and an editor with no theme paints its hosts exactly as before.

use agg_gui::geometry::Point;
use agg_gui::theme::{set_visuals, Visuals};
use agg_gui::widget::{paint_subtree, set_current_viewport};
use agg_gui::widgets::markdown_editing::{RichBlockKind, RichMarkdownEditWidget, RichThemeColors};
use agg_gui::{Color, Framebuffer, GfxCtx, Rect, Size, Widget};

use super::rich_markdown_edit_widget_tests::fonts;

const MARKDOWN: &str = "Before\n\n> ```\n> let value = 42;\n> ```\n\nAfter\n";
const WIDTH: f64 = 400.0;
const HEIGHT: f64 = 240.0;

/// The pixels of the editor painted on `clear` under `app_visuals`, and the Raw block's host box
/// in framebuffer pixels (Y up).
fn paint(
    theme: Option<RichThemeColors>,
    app_visuals: Visuals,
    clear: Color,
) -> (Framebuffer, Rect) {
    set_visuals(app_visuals);
    set_current_viewport(Size::new(WIDTH, HEIGHT));
    let mut editor = RichMarkdownEditWidget::new(fonts(), theme);
    editor.set_markdown(MARKDOWN).unwrap();
    editor.layout(Size::new(WIDTH, HEIGHT));
    editor.set_bounds(Rect::new(0.0, 0.0, WIDTH, HEIGHT));
    let mut fb = Framebuffer::new(WIDTH as u32, HEIGHT as u32);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(clear);
        paint_subtree(&mut editor, &mut ctx);
    }
    assert_eq!(editor.document().blocks[1].kind, RichBlockKind::Raw);
    let in_view = editor
        .raw_block_widget(1)
        .expect("the quoted code block has a host")
        .bounds_in_view();
    let low = editor.view_to_editor(Point::new(in_view.x, in_view.y));
    let high = editor.view_to_editor(Point::new(
        in_view.x + in_view.width,
        in_view.y + in_view.height,
    ));
    let (x0, x1) = (low.x.min(high.x), low.x.max(high.x));
    let (y0, y1) = (low.y.min(high.y), low.y.max(high.y));
    (fb, Rect::new(x0, y0, x1 - x0, y1 - y0))
}

fn rgb(fb: &Framebuffer, x: i32, y: i32) -> [u8; 3] {
    let i = ((y as usize * fb.width() as usize) + x as usize) * 4;
    let p = fb.pixels();
    [p[i], p[i + 1], p[i + 2]]
}

fn luma(c: [u8; 3]) -> u32 {
    (u32::from(c[0]) * 3 + u32::from(c[1]) * 6 + u32::from(c[2])) / 10
}

/// The pixel in `host` furthest in luma from `against`: the text ink.
fn ink(fb: &Framebuffer, host: Rect, against: [u8; 3]) -> [u8; 3] {
    let mut best = against;
    for y in host.y as i32..(host.y + host.height) as i32 {
        for x in host.x as i32..(host.x + host.width) as i32 {
            let c = rgb(fb, x, y);
            if luma(c).abs_diff(luma(against)) > luma(best).abs_diff(luma(against)) {
                best = c;
            }
        }
    }
    best
}

fn fnv(fb: &Framebuffer) -> u64 {
    fb.pixels().iter().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100_0000_01b3)
    })
}

const DARK_BACKGROUND: [u8; 3] = [30, 32, 36];
const DARK_TEXT: [u8; 3] = [235, 237, 240];

fn dark_theme() -> RichThemeColors {
    let byte = |c: [u8; 3]| Color::from_rgb8(c[0], c[1], c[2]);
    RichThemeColors {
        text: byte(DARK_TEXT),
        accent: Color::from_rgb8(0x7A, 0xD7, 0xF0),
        background: byte(DARK_BACKGROUND),
    }
}

fn close(a: [u8; 3], b: [u8; 3], tolerance: i32) -> bool {
    (0..3).all(|i| (i32::from(a[i]) - i32::from(b[i])).abs() <= tolerance)
}

/// Under the light app visuals the dark editor's code block still paints its own theme: the dark
/// text colour on the text colour's 24/255 shading over the dark background.
#[test]
fn rust_only_a_code_block_in_a_dark_editor_paints_the_dark_theme_colours() {
    let clear = Color::from_rgb8(DARK_BACKGROUND[0], DARK_BACKGROUND[1], DARK_BACKGROUND[2]);
    let (fb, host) = paint(Some(dark_theme()), Visuals::light(), clear);

    let shade = 24.0 / 255.0;
    let expected: Vec<u8> = (0..3)
        .map(|i| {
            (f32::from(DARK_BACKGROUND[i]) * (1.0 - shade) + f32::from(DARK_TEXT[i]) * shade)
                .round() as u8
        })
        .collect();
    // Just inside the block's left edge, at mid-height: clear of its text.
    let background = rgb(&fb, host.x as i32 + 2, (host.y + host.height * 0.5) as i32);
    assert!(
        close(background, [expected[0], expected[1], expected[2]], 2),
        "the code shading is the dark text at 24/255 over the dark background: {background:?} vs {expected:?}"
    );

    let text = ink(&fb, host, background);
    assert!(
        close(text, DARK_TEXT, 24),
        "the code text is the dark theme's text colour: {text:?}"
    );
}

/// An editor with a light theme draws its code block dark-on-light under dark app visuals.
#[test]
fn rust_only_a_code_block_in_a_light_editor_paints_the_light_theme_colours_under_dark_visuals() {
    let theme = RichThemeColors {
        text: Color::black(),
        accent: Color::from_rgb8(0x7A, 0xD7, 0xF0),
        background: Color::white(),
    };
    let (fb, host) = paint(Some(theme), Visuals::dark(), Color::white());
    let background = rgb(&fb, host.x as i32 + 2, (host.y + host.height * 0.5) as i32);
    assert!(
        luma(background) > 220,
        "a light shading, not the dark visuals': {background:?}"
    );
    let text = ink(&fb, host, background);
    assert!(luma(text) < 60, "black code text: {text:?}");
}

/// With no theme the host keeps the viewer's default look, pixel for pixel: this checksum was
/// captured from the unchanged code (a code block's grey shading under the light visuals).
#[test]
fn rust_only_an_editor_with_no_theme_paints_its_raw_hosts_as_before() {
    let (fb, host) = paint(None, Visuals::light(), Color::white());
    let background = rgb(&fb, host.x as i32 + 2, (host.y + host.height * 0.5) as i32);
    assert!(
        close(background, [239, 239, 239], 2),
        "the viewer's 0.5 grey at 12%: {background:?}"
    );
    assert_eq!(fnv(&fb), LIGHT_BASELINE);
}

const LIGHT_BASELINE: u64 = 7_768_079_947_327_348_541;

/// A `MarkdownView` given colours fills its box with the background (when set) and draws its text
/// in the text colour, ignoring the app's visuals; given none it draws in the visuals.
#[test]
fn rust_only_a_markdown_view_with_colors_draws_them_instead_of_the_visuals() {
    use agg_gui::widgets::markdown::{MarkdownColors, MarkdownView};
    set_visuals(Visuals::light());
    set_current_viewport(Size::new(200.0, 60.0));
    let colors = MarkdownColors {
        text: Color::from_rgb8(DARK_TEXT[0], DARK_TEXT[1], DARK_TEXT[2]),
        link: Color::from_rgb8(0x7A, 0xD7, 0xF0),
        code_background: Color::from_rgb8(60, 60, 60),
        rule: Color::from_rgb8(90, 90, 90),
        background: Some(Color::from_rgb8(
            DARK_BACKGROUND[0],
            DARK_BACKGROUND[1],
            DARK_BACKGROUND[2],
        )),
    };
    let mut view = MarkdownView::new("Hello world", fonts().regular)
        .with_padding(4.0)
        .with_colors(colors);
    view.layout(Size::new(200.0, 60.0));
    view.set_bounds(Rect::new(0.0, 0.0, 200.0, 60.0));
    let mut fb = Framebuffer::new(200, 60);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::white());
        paint_subtree(&mut view, &mut ctx);
    }
    assert_eq!(
        rgb(&fb, 190, 5),
        DARK_BACKGROUND,
        "the background fills the box"
    );
    let text = ink(&fb, Rect::new(0.0, 0.0, 200.0, 60.0), DARK_BACKGROUND);
    assert!(
        close(text, DARK_TEXT, 24),
        "the text colour draws the text: {text:?}"
    );
}
