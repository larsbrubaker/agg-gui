//! `is_local_rect_in_paint_clip` is public, so widgets outside agg-gui can
//! gate self-animation on visibility the way `Spinner` and `ProgressBar` do.
//!
//! The traversal still paints children a clipping ancestor hides; a
//! self-animating widget there must not re-arm its wake-up. This drives a
//! real `paint_subtree` pass with one child inside its parent's clip and
//! one outside it and checks what each sees, then that a child which opts
//! into `skip_paint_when_clipped_out` is left unpainted, with its subtree,
//! while none of it is inside the clip.

use std::cell::RefCell;
use std::rc::Rc;

use agg_gui::widget::{is_local_rect_in_paint_clip, paint_subtree, BackbufferCache};
use agg_gui::{DrawCtx, Event, EventResult, Framebuffer, GfxCtx, Rect, Size, Widget};

type Seen = Rc<RefCell<Vec<(&'static str, bool)>>>;

struct Node {
    name: &'static str,
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    seen: Seen,
    skip_when_clipped_out: bool,
}

impl Widget for Node {
    fn type_name(&self) -> &'static str {
        "Node"
    }
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
        Size::new(self.bounds.width, self.bounds.height)
    }
    fn skip_paint_when_clipped_out(&self) -> bool {
        self.skip_when_clipped_out
    }
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let (w, h) = (self.bounds.width, self.bounds.height);
        let visible = is_local_rect_in_paint_clip(ctx, 0.0, 0.0, w, h);
        self.seen.borrow_mut().push((self.name, visible));
    }
    fn on_event(&mut self, _e: &Event) -> EventResult {
        EventResult::Ignored
    }
}

fn node(name: &'static str, bounds: Rect, children: Vec<Box<dyn Widget>>, seen: &Seen) -> Node {
    Node {
        name,
        bounds,
        children,
        seen: Rc::clone(seen),
        skip_when_clipped_out: false,
    }
}

/// [`node`] that opts into `skip_paint_when_clipped_out`.
fn skipping_node(
    name: &'static str,
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    seen: &Seen,
) -> Node {
    Node {
        skip_when_clipped_out: true,
        ..node(name, bounds, children, seen)
    }
}

#[test]
fn clipped_out_child_reports_not_visible() {
    let seen: Seen = Rc::default();
    let inside = node("inside", Rect::new(10.0, 10.0, 20.0, 20.0), vec![], &seen);
    let outside = node("outside", Rect::new(150.0, 10.0, 20.0, 20.0), vec![], &seen);
    let mut root = node(
        "root",
        Rect::new(0.0, 0.0, 100.0, 100.0),
        vec![Box::new(inside), Box::new(outside)],
        &seen,
    );
    let mut fb = Framebuffer::new(200, 100);
    let mut ctx = GfxCtx::new(&mut fb);
    paint_subtree(&mut root, &mut ctx);
    assert_eq!(
        *seen.borrow(),
        vec![("root", true), ("inside", true), ("outside", false)]
    );
}

#[test]
fn an_opted_in_child_outside_the_clip_is_not_painted_with_its_subtree() {
    let seen: Seen = Rc::default();
    let grandchild = node(
        "outside_grandchild",
        Rect::new(1.0, 1.0, 5.0, 5.0),
        vec![],
        &seen,
    );
    let inside = skipping_node("inside", Rect::new(10.0, 10.0, 20.0, 20.0), vec![], &seen);
    let outside = skipping_node(
        "outside",
        Rect::new(150.0, 10.0, 20.0, 20.0),
        vec![Box::new(grandchild)],
        &seen,
    );
    // Not opted in: painted although hidden, as before.
    let plain_outside = node(
        "plain_outside",
        Rect::new(160.0, 40.0, 20.0, 20.0),
        vec![],
        &seen,
    );
    let mut root = node(
        "root",
        Rect::new(0.0, 0.0, 100.0, 100.0),
        vec![Box::new(inside), Box::new(outside), Box::new(plain_outside)],
        &seen,
    );
    let mut fb = Framebuffer::new(200, 100);
    let mut ctx = GfxCtx::new(&mut fb);
    paint_subtree(&mut root, &mut ctx);
    assert_eq!(
        *seen.borrow(),
        vec![("root", true), ("inside", true), ("plain_outside", false)]
    );
}

#[test]
fn an_opted_in_child_partly_inside_the_clip_is_painted() {
    let seen: Seen = Rc::default();
    let straddling = skipping_node(
        "straddling",
        Rect::new(90.0, 10.0, 20.0, 20.0),
        vec![],
        &seen,
    );
    let mut root = node(
        "root",
        Rect::new(0.0, 0.0, 100.0, 100.0),
        vec![Box::new(straddling)],
        &seen,
    );
    let mut fb = Framebuffer::new(200, 100);
    let mut ctx = GfxCtx::new(&mut fb);
    paint_subtree(&mut root, &mut ctx);
    assert_eq!(*seen.borrow(), vec![("root", true), ("straddling", true)]);
}

/// A CPU-backbuffered container (the way a `Label` is): its subtree paints into a bitmap of its
/// own, so the paint clip stack below it has to describe that bitmap, not the outer root.
struct Buffered {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    cache: BackbufferCache,
}

impl Widget for Buffered {
    fn type_name(&self) -> &'static str {
        "Buffered"
    }
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
        Size::new(self.bounds.width, self.bounds.height)
    }
    fn backbuffer_cache_mut(&mut self) -> Option<&mut BackbufferCache> {
        Some(&mut self.cache)
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _e: &Event) -> EventResult {
        EventResult::Ignored
    }
}

#[test]
fn an_opted_in_child_below_a_cpu_backbuffer_is_painted_in_the_bitmaps_own_space() {
    let seen: Seen = Rc::default();
    // In the bitmap's space (the buffered widget's own, 100 x 100) the child sits at (50, 50);
    // in the outer root's space the visible clip is only the 20 x 20 corner, which that rect
    // misses. The child is on screen as far as the bitmap goes, and the bitmap is cached.
    let child = skipping_node(
        "in_bitmap",
        Rect::new(50.0, 50.0, 10.0, 10.0),
        vec![],
        &seen,
    );
    let buffered = Buffered {
        bounds: Rect::new(0.0, 0.0, 100.0, 100.0),
        children: vec![Box::new(child)],
        cache: BackbufferCache::new(),
    };
    let mut root = node(
        "root",
        Rect::new(0.0, 0.0, 20.0, 20.0),
        vec![Box::new(buffered)],
        &seen,
    );
    let mut fb = Framebuffer::new(200, 100);
    let mut ctx = GfxCtx::new(&mut fb);
    paint_subtree(&mut root, &mut ctx);
    assert_eq!(
        *seen.borrow(),
        vec![("root", true), ("in_bitmap", true)],
        "the child is painted into the bitmap, which is then cached with it"
    );
}
