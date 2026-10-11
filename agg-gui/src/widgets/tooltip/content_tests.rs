//! Tests for the host tooltip content factory ([`super`]): a factory-built tip
//! sizes and paints from its widget, rebuilds when the tip text changes while
//! it shows, builds fresh content per show, and leaves the default text-line
//! path untouched when no factory is installed.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use super::*;
use crate::color::Color;
use crate::event::{Event, EventResult};
use crate::geometry::Point;
use crate::text::{measure_advance, Font};
use crate::widgets::tooltip::controller::{self, drive, is_visible, visible_rect};
use crate::widgets::tooltip::render::{
    begin_tooltip_frame, last_submitted_font, paint_global_tooltips, panel_size, place_panel,
};
use crate::widgets::tooltip::timings::{reset_tooltip_test_state, tooltip_timings};
use crate::widgets::tooltip::{text_to_lines, TOOLTIP_FONT_SIZE};

const FONT_BYTES: &[u8] = include_bytes!("../../../assets/fonts/NotoSans-Regular.ttf");
const VIEWPORT: Size = Size {
    width: 400.0,
    height: 300.0,
};
const ANCHOR: Point = Point { x: 100.0, y: 200.0 };

/// Resets the tooltip subsystem and pins the clock; undoes it on drop.
struct Guard;
impl Drop for Guard {
    fn drop(&mut self) {
        set_content_factory(None);
        reset_tooltip_test_state();
        controller::reset();
        crate::font_settings::set_system_font(None);
    }
}
fn pin() -> Guard {
    reset_tooltip_test_state();
    controller::reset();
    set_content_factory(None);
    crate::clock::start_virtual();
    Guard
}

/// A tip body that is 10 px wide per character and 40 px tall whatever it is
/// offered, filling itself red, and counting its paints.
struct Swatch {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    width: f64,
    paints: Rc<Cell<usize>>,
}
impl Widget for Swatch {
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
    fn layout(&mut self, _available: Size) -> Size {
        Size::new(self.width, 40.0)
    }
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        self.paints.set(self.paints.get() + 1);
        ctx.set_fill_color(Color::rgba(1.0, 0.0, 0.0, 1.0));
        ctx.begin_path();
        ctx.rect(0.0, 0.0, self.bounds.width, self.bounds.height);
        ctx.fill();
    }
    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// Install a [`Swatch`] factory; returns the texts it was asked to build
/// from and the shared paint counter.
fn install_swatch_factory() -> (Rc<RefCell<Vec<String>>>, Rc<Cell<usize>>) {
    let builds = Rc::new(RefCell::new(Vec::new()));
    let paints = Rc::new(Cell::new(0));
    let (b, p) = (Rc::clone(&builds), Rc::clone(&paints));
    set_content_factory(Some(Box::new(move |text: &str| {
        b.borrow_mut().push(text.to_string());
        Box::new(Swatch {
            bounds: Rect::default(),
            children: Vec::new(),
            width: 10.0 * text.chars().count() as f64,
            paints: Rc::clone(&p),
        }) as Box<dyn Widget>
    })));
    (builds, paints)
}

/// One App-like frame: publish the viewport, then drive the controller.
fn frame(text: Option<&str>) {
    begin_tooltip_frame(VIEWPORT);
    drive(text.map(|t| (vec![0], t.to_string())), Some(ANCHOR));
}

/// Hover `text` long enough for the tip to show.
fn show(text: &str) {
    frame(Some(text));
    crate::clock::advance(tooltip_timings().initial_delay);
    frame(Some(text));
    assert!(is_visible(), "tip shows after the initial delay");
}

fn visible_area() -> Rect {
    Rect::new(0.0, 0.0, VIEWPORT.width, VIEWPORT.height)
}

fn rgba_at(fb: &crate::framebuffer::Framebuffer, x: f64, y: f64) -> [u8; 4] {
    // Bottom-up rows, so a Y-up root coordinate indexes directly.
    let i = ((y as usize) * fb.width() as usize + x as usize) * 4;
    let p = fb.pixels();
    [p[i], p[i + 1], p[i + 2], p[i + 3]]
}

#[test]
fn factory_tip_sizes_and_paints_from_its_widget() {
    let _g = pin();
    let (builds, paints) = install_swatch_factory();
    let text = "Fillet\n\n*At least 1*";
    show(text);

    let size = Size::new(10.0 * text.chars().count() as f64, 40.0);
    let rect = visible_rect().expect("factory tip has a placed rect");
    assert_eq!(rect, place_panel(ANCHOR, size, visible_area(), true));
    assert_eq!(
        *builds.borrow(),
        vec![text.to_string()],
        "built once, from the full text"
    );
    assert!(
        last_submitted_font().is_none(),
        "no default text-line request is queued alongside the content"
    );

    let mut fb =
        crate::framebuffer::Framebuffer::new(VIEWPORT.width as u32, VIEWPORT.height as u32);
    {
        let mut ctx = crate::gfx_ctx::GfxCtx::new(&mut fb);
        paint_global_tooltips(&mut ctx, VIEWPORT);
    }
    assert_eq!(paints.get(), 1, "the drain paints the content once");
    let centre = rgba_at(&fb, rect.x + rect.width * 0.5, rect.y + rect.height * 0.5);
    assert_eq!(
        centre,
        [255, 0, 0, 255],
        "content painted at the panel rect"
    );
    let outside = rgba_at(&fb, rect.x + rect.width + 20.0, rect.y + rect.height * 0.5);
    assert_eq!(outside, [0, 0, 0, 0], "nothing painted beside the panel");

    // A later frame with the same text keeps the built widget.
    frame(Some(text));
    assert_eq!(builds.borrow().len(), 1, "same text: no rebuild");
}

#[test]
fn factory_tip_rebuilds_when_text_changes_while_visible() {
    let _g = pin();
    let (builds, _paints) = install_swatch_factory();
    show("Fillet");
    assert_eq!(visible_rect().map(|r| r.width), Some(60.0));

    // Same widget, new tip text (selection changed under a still pointer).
    frame(Some("Fillet - select a part"));
    assert!(is_visible(), "a text change keeps the tip up");
    assert_eq!(
        *builds.borrow(),
        vec!["Fillet".to_string(), "Fillet - select a part".to_string()]
    );
    assert_eq!(active_text().as_deref(), Some("Fillet - select a part"));
    assert_eq!(
        visible_rect().map(|r| r.width),
        Some(220.0),
        "the panel resizes to the rebuilt widget"
    );
}

#[test]
fn factory_tip_builds_fresh_content_per_show() {
    let _g = pin();
    let (builds, _paints) = install_swatch_factory();
    show("Bevel");
    frame(None);
    assert!(!is_visible());
    assert_eq!(active_text(), None, "hiding drops the built content");
    show("Bevel");
    assert_eq!(builds.borrow().len(), 2, "a new show builds new content");
}

#[test]
fn default_tip_path_unchanged_without_factory() {
    let _g = pin();
    let font = Arc::new(Font::from_bytes(FONT_BYTES.to_vec()).expect("bundled font"));
    crate::font_settings::set_system_font(Some(Arc::clone(&font)));
    // Install and remove a factory: removal restores the text-line tip.
    let (builds, _paints) = install_swatch_factory();
    set_content_factory(None);
    assert!(!has_content_factory());

    let text = "Fillet\n\n*At least 1*";
    show(text);
    assert!(
        builds.borrow().is_empty(),
        "a removed factory is never called"
    );
    assert_eq!(active_text(), None);
    assert!(
        last_submitted_font().is_some_and(|f| Arc::ptr_eq(&f, &font)),
        "the default path queues its text-line request"
    );
    let lines = text_to_lines(text);
    let text_w = lines
        .iter()
        .map(|l| measure_advance(&font, &l.text, TOOLTIP_FONT_SIZE))
        .fold(0.0_f64, f64::max);
    let expected = place_panel(
        ANCHOR,
        panel_size(text_w, lines.len()),
        visible_area(),
        true,
    );
    assert_eq!(visible_rect(), Some(expected));
}

#[test]
fn active_content_is_readable_while_the_tip_shows() {
    let _g = pin();
    let (_builds, _paints) = install_swatch_factory();
    assert_eq!(with_active_content(|w| w.bounds()), None);
    show("Fillet");
    let rect = visible_rect().expect("factory tip has a placed rect");
    assert_eq!(
        with_active_content(|w| (w.bounds().width, w.bounds().height)),
        Some((rect.width, rect.height)),
        "the content is laid out at the panel size"
    );
    frame(None);
    assert_eq!(with_active_content(|w| w.bounds()), None, "hidden: nothing");
}
