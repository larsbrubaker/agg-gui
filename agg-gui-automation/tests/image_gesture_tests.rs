//! Rust-only tests of the runner's image gestures (`ClickImage`,
//! `DragImage`, `DropImage`, `DragDropImage` in agg-sharp
//! `GuiAutomation/AutomationRunner.cs`) and their needles by file name. C#
//! has no tests for these; its gestures land above the window (they flip
//! with the region's Y-down bottom) and ignore their wait, which the tests
//! of the match position, the search region and the wait pin as fixed.

use agg_gui::{clock, Color, Event, Framebuffer, GfxCtx, Rect};
use agg_gui_automation::{
    decode_needle, show_window_and_execute_tests, AutomationWindow, ClickOrigin,
    DirectoryImageSource, ImageDragDropOpts, ImageNeedle, ImageOpts, ImageSource, Point2D,
    ProbeLog, ProbeWidget, RunOptions, ScreenRectangle, SearchRegion,
};

fn red() -> Color {
    Color::rgb(1.0, 0.0, 0.0)
}

fn green() -> Color {
    Color::rgb(0.0, 1.0, 0.0)
}

fn cleared_image(width: u32, height: u32, color: Color) -> Framebuffer {
    let mut image = Framebuffer::new(width, height);
    GfxCtx::new(&mut image).clear(color);
    image
}

/// A white 100 × 100 window with a 10 × 10 red patch whose lower-left corner
/// is at (30, 20) (logical, Y-up) and a green one at (60, 60); both patches
/// take presses and log what they are sent.
fn patches_window() -> (AutomationWindow, (ProbeLog, ProbeLog)) {
    let red_patch = ProbeWidget::new("red")
        .with_bounds(Rect::new(30.0, 20.0, 10.0, 10.0))
        .with_background(red())
        .capturing_presses();
    let green_patch = ProbeWidget::new("green")
        .with_bounds(Rect::new(60.0, 60.0, 10.0, 10.0))
        .with_background(green())
        .capturing_presses();
    let logs = (red_patch.log(), green_patch.log());
    let root = ProbeWidget::new("root")
        .with_background(Color::white())
        .with_child(Box::new(red_patch))
        .with_child(Box::new(green_patch));
    (
        AutomationWindow::with_root(Box::new(root), 100.0, 100.0),
        logs,
    )
}

fn downs(log: &ProbeLog) -> usize {
    log.borrow()
        .iter()
        .filter(|e| matches!(e, Event::MouseDown { .. }))
        .count()
}

fn ups(log: &ProbeLog) -> usize {
    log.borrow()
        .iter()
        .filter(|e| matches!(e, Event::MouseUp { .. }))
        .count()
}

#[test]
fn rust_only_click_image_clicks_the_middle_of_the_needle_where_it_is_found() {
    show_window_and_execute_tests(RunOptions::default(), patches_window, |runner, logs| {
        let needle = cleared_image(10, 10, red());
        assert!(runner.click_image(ImageNeedle::Buffer(&needle), &ImageOpts::default()));
        // The patch's center (35, 25) Y-up is pointer (35, 75) Y-down.
        assert_eq!(runner.current_mouse_position(), Point2D::new(35, 75));
        assert_eq!((downs(&logs.0), ups(&logs.0)), (1, 1));
        assert_eq!(downs(&logs.1), 0);
        runner.mark_test_complete();
    })
    .expect("click_image clicks the needle");
}

#[test]
fn rust_only_an_image_offset_is_measured_up_from_the_needles_lower_left_corner() {
    show_window_and_execute_tests(RunOptions::default(), patches_window, |runner, logs| {
        let needle = cleared_image(10, 10, red());
        let opts = ImageOpts {
            offset: Point2D::new(2, 3),
            origin: ClickOrigin::LowerLeft,
            ..ImageOpts::default()
        };
        assert!(runner.click_image(ImageNeedle::Buffer(&needle), &opts));
        assert_eq!(runner.current_mouse_position(), Point2D::new(32, 77));
        assert_eq!(downs(&logs.0), 1);
        runner.mark_test_complete();
    })
    .expect("the offset is from the lower-left corner");
}

#[test]
fn rust_only_an_image_that_is_not_there_is_not_clicked_and_is_waited_for() {
    show_window_and_execute_tests(RunOptions::default(), patches_window, |runner, logs| {
        let blue = cleared_image(10, 10, Color::rgb(0.0, 0.0, 1.0));
        let before = runner.current_mouse_position();
        let started = clock::now();
        let opts = ImageOpts {
            secs_to_wait: 0.5,
            ..ImageOpts::default()
        };
        assert!(!runner.click_image(ImageNeedle::Buffer(&blue), &opts));
        assert!(
            clock::since(started).as_secs_f64() >= 0.5,
            "the gesture waits its seconds for the needle (C# ignored them)"
        );
        assert_eq!(runner.current_mouse_position(), before);
        assert_eq!(downs(&logs.0) + downs(&logs.1), 0);
        runner.mark_test_complete();
    })
    .expect("a missing needle is not clicked");
}

#[test]
fn rust_only_image_gestures_place_the_match_inside_their_search_region() {
    show_window_and_execute_tests(RunOptions::default(), patches_window, |runner, logs| {
        // The region holds only the red patch: Y-down pixels 20..60 across,
        // 60..90 down. Its image is captured (cropped) on first use.
        let region = SearchRegion::new(ScreenRectangle::new(20, 60, 60, 90));
        let red_needle = cleared_image(10, 10, red());
        let green_needle = cleared_image(10, 10, green());
        let opts = ImageOpts {
            search_region: Some(&region),
            secs_to_wait: 0.0,
            ..ImageOpts::default()
        };
        assert!(
            !runner.click_image(ImageNeedle::Buffer(&green_needle), &opts),
            "the green patch is outside the region"
        );
        assert!(runner.click_image(ImageNeedle::Buffer(&red_needle), &opts));
        assert_eq!(runner.current_mouse_position(), Point2D::new(35, 75));
        assert_eq!(downs(&logs.0), 1);
        runner.mark_test_complete();
    })
    .expect("a region limits the search and places the match");
}

#[test]
fn rust_only_drag_drop_image_presses_on_one_needle_and_releases_on_the_other() {
    show_window_and_execute_tests(RunOptions::default(), patches_window, |runner, logs| {
        let red_needle = cleared_image(10, 10, red());
        let green_needle = cleared_image(10, 10, green());
        assert!(runner.drag_drop_image(
            ImageNeedle::Buffer(&red_needle),
            ImageNeedle::Buffer(&green_needle),
            &ImageDragDropOpts::default(),
        ));
        // Green's center (65, 65) Y-up is pointer (65, 35).
        assert_eq!(runner.current_mouse_position(), Point2D::new(65, 35));
        // The press captured the pointer on red, so red gets the release.
        assert_eq!((downs(&logs.0), ups(&logs.0)), (1, 1));
        assert_eq!(downs(&logs.1), 0);
        runner.mark_test_complete();
    })
    .expect("drag_drop_image drags between the needles");
}

#[test]
fn rust_only_drag_image_and_drop_image_split_the_press_from_the_release() {
    show_window_and_execute_tests(RunOptions::default(), patches_window, |runner, logs| {
        let red_needle = cleared_image(10, 10, red());
        let green_needle = cleared_image(10, 10, green());
        assert!(runner.drag_image(ImageNeedle::Buffer(&red_needle), &ImageOpts::default()));
        assert_eq!(
            (downs(&logs.0), ups(&logs.0)),
            (1, 0),
            "pressed, not released"
        );
        assert!(runner.drop_image(ImageNeedle::Buffer(&green_needle), &ImageOpts::default()));
        assert_eq!(ups(&logs.0), 1);
        runner.mark_test_complete();
    })
    .expect("drag_image presses and drop_image releases");
}

#[test]
fn rust_only_a_needle_named_by_file_loads_through_the_image_source() {
    show_window_and_execute_tests(RunOptions::default(), patches_window, |runner, logs| {
        runner.set_image_source(|name: &str| {
            (name == "red.png").then(|| cleared_image(10, 10, red()))
        });
        let opts = ImageOpts {
            secs_to_wait: 0.0,
            ..ImageOpts::default()
        };
        assert!(
            !runner.click_image(ImageNeedle::Name("missing.png"), &opts),
            "a name that does not load finds nothing"
        );
        assert!(runner.click_image(ImageNeedle::Name("red.png"), &opts));
        assert_eq!(downs(&logs.0), 1);
        runner.mark_test_complete();
    })
    .expect("named needles load through the source");
}

#[test]
fn rust_only_the_images_directory_is_the_default_image_source() {
    let directory =
        std::env::temp_dir().join(format!("agg-gui-automation-needles-{}", std::process::id()));
    std::fs::create_dir_all(&directory).expect("temp dir");
    image::RgbaImage::from_pixel(10, 10, image::Rgba([255, 0, 0, 255]))
        .save(directory.join("red.png"))
        .expect("write the needle");

    let opts = RunOptions {
        images_directory: directory.clone(),
        ..RunOptions::default()
    };
    let result = show_window_and_execute_tests(opts, patches_window, |runner, logs| {
        assert!(runner.click_image(ImageNeedle::Name("red.png"), &ImageOpts::default()));
        assert_eq!(downs(&logs.0), 1);
        runner.mark_test_complete();
    });
    let _ = std::fs::remove_dir_all(&directory);
    result.expect("needles load from the images directory");
}

#[test]
fn rust_only_decoded_needles_store_their_bottom_row_first() {
    // A 1 × 2 image: red on top, blue below. Captures keep row 0 at the
    // bottom, so the decoded needle starts with the blue pixel.
    let mut top_down = image::RgbaImage::new(1, 2);
    top_down.put_pixel(0, 0, image::Rgba([255, 0, 0, 255]));
    top_down.put_pixel(0, 1, image::Rgba([0, 0, 255, 255]));
    let mut png = Vec::new();
    top_down
        .write_to(
            &mut std::io::Cursor::new(&mut png),
            image::ImageOutputFormat::Png,
        )
        .expect("encode");

    let needle = decode_needle(&png).expect("decodes");
    assert_eq!((needle.width(), needle.height()), (1, 2));
    assert_eq!(&needle.pixels()[..4], &[0, 0, 255, 255]);
    assert_eq!(&needle.pixels()[4..], &[255, 0, 0, 255]);
    assert!(DirectoryImageSource::new("no-such-dir")
        .load("x.png")
        .is_none());
}
