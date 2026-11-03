//! The composed browser's layout pieces (`browser/mod.rs` places them): [`SearchSlot`], the
//! place in a rebuilt header where the browser's one search box stands; [`BottomBorder`], a bar
//! with a one-pixel rule along its bottom (the nav bar and the list-only form's toolbar); and
//! [`ListColumn`], the list under the showing folder's header markdown (a folder's own words
//! about what it holds, through agg-gui's `MarkdownView` or the look's own markdown widget,
//! `BrowserLook::header_markdown`, left out when the folder has none, and while a remembered
//! folder is still being found, whose loading line stands under the trail being found rather than
//! under the root's words) and the look's banner
//! (`BrowserLook::list_banner`), built again only when that text changes or the list shows its
//! content anew (`BrowserState::list_resets`).
//!
//! The header is built again whenever the showing folder changes (its breadcrumbs are the path),
//! but the search box is not, nor the bar around it: [`keep_search_bar`] keeps the old bar, the
//! row in it and the slot holding the box, and takes everything else from the new build, so the
//! box keeps its text, focus and cursor while the user types (a search that runs shows its
//! results as a folder, which rebuilds the header), and it and its clear button stay the same
//! widgets under the same parents, which agg-gui's identity anchors (the pointer's hover and
//! capture, GUI automation's widget handles) follow. A bar whose shape changed (the list-only
//! form's toolbar losing its view button on a phone's panel) is the new build's, the box moved
//! into it ([`take_search_box`]).

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::text::Font;
use agg_gui::widgets::{MarkdownView, Named};
use agg_gui::{Color, DrawCtx, Event, EventResult, Rect, Size, Widget};

use crate::state::BrowserState;

/// The search slot's type name, which [`take_search_box`] finds it by.
const SEARCH_SLOT: &str = "SearchSlot";

/// The place in the header where the search box stands (see the module header).
pub struct SearchSlot {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl SearchSlot {
    /// The slot holding `search_box`.
    pub fn new(search_box: Box<dyn Widget>) -> Self {
        Self {
            bounds: Rect::default(),
            children: vec![search_box],
        }
    }

    /// The slot holding `search_box`, or an empty one a kept bar's slot stands for
    /// ([`keep_search_bar`]).
    pub fn new_or_empty(search_box: Option<Box<dyn Widget>>) -> Self {
        Self {
            bounds: Rect::default(),
            children: search_box.into_iter().collect(),
        }
    }
}

impl Widget for SearchSlot {
    fn type_name(&self) -> &'static str {
        SEARCH_SLOT
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
    /// As big as the box it holds.
    fn layout(&mut self, available: Size) -> Size {
        let Some(child) = self.children.first_mut() else {
            return Size::new(0.0, 0.0);
        };
        let size = child.layout(available);
        child.set_bounds(Rect::new(0.0, 0.0, size.width, size.height));
        size
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// Takes the search box out of the [`SearchSlot`] somewhere under `widgets` (a header about to be
/// replaced), so the rebuilt header can stand the same box in its own slot.
pub fn take_search_box(widgets: &mut [Box<dyn Widget>]) -> Option<Box<dyn Widget>> {
    for widget in widgets.iter_mut() {
        if widget.type_name() == SEARCH_SLOT {
            return widget.children_mut().pop();
        }
        if let Some(found) = take_search_box(widget.children_mut()) {
            return Some(found);
        }
    }
    None
}

/// Whether a [`SearchSlot`] stands somewhere in `widget` (or is it).
pub fn holds_search_slot(widget: &dyn Widget) -> bool {
    widget.type_name() == SEARCH_SLOT
        || widget
            .children()
            .iter()
            .any(|c| holds_search_slot(c.as_ref()))
}

/// The header bar `new` (just built, its search slot empty) with the search box of `old` (the
/// bar it replaces): `old` itself when the two have the same shape along the way to the slot,
/// with every child off that way taken from `new`; otherwise `new`, the box moved into its slot.
pub fn keep_search_bar(mut old: Box<dyn Widget>, mut new: Box<dyn Widget>) -> Box<dyn Widget> {
    if same_way_to_slot(old.as_ref(), new.as_ref()) {
        graft(old.as_mut(), new.as_mut());
        return old;
    }
    if let Some(search_box) = take_search_box(std::slice::from_mut(&mut old)) {
        put_search_box(new.as_mut(), search_box);
    }
    new
}

/// Whether `old` and `new` are the same kind of widget with as many children, down the way to
/// the slot in `old`, and `new`'s slot is at the end of the same way.
fn same_way_to_slot(old: &dyn Widget, new: &dyn Widget) -> bool {
    if old.type_name() != new.type_name() {
        return false;
    }
    if old.type_name() == SEARCH_SLOT {
        return true;
    }
    let (olds, news) = (old.children(), new.children());
    if olds.len() != news.len() {
        return false;
    }
    olds.iter()
        .zip(news)
        .find(|(o, _)| holds_search_slot(o.as_ref()))
        .is_some_and(|(o, n)| same_way_to_slot(o.as_ref(), n.as_ref()))
}

/// Swaps every child of `old` off the way to its slot for `new`'s at the same place (the shapes
/// match, [`same_way_to_slot`]); the slot and everything over it stay `old`'s.
fn graft(old: &mut dyn Widget, new: &mut dyn Widget) {
    if old.type_name() == SEARCH_SLOT {
        return;
    }
    let news = new.children_mut();
    for (i, child) in old.children_mut().iter_mut().enumerate() {
        match holds_search_slot(child.as_ref()) {
            true => graft(child.as_mut(), news[i].as_mut()),
            false => std::mem::swap(child, &mut news[i]),
        }
    }
}

/// Stands `search_box` in the (empty) [`SearchSlot`] somewhere in `widget`.
fn put_search_box(widget: &mut dyn Widget, search_box: Box<dyn Widget>) {
    if widget.type_name() == SEARCH_SLOT {
        widget.children_mut().push(search_box);
        return;
    }
    if let Some(child) = widget
        .children_mut()
        .iter_mut()
        .find(|c| holds_search_slot(c.as_ref()))
    {
        put_search_box(child.as_mut(), search_box);
    }
}

/// A child with a one-pixel rule along its bottom, in `color`.
pub struct BottomBorder {
    color: Color,
    name: Option<String>,
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl BottomBorder {
    /// `child` over a rule in `color`.
    pub fn new(child: Box<dyn Widget>, color: Color) -> Self {
        Self {
            color,
            name: None,
            bounds: Rect::default(),
            children: vec![child],
        }
    }

    /// The same bar, named `name` (the list-only form's "Folders Toolbar").
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }
}

impl Widget for BottomBorder {
    fn type_name(&self) -> &'static str {
        "OverflowBar"
    }
    fn id(&self) -> Option<&str> {
        self.name.as_deref()
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
    /// As tall as the child, plus the rule under it.
    fn layout(&mut self, available: Size) -> Size {
        let mut size = Size::new(available.width, 0.0);
        if let Some(child) = self.children.first_mut() {
            let inner = Size::new(available.width, (available.height - 1.0).max(0.0));
            let child_size = child.layout(inner);
            size.height = child_size.height + 1.0;
            child.set_bounds(Rect::new(0.0, 1.0, available.width, child_size.height));
        }
        size
    }
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        ctx.set_fill_color(self.color);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, self.bounds.width, 1.0);
        ctx.fill();
    }
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// The list under the showing folder's header markdown (see the module header).
pub struct ListColumn {
    state: Rc<RefCell<BrowserState>>,
    font: Arc<Font>,
    name: String,
    /// The header text and list reset count the children were built for.
    built: Option<(String, u64)>,
    bounds: Rect,
    /// The header (when the folder has one), the look's banner (when it gives one), then the
    /// list.
    children: Vec<Box<dyn Widget>>,
}

impl ListColumn {
    /// `list` under the header markdown of what `state` shows, the header drawn in `font` and
    /// named `name`.
    pub fn new(
        state: &Rc<RefCell<BrowserState>>,
        list: Box<dyn Widget>,
        font: Arc<Font>,
        name: impl Into<String>,
    ) -> Self {
        Self {
            state: Rc::clone(state),
            font,
            name: name.into(),
            built: None,
            bounds: Rect::default(),
            children: vec![list],
        }
    }

    /// Puts the header for `text` and the look's banner above the list (the header left out for
    /// an empty text); `shown_anew` says the list shows its content anew.
    fn set_header(&mut self, text: &str, shown_anew: bool) {
        let list = self.children.pop();
        self.children.clear();
        let look = Rc::clone(&self.state.borrow().config().look);
        if !text.is_empty() {
            let view = look
                .header_markdown(text, &self.font)
                .unwrap_or_else(|| Box::new(MarkdownView::new(text, Arc::clone(&self.font))));
            self.children
                .push(Box::new(Named::new(self.name.clone(), view)));
        }
        if let Some(banner) = look.list_banner(&self.state, shown_anew, &self.font) {
            self.children.push(banner);
        }
        self.children.extend(list);
    }
}

impl Widget for ListColumn {
    fn type_name(&self) -> &'static str {
        "ListColumn"
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
    /// The header as tall as its text at the top, the list in the rest.
    fn layout(&mut self, available: Size) -> Size {
        let (text, resets) = {
            let s = self.state.borrow();
            // The header showing is the root's while a remembered folder is found: none then.
            let text = match s.is_restoring() {
                true => String::new(),
                false => s.header_markdown(),
            };
            (text, s.list_resets())
        };
        let built = self.built.as_ref().map(|(t, r)| (t.as_str(), *r));
        if built != Some((text.as_str(), resets)) {
            let anew = built.is_none_or(|(_, r)| r != resets);
            self.set_header(&text, anew);
            self.built = Some((text, resets));
        }
        let mut top = available.height;
        let count = self.children.len();
        for (i, child) in self.children.iter_mut().enumerate() {
            if i + 1 < count {
                let size = child.layout(Size::new(available.width, top));
                // A banner with nothing to say hides itself: no room and no gap, as a column
                // leaves out a hidden child.
                if !child.is_visible() {
                    child.set_bounds(Rect::new(0.0, top, 0.0, 0.0));
                    continue;
                }
                top -= size.height;
                child.set_bounds(Rect::new(0.0, top, available.width, size.height));
                top -= super::PIECE_GAP;
            } else {
                let room = Size::new(available.width, top.max(0.0));
                child.layout(room);
                child.set_bounds(Rect::new(0.0, 0.0, room.width, room.height));
            }
        }
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}
