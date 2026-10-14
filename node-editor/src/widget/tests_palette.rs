//! Tests for the host palette (`palette_source.rs`): a palette set with
//! `set_palette`, or built per theme by `set_palette_fn`, is what the canvas
//! (`paint.rs`) and the hosted cards' chrome (`hosted.rs`) draw with through
//! every repaint and relayout, until the host clears it.

use std::cell::Cell;
use std::rc::Rc;

use agg_gui::{Color, Size};

use super::hosted::HostedNodeBody;
use super::tests_common::{mk_node, Memory};
use super::tests_socket_at::EmptyBody;
use super::*;
use crate::model::NodeView;
use crate::test_recorder::{same, Op, Recorder};

const VIEW: Size = Size {
    width: 800.0,
    height: 600.0,
};

/// An editor over one hosted card, laid out once.
fn editor(setup: impl FnOnce(NodeEditor) -> NodeEditor) -> NodeEditor {
    let m = Memory {
        nodes: vec![mk_node(1, "Box", [100.0, 400.0])],
        ..Memory::default()
    };
    let shared: SharedModel = Arc::new(Mutex::new(m));
    let editor = NodeEditor::new(shared).with_body_factory(|_n: &NodeView| {
        Some(HostedNodeBody::new(Box::new(EmptyBody::default())))
    });
    let mut editor = setup(editor);
    editor.set_bounds(Rect::new(0.0, 0.0, VIEW.width, VIEW.height));
    editor.layout(VIEW);
    editor
}

/// The colour of the canvas background fill (the first fill, over the
/// whole editor).
fn canvas_fill(editor: &mut NodeEditor) -> Color {
    let mut r = Recorder::default();
    editor.paint_canvas(&mut r);
    let first = r.fills().next().expect("the canvas fills its background");
    assert_eq!(
        first.path.first(),
        Some(&Op::Rect([0.0, 0.0], [VIEW.width, VIEW.height]))
    );
    first.color
}

/// The hosted card's body and border colours.
fn card_colors(editor: &mut NodeEditor) -> (Color, Color) {
    let card = editor
        .children_mut()
        .last_mut()
        .and_then(|l| l.children_mut().first_mut())
        .and_then(|c| c.as_any_mut())
        .and_then(|a| a.downcast_mut::<HostedCard>())
        .expect("hosted card");
    (card.chrome.body, card.chrome.border)
}

/// MatterCAD's node editor colours over a theme background and accent.
fn mattercad(background: Color, accent: Color) -> CanvasPalette {
    let mut p = CanvasPalette::from_visuals(&agg_gui::current_visuals());
    p.canvas_bg = Color::rgba(background.r, background.g, background.b, 150.0 / 255.0);
    p.node_body = Color::rgba(background.r, background.g, background.b, 235.0 / 255.0);
    p.node_border = accent;
    p
}

#[test]
fn a_host_palette_survives_repaint_and_relayout() {
    let host = mattercad(Color::rgb(0.2, 0.3, 0.4), Color::rgb(0.9, 0.5, 0.1));
    let mut e = editor(|e| e.with_palette(host.clone()));
    for _ in 0..2 {
        assert!(same(canvas_fill(&mut e), host.canvas_bg));
        let (body, border) = card_colors(&mut e);
        assert!(same(body, host.node_body), "{body:?}");
        assert!(same(border, host.node_border), "{border:?}");
        assert_eq!(e.palette(), &host);
        e.layout(VIEW);
    }
}

#[test]
fn a_palette_set_after_layout_reaches_the_cards_at_the_next_layout() {
    let mut e = editor(|e| e);
    let host = mattercad(Color::rgb(0.1, 0.1, 0.1), Color::rgb(0.0, 0.6, 1.0));
    e.set_palette(host.clone());
    e.layout(VIEW);
    let (body, border) = card_colors(&mut e);
    assert!(same(body, host.node_body));
    assert!(same(border, host.node_border));
    assert!(same(canvas_fill(&mut e), host.canvas_bg));
}

#[test]
fn a_palette_fn_follows_the_hosts_theme() {
    let accent = Rc::new(Cell::new(Color::rgb(1.0, 0.0, 0.0)));
    let theme = Rc::clone(&accent);
    let background = Color::rgb(0.95, 0.95, 0.95);
    let mut e = editor(|e| e.with_palette_fn(move |_v| mattercad(background, theme.get())));
    assert!(same(card_colors(&mut e).1, Color::rgb(1.0, 0.0, 0.0)));

    // The host's theme changes; the next layout and paint follow it.
    accent.set(Color::rgb(0.0, 0.0, 1.0));
    e.layout(VIEW);
    assert!(same(card_colors(&mut e).1, Color::rgb(0.0, 0.0, 1.0)));
    assert!(same(
        canvas_fill(&mut e),
        Color::rgba(0.95, 0.95, 0.95, 150.0 / 255.0)
    ));
}

#[test]
fn clearing_the_host_palette_goes_back_to_the_visuals() {
    let host = mattercad(Color::rgb(0.2, 0.3, 0.4), Color::rgb(0.9, 0.5, 0.1));
    let mut e = editor(|e| e.with_palette(host));
    e.clear_palette();
    e.layout(VIEW);
    let visuals = CanvasPalette::from_visuals(&agg_gui::current_visuals());
    assert!(same(card_colors(&mut e).0, visuals.node_body));
    assert!(same(canvas_fill(&mut e), visuals.canvas_bg));
}

#[test]
fn a_host_palette_reaches_the_simplified_cards() {
    let host = mattercad(Color::rgb(0.2, 0.3, 0.4), Color::rgb(0.9, 0.5, 0.1));
    let m = Memory {
        nodes: vec![mk_node(1, "Box", [100.0, 400.0])],
        ..Memory::default()
    };
    let shared: SharedModel = Arc::new(Mutex::new(m));
    let mut e = NodeEditor::new(shared).with_palette(host.clone());
    e.set_bounds(Rect::new(0.0, 0.0, VIEW.width, VIEW.height));
    for _ in 0..2 {
        e.layout(VIEW);
        let card = e.children_mut().first_mut().expect("the node's card");
        let mut r = Recorder::default();
        card.paint(&mut r);
        assert!(
            r.fills().any(|s| same(s.color, host.node_body)),
            "the card body is the host's"
        );
        assert!(r.strokes().any(|s| same(s.color, host.node_border)));
    }
}
