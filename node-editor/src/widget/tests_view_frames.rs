//! Tests for view-only frames (`view_frames.rs`), the cards' retained layers
//! (`hosted_card.rs`) and the canvas paint's model read (`paint_cache.rs`):
//! a pan lays no card body out again, keeps every card's layer and paints
//! without reading the model again; any other draw request makes the next
//! layout a full one that drops the layers; a zoom composites the cards
//! stretched until it settles, then rasterises them again.

use std::cell::Cell;
use std::rc::Rc;

use agg_gui::{
    DrawCtx, Event, Framebuffer, GfxCtx, Modifiers, MouseButton, Point, Rect, Size, Widget,
};

use super::hosted::HostedNodeBody;
use super::hosted_card::{HostedLayer, ZOOM_SETTLE};
use super::tests_common::Memory;
use super::tests_hosted::{dispatch, node_with_sockets};
use super::*;
use crate::model::{NodeView, NoodleView};

const VIEW: Size = Size {
    width: 800.0,
    height: 600.0,
};

/// A point of the canvas no card covers.
const EMPTY: Point = Point { x: 700.0, y: 60.0 };

/// A body of a fixed height counting its layouts.
struct CountingBody {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    layouts: Rc<Cell<usize>>,
}

impl Widget for CountingBody {
    fn type_name(&self) -> &'static str {
        "CountingBody"
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
    fn measure_min_height(&self, _w: f64) -> f64 {
        40.0
    }
    fn layout(&mut self, available: Size) -> Size {
        self.layouts.set(self.layouts.get() + 1);
        Size::new(available.width, 40.0)
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _: &Event) -> agg_gui::EventResult {
        agg_gui::EventResult::Ignored
    }
}

struct Fixture {
    editor: NodeEditor,
    /// Card body layouts so far.
    layouts: Rc<Cell<usize>>,
}

/// Two hosted cards joined by a noodle, laid out.
fn fixture() -> Fixture {
    let model = Memory {
        nodes: vec![
            node_with_sockets(1, [100.0, 400.0]),
            node_with_sockets(2, [400.0, 400.0]),
        ],
        noodles: vec![NoodleView {
            from_node: NodeId(1),
            from_socket: "Result".into(),
            to_node: NodeId(2),
            to_socket: "Width".into(),
        }],
        ..Memory::default()
    };
    let shared: SharedModel = Arc::new(Mutex::new(model));
    let layouts = Rc::new(Cell::new(0));
    let counted = layouts.clone();
    let mut editor = NodeEditor::new(shared).with_body_factory(move |_n: &NodeView| {
        Some(HostedNodeBody::new(Box::new(CountingBody {
            bounds: Rect::default(),
            children: Vec::new(),
            layouts: counted.clone(),
        })))
    });
    editor.layout(VIEW);
    Fixture { editor, layouts }
}

/// A middle press: the button that pans whatever the mode and the host.
fn middle_press(pos: Point) -> Event {
    Event::MouseDown {
        pos,
        button: MouseButton::Middle,
        modifiers: Modifiers::default(),
    }
}

fn wheel(pos: Point, notches: f64) -> Event {
    Event::MouseWheel {
        pos,
        delta_x: 0.0,
        delta_y: notches,
        modifiers: Modifiers::default(),
    }
}

/// Press the middle button on empty canvas and lay out that frame (a full
/// one: the press itself is a change), as a pan starts.
fn start_pan(editor: &mut NodeEditor) {
    dispatch(editor, middle_press(EMPTY), EMPTY);
    editor.layout(VIEW);
}

/// One pan step to `EMPTY` moved by `step` pixels, and its frame's layout.
fn pan_step(editor: &mut NodeEditor, step: f64) {
    let to = Point::new(EMPTY.x - step, EMPTY.y + step);
    let before = editor.pan();
    dispatch(editor, Event::MouseMove { pos: to }, to);
    editor.layout(VIEW);
    assert_ne!(editor.pan(), before, "the drag pans the view");
}

fn layer(editor: &mut NodeEditor) -> &mut HostedLayer {
    editor
        .children_mut()
        .last_mut()
        .and_then(|w| w.as_any_mut())
        .and_then(|a| a.downcast_mut::<HostedLayer>())
        .expect("the hosted layer")
}

/// Every card's retained layer state, as a paint left it (clean) or as
/// something since dropped it (dirty).
fn card_layers_dirty(editor: &mut NodeEditor) -> Vec<bool> {
    layer(editor)
        .children_mut()
        .iter_mut()
        .map(|c| c.backbuffer_state_mut().expect("a card has a layer").dirty)
        .collect()
}

/// Mark every card's layer clean, as a GPU paint that rasterised them does.
fn rasterise_cards(editor: &mut NodeEditor) {
    for card in layer(editor).children_mut() {
        card.backbuffer_state_mut()
            .expect("a card has a layer")
            .dirty = false;
    }
}

fn paint(editor: &mut NodeEditor) {
    let mut fb = Framebuffer::new(VIEW.width as u32, VIEW.height as u32);
    let mut ctx = GfxCtx::new(&mut fb);
    editor.paint(&mut ctx);
    editor.finish_paint(&mut ctx);
}

#[test]
fn a_pan_lays_no_card_body_out_again_and_moves_the_cards_with_the_view() {
    let mut f = fixture();
    start_pan(&mut f.editor);
    f.layouts.set(0);
    for step in 1..=5 {
        pan_step(&mut f.editor, 10.0 * f64::from(step));
    }
    assert_eq!(
        f.layouts.get(),
        0,
        "a pan step's layout lays no card body out again"
    );
    let offset = f.editor.canvas_offset;
    let t = layer(&mut f.editor)
        .child_transform()
        .expect("the layer places its cards");
    assert_eq!([t.tx, t.ty], offset, "the cards are placed at the new view");

    // Anything else asking for a frame (an edit, a hover) lays the cards out again.
    agg_gui::animation::request_draw();
    f.editor.layout(VIEW);
    assert!(
        f.layouts.get() > 0,
        "a layout after another draw request is a full one"
    );
}

#[test]
fn a_pan_keeps_every_cards_layer_and_any_other_change_drops_them() {
    let mut f = fixture();
    start_pan(&mut f.editor);
    rasterise_cards(&mut f.editor);
    for step in 1..=3 {
        pan_step(&mut f.editor, 10.0 * f64::from(step));
    }
    assert_eq!(
        card_layers_dirty(&mut f.editor),
        vec![false, false],
        "a pan composites the cards' layers as they are"
    );
    assert!(
        f.editor.backbuffer.dirty,
        "the canvas itself is drawn again at the new view"
    );

    agg_gui::animation::request_draw();
    f.editor.layout(VIEW);
    assert_eq!(
        card_layers_dirty(&mut f.editor),
        vec![true, true],
        "a full layout draws every card again"
    );
}

#[test]
fn a_pan_paints_from_the_model_read_it_already_made() {
    let mut f = fixture();
    start_pan(&mut f.editor);
    paint(&mut f.editor);
    let read = f.editor.paint_read();
    for step in 1..=3 {
        pan_step(&mut f.editor, 10.0 * f64::from(step));
        paint(&mut f.editor);
        assert!(
            Rc::ptr_eq(&read, &f.editor.paint_read()),
            "pan step {step} paints from the same model read"
        );
    }
    assert_eq!(read.noodles.len(), 1, "the read holds the noodle");

    agg_gui::animation::request_draw();
    f.editor.layout(VIEW);
    assert!(
        !Rc::ptr_eq(&read, &f.editor.paint_read()),
        "a change of the canvas's content reads the model again"
    );
}

#[test]
fn a_zoom_stretches_the_cards_layers_until_it_settles_then_draws_them_crisp() {
    let _clock = agg_gui::clock::scoped_virtual(None);
    let mut f = fixture();
    rasterise_cards(&mut f.editor);
    let scale = f.editor.scale();

    dispatch(&mut f.editor, wheel(EMPTY, 1.0), EMPTY);
    f.editor.layout(VIEW);
    assert!(f.editor.scale() > scale, "the wheel zoomed in");
    let stretched: Vec<bool> = layer(&mut f.editor)
        .children()
        .iter()
        .map(|c| c.retained_layer_may_stretch())
        .collect();
    assert_eq!(
        stretched,
        vec![true, true],
        "while the zoom moves, the cards' layers are composited stretched"
    );
    assert_eq!(card_layers_dirty(&mut f.editor), vec![false, false]);
    assert!(
        !f.editor.needs_draw(),
        "nothing more is drawn while the zoom is still moving"
    );
    let settle = f
        .editor
        .next_draw_deadline()
        .expect("the editor asks for a frame once the zoom settles");
    assert_eq!(settle, agg_gui::clock::now() + ZOOM_SETTLE);

    agg_gui::clock::advance(ZOOM_SETTLE);
    assert!(
        f.editor.needs_draw(),
        "the settled zoom's frame draws the canvas again"
    );
    // That frame is laid out because the layer asked to be drawn; it lays no card body out.
    f.layouts.set(0);
    f.editor.layout(VIEW);
    assert_eq!(
        f.layouts.get(),
        0,
        "the settling frame lays no card body out"
    );
    paint_layer(&mut f.editor);
    let stretched: Vec<bool> = layer(&mut f.editor)
        .children()
        .iter()
        .map(|c| c.retained_layer_may_stretch())
        .collect();
    assert_eq!(stretched, vec![false, false]);
    assert_eq!(
        card_layers_dirty(&mut f.editor),
        vec![true, true],
        "the settled zoom rasterises every card again, at the new scale"
    );
    assert!(!f.editor.needs_draw());
    assert_eq!(f.editor.next_draw_deadline(), None);
}

/// The hosted layer's paint, as the editor's subtree paint runs it.
fn paint_layer(editor: &mut NodeEditor) {
    let mut fb = Framebuffer::new(VIEW.width as u32, VIEW.height as u32);
    let mut ctx = GfxCtx::new(&mut fb);
    layer(editor).paint(&mut ctx);
}
