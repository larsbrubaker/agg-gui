//! Port of agg-sharp `Tests/Agg.Tests/Agg.UI/RoundedRectOutlineTests.cs`:
//! "stadium" shapes (a rounded rect whose radius is half its height) through
//! [`agg_gui::render_background`] (C#'s `GuiWidget.RenderBackground`) and
//! [`agg_gui::render_progress_fill`] (the fill C#'s `ProgressBar.OnDraw`
//! draws after `OnDrawBackground`). Insetting such a rect without shrinking
//! its radius makes the corner arcs sweep past each other.
//!
//! C# pins `GuiWidget.DeviceScale` to 1 and keys the class `[NotInParallel]`;
//! agg-gui's device scale is thread local and these paint at scale 1 with no
//! transform, so the logical units are the image's pixels.

use agg_gui::{render_background, render_progress_fill, Color, Framebuffer, GfxCtx, Rect};

const WIDTH: u32 = 80;
const HEIGHT: u32 = 24;
const RADIUS: f64 = HEIGHT as f64 / 2.0;

// wide enough that a radius which was not reduced along with the inset is
// unmistakable; the same defect at a one pixel outline is a sub-pixel smear
const OUTLINE_WIDTH: f64 = 4.0;

const WHITE: [u8; 4] = [255, 255, 255, 255];
const RED: [u8; 4] = [255, 0, 0, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];

fn blue() -> Color {
    Color::rgba(0.0, 0.0, 1.0, 1.0)
}
fn red() -> Color {
    Color::rgba(1.0, 0.0, 0.0, 1.0)
}
fn green() -> Color {
    Color::rgba(0.0, 1.0, 0.0, 1.0)
}

fn bounds() -> Rect {
    Rect::new(0.0, 0.0, WIDTH as f64, HEIGHT as f64)
}

/// C#'s `RenderAtDeviceScaleOne`: a white image the size of the widget.
fn render(draw: impl FnOnce(&mut GfxCtx)) -> Framebuffer {
    let mut fb = Framebuffer::new(WIDTH, HEIGHT);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::rgba(1.0, 1.0, 1.0, 1.0));
        draw(&mut ctx);
    }
    fb
}

/// RGBA of pixel (`x`, `y`), Y-up as C#'s `ImageBuffer`.
fn pixel(fb: &Framebuffer, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * fb.width() + x) * 4) as usize;
    let p = fb.pixels();
    [p[i], p[i + 1], p[i + 2], p[i + 3]]
}

/// C#'s `DrawProgressBar`: the background and outline every widget gets,
/// then the progress fill.
fn draw_progress_bar(ctx: &mut GfxCtx, ratio_complete: f64) {
    render_background(ctx, bounds(), blue(), RADIUS, OUTLINE_WIDTH, red());
    render_progress_fill(
        ctx,
        bounds(),
        ratio_complete,
        green(),
        RADIUS,
        OUTLINE_WIDTH,
        red(),
    );
}

/// How far a point sits outside the stadium (negative when inside).
fn distance_outside_stadium(x: f64, y: f64) -> f64 {
    let nearest_on_axis = x.clamp(RADIUS, WIDTH as f64 - RADIUS);
    let dx = x - nearest_on_axis;
    let dy = y - (HEIGHT as f64 / 2.0);
    (dx * dx + dy * dy).sqrt() - RADIUS
}

fn assert_nothing_painted_outside_the_stadium(fb: &Framebuffer) {
    for x in 0..WIDTH {
        for y in 0..HEIGHT {
            // a full pixel of slack keeps the antialiased edge out of this
            if distance_outside_stadium(x as f64 + 0.5, y as f64 + 0.5) > 1.0 {
                assert_eq!(
                    pixel(fb, x, y),
                    WHITE,
                    "pixel {x}, {y} lies outside the stadium and should never be painted"
                );
            }
        }
    }
}

#[test]
fn stadium_background_outline_stays_inside_the_stadium() {
    let image =
        render(|ctx| render_background(ctx, bounds(), blue(), RADIUS, OUTLINE_WIDTH, red()));
    assert_nothing_painted_outside_the_stadium(&image);
}

#[test]
fn an_outline_too_wide_for_the_widget_paints_nothing() {
    // 30 on a 24 tall widget: the background insets by the full stroke and the
    // stroke centerline by half of it, both past the 12 of half height
    const TOO_WIDE_OUTLINE: f64 = 30.0;
    let image =
        render(|ctx| render_background(ctx, bounds(), blue(), RADIUS, TOO_WIDE_OUTLINE, red()));
    for x in 0..WIDTH {
        for y in 0..HEIGHT {
            assert_eq!(
                pixel(&image, x, y),
                WHITE,
                "pixel {x}, {y} should be left alone by a background and an outline that do not fit"
            );
        }
    }
}

#[test]
fn stadium_progress_bar_outline_stays_inside_the_stadium() {
    // no progress, so this sees only the background and the outline
    let image = render(|ctx| draw_progress_bar(ctx, 0.0));
    assert_nothing_painted_outside_the_stadium(&image);
}

#[test]
fn empty_progress_bar_paints_nothing_beyond_the_standard_background() {
    let progress_bar = render(|ctx| draw_progress_bar(ctx, 0.0));
    let background_only =
        render(|ctx| render_background(ctx, bounds(), blue(), RADIUS, OUTLINE_WIDTH, red()));
    for x in 0..WIDTH {
        for y in 0..HEIGHT {
            assert_eq!(
                pixel(&progress_bar, x, y),
                pixel(&background_only, x, y),
                "pixel {x}, {y} of an empty bar should be the plain widget background"
            );
        }
    }
}

#[test]
fn narrow_progress_fill_stays_within_the_fill_width() {
    const RATIO_COMPLETE: f64 = 0.02;
    // the fill starts at the outline's inner edge and measures its progress
    // across only the room left there
    let fill_width = (WIDTH as f64 - OUTLINE_WIDTH * 2.0) * RATIO_COMPLETE;
    let fill_right = OUTLINE_WIDTH + fill_width;

    let empty = render(|ctx| draw_progress_bar(ctx, 0.0));
    let filled = render(|ctx| draw_progress_bar(ctx, RATIO_COMPLETE));

    let first_column_beyond_the_fill = fill_right.ceil() as u32;
    for x in first_column_beyond_the_fill..WIDTH {
        for y in 0..HEIGHT {
            assert_eq!(
                pixel(&filled, x, y),
                pixel(&empty, x, y),
                "pixel {x}, {y} is past the {fill_width} wide fill that ends at {fill_right} \
                 and should look the same as an empty bar"
            );
        }
    }
}

#[test]
fn full_progress_fill_leaves_the_outline_visible() {
    let image = render(|ctx| draw_progress_bar(ctx, 1.0));
    let middle_row = HEIGHT / 2;

    // one pixel in from each end cap: wholly inside the outline ring
    assert_eq!(
        pixel(&image, 1, middle_row),
        RED,
        "the left end cap of the outline must survive a full fill"
    );
    assert_eq!(
        pixel(&image, WIDTH - 2, middle_row),
        RED,
        "the right end cap of the outline must survive a full fill"
    );
    // and the fill really did paint
    assert_eq!(
        pixel(&image, WIDTH / 2, middle_row),
        GREEN,
        "a full bar should be filled between the end caps"
    );
}
