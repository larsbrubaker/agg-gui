//! Port of agg-sharp `AggSharpDemo/WidgetGalleryPlaceholderGpuTests.cs`: an
//! empty text field draws its dimmed hint on the GPU, with LCD text on and
//! off, at 1x and 2x device scale, and after the theme has changed from the
//! one the tree was built in (the browser builds dark, then picks up the
//! system's light preference).
//!
//! C# builds the whole AggSharpDemo shell and finds the Widget Gallery's
//! `TextField`; the demo shell is not ported (it is AggSharpDemo-only), so the
//! port keeps what the test is about: an empty `TextField` with a hint inside
//! a rounded `Window` (composited through its own GPU layer, as the gallery
//! sits in one) over a page, the whole page drawn through `GpuFrames`.
//! MatterCAD's search fields show hints this way. C#'s `DeviceScale` is a
//! `scale(ds, ds)` before the tree paints, on a frame `ds` times the size.

use agg_gui::draw_ctx::DrawCtx;
use agg_gui::font_settings::{clear_lcd_enabled_override, set_lcd_enabled};
use agg_gui::theme::{current_visuals, set_visuals, Visuals};
use agg_gui::widget::Widget;
use agg_gui::widgets::{TextField, Window};
use agg_gui::{Event, EventResult, Rect, Size};

use super::harness::{at, GpuFrames};

const PAGE_W: f64 = 400.0;
const PAGE_H: f64 = 250.0;
/// The window's bounds on the page and the field's inside its content area.
const WINDOW: (f64, f64, f64, f64) = (40.0, 30.0, 300.0, 180.0);
const FIELD: (f64, f64, f64, f64) = (20.0, 60.0, 220.0, 26.0);

/// A container that paints nothing and holds its children where they were
/// put: the window's content, and the page.
struct Holder {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl Widget for Holder {
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
        for child in &mut self.children {
            let b = child.bounds();
            child.layout(Size::new(b.width, b.height));
            child.set_bounds(b);
        }
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

fn font() -> std::sync::Arc<agg_gui::text::Font> {
    let bytes: &[u8] = include_bytes!("../../assets/fonts/NotoSans-Regular.ttf");
    std::sync::Arc::new(agg_gui::text::Font::from_slice(bytes).expect("parse the test font"))
}

/// Puts the thread's visuals and LCD setting back however the test ends.
struct Restore(Visuals);

impl Drop for Restore {
    fn drop(&mut self) {
        set_visuals(self.0.clone());
        clear_lcd_enabled_override();
    }
}

fn empty_text_edit_draws_its_placeholder_on_the_gpu(light: bool, lcd: bool, device_scale: f64) {
    let _restore = Restore(current_visuals());
    let (preference, opposite) = if light {
        (Visuals::light(), Visuals::dark())
    } else {
        (Visuals::dark(), Visuals::light())
    };
    let what = format!("light {light}, lcd {lcd}, scale {device_scale}");

    // Built in the opposite theme, then switched before it paints.
    set_visuals(opposite);
    let mut field = TextField::new(font()).with_placeholder("Write something here");
    let (fx, fy, fw, fh) = FIELD;
    field.set_bounds(Rect::new(fx, fy, fw, fh));
    let content = Holder {
        bounds: Rect::default(),
        children: vec![Box::new(field)],
    };
    let (wx, wy, ww, wh) = WINDOW;
    let mut window = Window::new("Widget Gallery", font(), Box::new(content))
        .with_bounds(Rect::new(wx, wy, ww, wh));
    window.layout(Size::new(PAGE_W, PAGE_H));
    let mut page = Holder {
        bounds: Rect::new(0.0, 0.0, PAGE_W, PAGE_H),
        children: vec![Box::new(window)],
    };
    set_visuals(preference.clone());
    set_lcd_enabled(lcd);

    let width = (PAGE_W * device_scale).ceil() as u32;
    let height = (PAGE_H * device_scale).ceil() as u32;
    let Some(mut frames) = GpuFrames::new(width, height) else {
        return;
    };
    frames.set_lcd_mode(lcd);
    let fill = preference.widget_bg;
    let image = frames.paint(fill, &mut page, &|ctx| {
        ctx.scale(device_scale, device_scale)
    });

    // The field on the page: the window's origin plus the field's place in
    // the content area (which starts at the window's bottom-left). Inset past
    // the field's rounded frame so only the hint can ink.
    let inset = 4.0;
    let x0 = ((wx + fx + inset) * device_scale) as u32;
    let x1 = ((wx + fx + fw - inset) * device_scale) as u32;
    let y0 = ((wy + fy + inset) * device_scale) as u32;
    let y1 = ((wy + fy + fh - inset) * device_scale) as u32;
    let to8 = |c: f32| (c * 255.0).round() as i32;
    let (fr, fg, fb) = (to8(fill.r), to8(fill.g), to8(fill.b));
    let mut inked = 0;
    for y in y0..y1 {
        for x in x0..x1 {
            let p = at(&image, width, x, height - 1 - y);
            let d = (p[0] as i32 - fr).abs() + (p[1] as i32 - fg).abs() + (p[2] as i32 - fb).abs();
            if d > 90 {
                inked += 1;
            }
        }
    }
    assert!(
        inked > 20,
        "{what}: the hint in the field at ({x0}..{x1}, {y0}..{y1}) should draw ink ({inked} pixels)"
    );
}

#[test]
fn empty_text_edit_draws_its_placeholder_on_the_gpu_light_1x() {
    empty_text_edit_draws_its_placeholder_on_the_gpu(true, false, 1.0);
}

#[test]
fn empty_text_edit_draws_its_placeholder_on_the_gpu_dark_1x() {
    empty_text_edit_draws_its_placeholder_on_the_gpu(false, false, 1.0);
}

#[test]
fn empty_text_edit_draws_its_placeholder_on_the_gpu_light_lcd_1x() {
    empty_text_edit_draws_its_placeholder_on_the_gpu(true, true, 1.0);
}

#[test]
fn empty_text_edit_draws_its_placeholder_on_the_gpu_dark_lcd_1x() {
    empty_text_edit_draws_its_placeholder_on_the_gpu(false, true, 1.0);
}

#[test]
fn empty_text_edit_draws_its_placeholder_on_the_gpu_light_2x() {
    empty_text_edit_draws_its_placeholder_on_the_gpu(true, false, 2.0);
}

#[test]
fn empty_text_edit_draws_its_placeholder_on_the_gpu_dark_2x() {
    empty_text_edit_draws_its_placeholder_on_the_gpu(false, false, 2.0);
}

#[test]
fn empty_text_edit_draws_its_placeholder_on_the_gpu_light_lcd_2x() {
    empty_text_edit_draws_its_placeholder_on_the_gpu(true, true, 2.0);
}

#[test]
fn empty_text_edit_draws_its_placeholder_on_the_gpu_dark_lcd_2x() {
    empty_text_edit_draws_its_placeholder_on_the_gpu(false, true, 2.0);
}
