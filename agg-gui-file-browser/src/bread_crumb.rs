//! The browser's breadcrumb trail: the up button, then one link per folder from the root down
//! with "/" between them ([`folder_bread_crumb_for`] builds it from the folder names and the
//! look). When the trail is wider than the widget (the normal case in a 480-wide dialog) the
//! crumbs just after the root are dropped, each with its separator, and a plain "..." label stands
//! in for them: it has no click handler, so an elided folder is not reachable from the trail,
//! which is why the root crumb always survives (the up button climbs a single level per click).
//! The floor is up button / root / "..." / current folder. Which crumbs fit depends on the width,
//! so [`FolderBreadCrumbWidget`] keeps every crumb and re-elides at each layout whose width
//! differs from the last.
//!
//! A trail built from equal inputs ([`CrumbKey`]: the names, the look's style and widget names,
//! the up button's tooltip) looks and acts the same, since each crumb's click goes to its depth
//! through the caller's handler; so the composed browser keeps the trail it has rather than build
//! an equal one (`browser/mod.rs`), and a crumb under the pointer stays the widget the user
//! pressed.

use std::rc::Rc;
use std::sync::Arc;

use agg_gui::text::Font;
use agg_gui::{DrawCtx, Event, EventResult, Label, Rect, Size, Widget};
use agg_gui_files::tr;

use crate::icon_button::BrowserIconButton;
use crate::link_label::LinkLabel;
use crate::look::{BrowserIcon, BrowserLook, BrowserNames, BrowserStyle};

/// The space between neighbouring pieces of the trail.
const GAP: f64 = 4.0;
/// The "..." label's right margin.
const ELLIPSIS_RIGHT_MARGIN: f64 = 5.0;
/// Slack: the trail elides once it is wider than `Width - 20`, so it never quite touches the edge.
const SLACK: f64 = 20.0;
/// The logical size the up button's image is asked for at.
pub const UP_ICON_SIZE: Size = Size {
    width: 20.0,
    height: 20.0,
};

/// The breadcrumb trail: its pieces (up button, crumbs and separators, in order),
/// of which the ones that fit are its children.
pub struct FolderBreadCrumbWidget {
    id: String,
    bounds: Rect,
    /// Every piece of the full trail, by its index in it; `None` while it is a child.
    pool: Vec<Option<Box<dyn Widget>>>,
    /// The "..." label, `None` while it is a child.
    ellipsis: Option<Box<dyn Widget>>,
    /// The pieces shown, in order.
    children: Vec<Box<dyn Widget>>,
    /// For each child, its index in `pool` (`None` for the "...").
    origins: Vec<Option<usize>>,
    /// The width the children were chosen for.
    last_layout_width: f64,
}

impl FolderBreadCrumbWidget {
    /// The trail `pieces` (the up button first, then crumb, "/", crumb, ...), named `id`, eliding
    /// with a "..." in `font` at `font_size` in `text_color`.
    pub fn new(
        id: &str,
        pieces: Vec<Box<dyn Widget>>,
        font: &Arc<Font>,
        font_size: f64,
        text_color: agg_gui::Color,
    ) -> Self {
        let ellipsis = Label::new("...", Arc::clone(font))
            .with_font_size(font_size)
            .with_color(text_color);
        let origins = (0..pieces.len()).map(Some).collect();
        Self {
            id: id.to_string(),
            bounds: Rect::default(),
            pool: pieces.iter().map(|_| None).collect(),
            ellipsis: Some(Box::new(ellipsis)),
            children: pieces,
            origins,
            last_layout_width: -1.0,
        }
    }

    /// Puts every child back in the pool, then shows the pieces at `shown` (pool indices, `None`
    /// for the "...") in that order.
    fn show(&mut self, shown: &[Option<usize>]) {
        let children = std::mem::take(&mut self.children);
        for (child, origin) in children.into_iter().zip(std::mem::take(&mut self.origins)) {
            match origin {
                Some(i) => self.pool[i] = Some(child),
                None => self.ellipsis = Some(child),
            }
        }
        for &origin in shown {
            let child = match origin {
                Some(i) => self.pool[i].take(),
                None => self.ellipsis.take(),
            };
            if let Some(child) = child {
                self.children.push(child);
                self.origins.push(origin);
            }
        }
    }

    /// The room `origin`'s piece takes in the row, its gap (and the "..."'s margin) included.
    fn piece_width(&mut self, origin: Option<usize>, height: f64) -> f64 {
        let piece = match origin {
            Some(i) => self.pool[i].as_mut(),
            None => self.ellipsis.as_mut(),
        };
        let Some(piece) = piece else {
            return 0.0;
        };
        let size = piece.layout(Size::new(f64::INFINITY, height));
        size.width
            + GAP
            + if origin.is_none() {
                ELLIPSIS_RIGHT_MARGIN
            } else {
                0.0
            }
    }

    /// Chooses which crumbs show in a widget `width` wide.
    fn elide(&mut self, width: f64, height: f64) {
        let count = self.pool.len();
        self.show(&[]);
        let widths: Vec<f64> = (0..count)
            .map(|i| self.piece_width(Some(i), height))
            .collect();
        let children_width: f64 = widths.iter().sum::<f64>() - GAP;
        let mut shown: Vec<Option<usize>> = (0..count).map(Some).collect();
        if width > 0.0 && shown.len() > 4 && children_width > width - SLACK {
            let width_of = |origin: Option<usize>| origin.map_or(0.0, |i| widths[i]);
            // Take out the crumb just after the root, and its separator, and put in a "...". The
            // root crumb itself always survives.
            let mut removed_width = width_of(shown.remove(3));
            removed_width += width_of(shown.remove(2));
            shown.insert(2, None);
            removed_width -= self.piece_width(None, height);
            // Keep eating the crumbs just past the "..." until what is left fits.
            while children_width - removed_width > width - SLACK && shown.len() > 5 {
                removed_width += width_of(shown.remove(3));
                removed_width += width_of(shown.remove(3));
            }
        }
        self.show(&shown);
    }
}

impl Widget for FolderBreadCrumbWidget {
    fn type_name(&self) -> &'static str {
        "FolderBreadCrumbWidget"
    }
    fn id(&self) -> Option<&str> {
        Some(&self.id)
    }
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
        // A new width lays the trail out again.
        if (available.width - self.last_layout_width).abs() > f64::EPSILON {
            self.last_layout_width = available.width;
            self.elide(available.width, available.height);
        }
        let sizes: Vec<Size> = self
            .children
            .iter_mut()
            .map(|child| child.layout(Size::new(f64::INFINITY, available.height)))
            .collect();
        let height = sizes.iter().fold(0.0_f64, |h, s| h.max(s.height));
        let mut x = 0.0;
        for ((child, size), origin) in self.children.iter_mut().zip(&sizes).zip(&self.origins) {
            // Each piece is centred vertically.
            let y = ((height - size.height) / 2.0).floor();
            child.set_bounds(Rect::new(x, y, size.width, size.height));
            x += size.width + GAP;
            if origin.is_none() {
                x += ELLIPSIS_RIGHT_MARGIN;
            }
        }
        Size::new(available.width.min((x - GAP).max(0.0)), height)
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// What [`folder_bread_crumb_for`] builds a trail from, besides its click handler and font (a
/// browser's own, fixed for its life): two trails of equal keys look and act the same.
#[derive(Clone, PartialEq)]
pub(crate) struct CrumbKey {
    names: Vec<String>,
    style: BrowserStyle,
    widget_names: BrowserNames,
    up_tooltip: String,
}

impl CrumbKey {
    /// The key of the trail for `names` (the root first) drawn in `look` now: the look's style is
    /// read as it is now (a theme switch changes it), and so is the tooltip's language.
    pub(crate) fn new(names: &[String], look: &Rc<dyn BrowserLook>) -> Self {
        Self {
            names: names.to_vec(),
            style: look.style(),
            widget_names: look.names().clone(),
            up_tooltip: tr("Click to go back"),
        }
    }
}

/// The breadcrumb trail for `names` (the root first) drawn in `look`: the up button, then a link
/// per name, each calling `go_to` with its depth (0 = the root), all in `font`.
///
/// The up button draws [`BrowserIcon::Up`] (asked for at [`UP_ICON_SIZE`]) on a square the
/// style's button height wide, in the text colour with the hover fill under the pointer; it says
/// "Click to go back" and is disabled at the root, which has nothing above it. The separators are
/// "/" one unit larger than the links, which draw in the text colour, underlined until hovered.
/// Each piece carries its name from the look ([`crate::BrowserNames`]).
pub fn folder_bread_crumb_for(
    names: &[String],
    go_to: Rc<dyn Fn(usize)>,
    look: &Rc<dyn BrowserLook>,
    font: &Arc<Font>,
) -> FolderBreadCrumbWidget {
    let style = look.style();
    let widget_names = look.names();
    let mut pieces: Vec<Box<dyn Widget>> = Vec::new();
    let depth = names.len();
    let up_go_to = Rc::clone(&go_to);
    let up = BrowserIconButton::new(Rc::clone(look), BrowserIcon::Up, style.button_height)
        .with_icon_size(UP_ICON_SIZE)
        .with_name(widget_names.up_button.clone())
        .with_tooltip_text(&tr("Click to go back"))
        .with_enabled(depth > 1)
        .on_click(move || up_go_to(depth.saturating_sub(2)));
    pieces.push(Box::new(up));
    let text = style.text_color;
    let font_size = style.font_size;
    for (i, name) in names.iter().enumerate() {
        if i > 0 {
            let separator = Label::new("/", Arc::clone(font))
                .with_font_size(font_size + 1.0)
                .with_color(text);
            pieces.push(Box::new(separator));
        }
        let go_to = Rc::clone(&go_to);
        let link = LinkLabel::new(
            widget_names.bread_crumb_button_for(name),
            name.clone(),
            Arc::clone(font),
            font_size,
            text,
            move || go_to(i),
        );
        pieces.push(Box::new(link));
    }
    FolderBreadCrumbWidget::new(&widget_names.bread_crumb, pieces, font, font_size, text)
}
