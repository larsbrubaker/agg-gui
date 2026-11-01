//! The breadcrumb trail (`folder_bread_crumb_for`, `FolderBreadCrumbWidget`): what it does when it
//! is wider than the widget - a Save As dialog is only 480 wide, so this is the normal case
//! there, not an edge case. Crumbs at the front of the trail are dropped and replaced by a "..."
//! that is only a label: it has no click handler, so an elided folder cannot be reached by
//! clicking it. That is why the root crumb has to survive elision, and why tests must never treat
//! a missing crumb button as a missing folder. Also what its buttons do when clicked and how the
//! up button draws: the look's image centred, or the Font Awesome glyph, dimmed at the root.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::icon_image::IconImage;
use agg_gui::text::Font;
use agg_gui::widget::paint_subtree;
use agg_gui::{Event, Framebuffer, GfxCtx, Modifiers, MouseButton, Point, Rect, Size, Widget};
use agg_gui_file_browser::{
    folder_bread_crumb_for, BrowserIcon, BrowserLook, BrowserNames, BrowserStyle, DefaultLook,
    FolderBreadCrumbWidget,
};

/// The width a Save As or browser dialog gives the breadcrumb row.
const DIALOG_WIDTH: f64 = 480.0;

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(agg_gui::fonts::NOTO_SANS_REGULAR).expect("bundled font loads"))
}

/// A trail far too long for any dialog, so the elision loop runs to its floor.
fn deep_trail() -> Vec<String> {
    let mut trail = vec!["Home".to_string()];
    for i in 0..12 {
        trail.push(format!("a rather long folder name {i}"));
    }
    trail
}

fn names_of(trail: &[&str]) -> Vec<String> {
    trail.iter().map(|n| n.to_string()).collect()
}

/// The trail of `trail` (root first) in `look`, laid out `width` wide, its clicks recorded.
fn trail_in(
    look: Rc<dyn BrowserLook>,
    width: f64,
    trail: &[&str],
) -> (FolderBreadCrumbWidget, Rc<RefCell<Vec<usize>>>) {
    agg_gui::set_device_scale(1.0);
    let clicks = Rc::new(RefCell::new(Vec::new()));
    let seen = Rc::clone(&clicks);
    let go_to: Rc<dyn Fn(usize)> = Rc::new(move |depth| seen.borrow_mut().push(depth));
    let mut widget = folder_bread_crumb_for(&names_of(trail), go_to, &look, &font());
    let size = widget.layout(Size::new(width, 32.0));
    widget.set_bounds(Rect::new(0.0, 0.0, size.width, size.height));
    (widget, clicks)
}

fn lay_out_trail(width: f64, trail: &[&str]) -> FolderBreadCrumbWidget {
    trail_in(Rc::new(DefaultLook::default()), width, trail).0
}

fn crumb_names(widget: &FolderBreadCrumbWidget) -> Vec<String> {
    const PREFIX: &str = "Bread Crumb Button ";
    widget
        .children()
        .iter()
        .filter_map(|child| child.id()?.strip_prefix(PREFIX).map(str::to_string))
        .collect()
}

fn has_ellipsis(widget: &FolderBreadCrumbWidget) -> bool {
    widget.children().iter().any(|child| {
        child.type_name() == "Label"
            && child
                .properties()
                .iter()
                .any(|(key, value)| *key == "text" && value == "...")
    })
}

/// Whether `id` is anywhere under `widget`.
fn has_descendant(widget: &dyn Widget, id: &str) -> bool {
    widget
        .children()
        .iter()
        .any(|child| child.id() == Some(id) || has_descendant(child.as_ref(), id))
}

fn child_named<'a>(widget: &'a mut FolderBreadCrumbWidget, id: &str) -> &'a mut Box<dyn Widget> {
    widget
        .children_mut()
        .iter_mut()
        .find(|child| child.id() == Some(id))
        .unwrap_or_else(|| panic!("no {id}"))
}

fn click(widget: &mut dyn Widget) {
    let pos = Point::new(2.0, 2.0);
    let modifiers = Modifiers::default();
    let button = MouseButton::Left;
    widget.on_event(&Event::MouseDown {
        pos,
        button,
        modifiers,
    });
    widget.on_event(&Event::MouseUp {
        pos,
        button,
        modifiers,
    });
}

fn property(widget: &dyn Widget, key: &str) -> String {
    widget
        .properties()
        .into_iter()
        .find(|(k, _)| *k == key)
        .map(|(_, v)| v)
        .unwrap_or_else(|| panic!("no {key}"))
}

#[test]
fn a_trail_that_fits_shows_every_crumb_and_no_ellipsis() {
    let widget = lay_out_trail(DIALOG_WIDTH, &["Home", "Computer"]);
    assert_eq!(crumb_names(&widget), ["Home", "Computer"]);
    assert!(!has_ellipsis(&widget));
    assert_eq!(widget.id(), Some("FolderBreadCrumbWidget"));
}

/// An elided crumb is deleted outright: the "..." that replaces it is a plain label.
#[test]
fn elided_crumbs_are_deleted_rather_than_tucked_into_the_ellipsis() {
    let trail = deep_trail();
    let trail_refs: Vec<&str> = trail.iter().map(String::as_str).collect();
    let widget = lay_out_trail(DIALOG_WIDTH, &trail_refs);
    let crumbs = crumb_names(&widget);
    let elided: Vec<&String> = trail.iter().filter(|n| !crumbs.contains(n)).collect();
    assert!(has_ellipsis(&widget), "a trail this long cannot fit");
    assert!(!elided.is_empty());
    assert!(
        !has_descendant(&widget, &format!("Bread Crumb Button {}", elided[0])),
        "the '...' is a label, not a menu - nothing holds on to the crumbs it stands for"
    );
    assert!(widget.bounds().width <= DIALOG_WIDTH);
}

/// The root crumb is the only one-click way back to the top (the up button climbs a single level
/// per click), so elision must never take it.
#[test]
fn a_deep_trail_keeps_the_root_and_the_current_folder_reachable() {
    let widget = lay_out_trail(
        DIALOG_WIDTH,
        &[
            "Home",
            "Computer",
            "Downloads",
            "MatterCAD Tests",
            "sa_123456789",
        ],
    );
    let crumbs = crumb_names(&widget);
    assert_eq!(crumbs.first().map(String::as_str), Some("Home"));
    assert_eq!(crumbs.last().map(String::as_str), Some("sa_123456789"));
}

#[test]
fn a_very_deep_trail_still_keeps_the_root_and_the_current_folder() {
    let trail = deep_trail();
    let trail_refs: Vec<&str> = trail.iter().map(String::as_str).collect();
    let widget = lay_out_trail(DIALOG_WIDTH, &trail_refs);
    let crumbs = crumb_names(&widget);
    assert!(has_ellipsis(&widget));
    assert_eq!(crumbs.first().map(String::as_str), Some("Home"));
    assert_eq!(crumbs.last(), trail.last());
}

#[test]
fn a_wider_widget_brings_the_elided_crumbs_back() {
    let trail = deep_trail();
    let trail_refs: Vec<&str> = trail.iter().map(String::as_str).collect();
    let mut widget = lay_out_trail(DIALOG_WIDTH, &trail_refs);
    assert!(has_ellipsis(&widget));
    widget.layout(Size::new(100_000.0, 32.0));
    assert!(!has_ellipsis(&widget));
    assert_eq!(crumbs_len(&widget), trail.len());
}

fn crumbs_len(widget: &FolderBreadCrumbWidget) -> usize {
    crumb_names(widget).len()
}

#[test]
fn the_up_button_climbs_one_level_and_a_crumb_opens_its_folder() {
    let look: Rc<dyn BrowserLook> = Rc::new(DefaultLook::default());
    let (mut widget, clicks) = trail_in(look, DIALOG_WIDTH, &["Home", "Computer", "Downloads"]);
    let up = child_named(&mut widget, "Library Up Button");
    assert_eq!(property(up.as_ref(), "enabled"), "true");
    assert_eq!(property(up.as_ref(), "icon"), "up");
    assert_eq!(up.tooltip_text(), Some("Click to go back"));
    click(up.as_mut());
    click(child_named(&mut widget, "Bread Crumb Button Home").as_mut());
    click(child_named(&mut widget, "Bread Crumb Button Downloads").as_mut());
    assert_eq!(*clicks.borrow(), [1, 0, 2]);
}

#[test]
fn at_the_root_the_up_button_is_disabled_and_ignores_clicks() {
    let look: Rc<dyn BrowserLook> = Rc::new(DefaultLook::default());
    let (mut widget, clicks) = trail_in(look, DIALOG_WIDTH, &["Home"]);
    let up = child_named(&mut widget, "Library Up Button");
    assert_eq!(property(up.as_ref(), "enabled"), "false");
    assert!(!up.is_enabled());
    click(up.as_mut());
    assert!(clicks.borrow().is_empty());
}

/// A look with its own up icon (a solid red square) and names, recording what it was asked for.
struct RedUpLook {
    names: BrowserNames,
    asked: RefCell<Vec<(BrowserIcon, Size, bool)>>,
}

impl BrowserLook for RedUpLook {
    fn style(&self) -> BrowserStyle {
        BrowserStyle::default()
    }
    fn icon(&self, which: BrowserIcon, logical: Size, enabled: bool) -> Option<IconImage> {
        self.asked.borrow_mut().push((which, logical, enabled));
        let side = logical.width as u32;
        let alpha = if enabled { 255 } else { 51 };
        let rgba = [255u8, 0, 0, alpha].repeat((side * side) as usize);
        IconImage::from_rgba(Arc::new(rgba), side, side, logical)
    }
    fn icon_name(&self, _which: BrowserIcon) -> String {
        "back.svg".into()
    }
    fn names(&self) -> &BrowserNames {
        &self.names
    }
}

/// The trail painted into a framebuffer its own size: RGBA, top row first.
fn paint(widget: &mut FolderBreadCrumbWidget) -> (Vec<u8>, usize) {
    let (w, h) = (widget.bounds().width as u32, widget.bounds().height as u32);
    let mut fb = Framebuffer::new(w, h);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        paint_subtree(widget, &mut ctx);
    }
    (fb.pixels_flipped(), w as usize)
}

fn pixel(pixels: &[u8], width: usize, x: usize, y: usize) -> [u8; 4] {
    let at = (y * width + x) * 4;
    [pixels[at], pixels[at + 1], pixels[at + 2], pixels[at + 3]]
}

#[test]
fn the_up_button_draws_the_looks_image_centred_on_whole_pixels() {
    let look = Rc::new(RedUpLook {
        names: BrowserNames::default(),
        asked: RefCell::new(Vec::new()),
    });
    let (mut widget, _) = trail_in(look.clone(), DIALOG_WIDTH, &["Home", "Computer"]);
    assert_eq!(
        property(
            child_named(&mut widget, "Library Up Button").as_ref(),
            "icon"
        ),
        "back.svg"
    );
    let (pixels, width) = paint(&mut widget);
    // A 32-unit button with a 20-unit image: the image covers 6..26 on both axes.
    assert_eq!(pixel(&pixels, width, 6, 6), [255, 0, 0, 255]);
    assert_eq!(pixel(&pixels, width, 25, 25), [255, 0, 0, 255]);
    assert_eq!(pixel(&pixels, width, 16, 16), [255, 0, 0, 255]);
    assert_eq!(
        pixel(&pixels, width, 5, 16)[3],
        0,
        "nothing left of the image"
    );
    assert_eq!(
        pixel(&pixels, width, 26, 16)[3],
        0,
        "nothing right of the image"
    );
    let asked = look.asked.borrow().clone();
    assert_eq!(asked, [(BrowserIcon::Up, Size::new(20.0, 20.0), true)]);
    // A second paint keeps the image it has.
    paint(&mut widget);
    assert_eq!(look.asked.borrow().len(), 1);
}

#[test]
fn at_the_root_the_look_is_asked_for_its_dimmed_image() {
    let look = Rc::new(RedUpLook {
        names: BrowserNames::default(),
        asked: RefCell::new(Vec::new()),
    });
    let (mut widget, _) = trail_in(look.clone(), DIALOG_WIDTH, &["Home"]);
    let (pixels, width) = paint(&mut widget);
    assert_eq!(pixel(&pixels, width, 16, 16)[3], 51);
    assert!(!look.asked.borrow()[0].2, "asked for the disabled image");
}

/// The most opaque pixel inside the up button's square.
fn up_button_ink(look: Rc<dyn BrowserLook>, trail: &[&str]) -> u8 {
    let (mut widget, _) = trail_in(look, DIALOG_WIDTH, trail);
    let (pixels, width) = paint(&mut widget);
    let mut most = 0;
    for y in 0..32 {
        for x in 0..32 {
            most = most.max(pixel(&pixels, width, x, y)[3]);
        }
    }
    most
}

#[test]
fn without_an_image_the_up_button_draws_its_glyph_dimmed_at_the_root() {
    let enabled = up_button_ink(Rc::new(DefaultLook::default()), &["Home", "Computer"]);
    let disabled = up_button_ink(Rc::new(DefaultLook::default()), &["Home"]);
    assert!(
        enabled > 200,
        "the glyph draws in the text colour ({enabled})"
    );
    assert!(
        disabled > 0 && disabled < enabled / 3,
        "a disabled glyph keeps a fifth of its alpha ({disabled} of {enabled})"
    );
}

#[test]
fn the_crumbs_are_named_by_the_look() {
    let names = BrowserNames {
        bread_crumb: "Trail".into(),
        up_button: "Up".into(),
        bread_crumb_button: "Go to {name}".into(),
        ..BrowserNames::default()
    };
    let look: Rc<dyn BrowserLook> = Rc::new(DefaultLook::with_names(names));
    let (widget, _) = trail_in(look, DIALOG_WIDTH, &["Home", "Computer"]);
    assert_eq!(widget.id(), Some("Trail"));
    assert!(has_descendant(&widget, "Up"));
    assert!(has_descendant(&widget, "Go to Computer"));
}

/// A look whose up icon is a 19-unit red square whatever size is asked for, so centring it in
/// the 32-unit button lands half a unit off the pixel grid.
struct OddUpLook {
    names: BrowserNames,
}

impl BrowserLook for OddUpLook {
    fn style(&self) -> BrowserStyle {
        BrowserStyle::default()
    }
    fn icon(&self, _which: BrowserIcon, _logical: Size, _enabled: bool) -> Option<IconImage> {
        let rgba = [255u8, 0, 0, 255].repeat(19 * 19);
        IconImage::from_rgba(Arc::new(rgba), 19, 19, Size::new(19.0, 19.0))
    }
    fn names(&self) -> &BrowserNames {
        &self.names
    }
}

#[test]
fn an_image_centred_half_a_pixel_off_moves_onto_whole_pixels() {
    let look: Rc<dyn BrowserLook> = Rc::new(OddUpLook {
        names: BrowserNames::default(),
    });
    let (mut widget, _) = trail_in(look, DIALOG_WIDTH, &["Home", "Computer"]);
    let (pixels, width) = paint(&mut widget);
    // Centred at 6.5 and moved to 7: columns 7..26 are solid, with no half-covered edge.
    for x in 7..26 {
        assert_eq!(pixel(&pixels, width, x, 16), [255, 0, 0, 255], "column {x}");
    }
    assert_eq!(
        pixel(&pixels, width, 6, 16)[3],
        0,
        "nothing left of the image"
    );
    assert_eq!(
        pixel(&pixels, width, 26, 16)[3],
        0,
        "nothing right of the image"
    );
}
