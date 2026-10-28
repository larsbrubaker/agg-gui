//! Tests for rubber-band selection (`rubber_band.rs`): a drag from empty
//! canvas selects every card the rectangle touches, Shift adds to the
//! selection held at the press, Shift with Ctrl/Cmd toggles against it,
//! Escape puts it back, a press that hardly moves is still a click, the
//! Pan mode never draws a band, the cards preview the selection while the
//! band drags, the primary selection settles as `select_nodes` reports it,
//! and the band is painted in the palette's colours.

use std::sync::Arc;

use agg_gui::text::Font;
use agg_gui::{Color, Framebuffer, GfxCtx, Key, Modifiers, MouseButton, Point, Slider};

use super::hosted::HostedNodeBody;
use super::tests_common::{
    fixture_with_typed_handle, mk_node, seed_nodes, Memory, TEST_FONT_FOR_PICKER,
};
use super::tests_hosted::{card_by_id, node_with_sockets, HostedModel};
use super::*;
use crate::model::NodeView;

/// Three simplified cards in the 400 x 300 pane (offset 0, scale 1, so
/// local == canvas): 1 top-left, 2 top-right from x = 250, 3 below 1.
fn editor() -> (NodeEditor, Arc<Mutex<Memory>>) {
    let (shared, typed) = fixture_with_typed_handle();
    let mut editor = NodeEditor::new(shared);
    seed_nodes(
        &mut editor,
        &typed,
        vec![
            mk_node(1, "a", [10.0, 280.0]),
            mk_node(2, "b", [250.0, 280.0]),
            mk_node(3, "c", [10.0, 120.0]),
        ],
    );
    (editor, typed)
}

fn set(ids: &[u64]) -> HashSet<NodeId> {
    ids.iter().map(|&id| NodeId(id)).collect()
}

fn shift() -> Modifiers {
    Modifiers {
        shift: true,
        ..Modifiers::default()
    }
}

/// Shift with the platform's command key (Ctrl, Cmd on a Mac).
fn shift_command() -> Modifiers {
    let command = agg_gui::platform::current_platform() == agg_gui::platform::Platform::MacOS;
    Modifiers {
        shift: true,
        ctrl: !command,
        meta: command,
        ..Modifiers::default()
    }
}

fn down(editor: &mut NodeEditor, p: Point, modifiers: Modifiers) {
    editor.on_event(&Event::MouseDown {
        pos: p,
        button: MouseButton::Left,
        modifiers,
    });
}

fn to(editor: &mut NodeEditor, p: Point) {
    editor.on_event(&Event::MouseMove { pos: p });
}

fn up(editor: &mut NodeEditor, p: Point, modifiers: Modifiers) {
    editor.on_event(&Event::MouseUp {
        pos: p,
        button: MouseButton::Left,
        modifiers,
    });
}

/// Press at `a`, drag through the midpoint to `b`, release at `b`.
fn band(editor: &mut NodeEditor, a: Point, b: Point, modifiers: Modifiers) {
    down(editor, a, modifiers);
    to(editor, Point::new((a.x + b.x) * 0.5, (a.y + b.y) * 0.5));
    to(editor, b);
    up(editor, b, modifiers);
}

/// Above every card, left of card 1.
fn empty() -> Point {
    Point::new(5.0, 295.0)
}

#[test]
fn a_band_selects_the_cards_it_touches_and_no_others() {
    let (mut editor, typed) = editor();
    editor.select_node(NodeId(3), false);
    // x 5..260 reaches card 1 (10..210) and card 2 (from 250); y 250..295
    // stays above card 3 (top 120).
    band(
        &mut editor,
        empty(),
        Point::new(260.0, 250.0),
        Modifiers::default(),
    );
    assert_eq!(editor.selected_ids(), &set(&[1, 2]), "plain drag replaces");
    let m = typed.lock().unwrap();
    assert_eq!(
        m.selection_reports,
        vec![Some(NodeId(3)), Some(NodeId(2))],
        "the old primary left the selection: the top-most selected card"
    );
}

#[test]
fn a_band_touching_a_card_edge_selects_it() {
    let (mut editor, _) = editor();
    band(
        &mut editor,
        Point::new(230.0, 295.0),
        Point::new(250.0, 270.0),
        Modifiers::default(),
    );
    assert_eq!(editor.selected_ids(), &set(&[2]));
}

#[test]
fn shift_adds_the_touched_cards_to_the_selection() {
    let (mut editor, typed) = editor();
    editor.select_node(NodeId(3), false);
    band(&mut editor, empty(), Point::new(100.0, 250.0), shift());
    assert_eq!(editor.selected_ids(), &set(&[1, 3]));
    assert_eq!(
        typed.lock().unwrap().selection_reports,
        vec![Some(NodeId(3))],
        "the primary is still selected: nothing more reported"
    );
}

#[test]
fn shift_with_the_command_key_toggles_the_touched_cards() {
    let (mut editor, typed) = editor();
    editor.select_nodes(&[NodeId(1), NodeId(3)], false);
    band(
        &mut editor,
        empty(),
        Point::new(260.0, 250.0),
        shift_command(),
    );
    assert_eq!(editor.selected_ids(), &set(&[2, 3]), "1 out, 2 in, 3 kept");
    assert_eq!(typed.lock().unwrap().last_selection, Some(NodeId(3)));
    assert_eq!(editor.pan(), [0.0, 0.0], "a band, not a pan");
}

#[test]
fn escape_during_a_band_restores_the_selection_held_at_the_press() {
    let (mut editor, typed) = editor();
    editor.select_node(NodeId(3), false);
    down(&mut editor, empty(), Modifiers::default());
    to(&mut editor, Point::new(260.0, 250.0));
    assert_eq!(editor.selected_ids(), &set(&[1, 2]), "previewed");
    editor.on_event(&Event::KeyDown {
        key: Key::Escape,
        modifiers: Modifiers::default(),
    });
    assert_eq!(editor.selected_ids(), &set(&[3]));
    assert!(editor.rubber_band_local_rect().is_none(), "band gone");
    up(&mut editor, Point::new(260.0, 250.0), Modifiers::default());
    assert_eq!(editor.selected_ids(), &set(&[3]), "release after Escape");
    let m = typed.lock().unwrap();
    assert_eq!(m.selection_reports, vec![Some(NodeId(3))]);
    assert_eq!(m.escapes, 0, "the band used the Escape");
}

#[test]
fn a_press_that_hardly_moves_is_a_click_on_empty_canvas() {
    let (mut editor, typed) = editor();
    editor.select_node(NodeId(1), false);
    band(
        &mut editor,
        empty(),
        Point::new(7.0, 294.0),
        Modifiers::default(),
    );
    assert!(editor.selected_ids().is_empty(), "a click clears");
    assert_eq!(typed.lock().unwrap().last_selection, None);

    editor.select_node(NodeId(1), false);
    band(&mut editor, empty(), Point::new(7.0, 294.0), shift());
    assert_eq!(editor.selected_ids(), &set(&[1]), "a Shift-click keeps it");
}

#[test]
fn a_band_that_touches_nothing_clears_and_reports_none() {
    let (mut editor, typed) = editor();
    editor.select_node(NodeId(1), false);
    band(
        &mut editor,
        empty(),
        Point::new(240.0, 290.0),
        Modifiers::default(),
    );
    assert!(editor.selected_ids().is_empty());
    assert_eq!(
        typed.lock().unwrap().selection_reports,
        vec![Some(NodeId(1)), None]
    );
}

#[test]
fn the_pan_mode_drags_the_view_instead_of_a_band() {
    let (mut editor, _) = editor();
    editor.mode = InteractionMode::Pan;
    band(
        &mut editor,
        empty(),
        Point::new(260.0, 250.0),
        Modifiers::default(),
    );
    assert!(editor.selected_ids().is_empty());
    assert_eq!(editor.pan(), [255.0, -45.0]);
}

#[test]
fn the_command_key_alone_still_pans_from_empty_canvas() {
    let (mut editor, _) = editor();
    let command = Modifiers {
        shift: false,
        ..shift_command()
    };
    band(&mut editor, empty(), Point::new(260.0, 250.0), command);
    assert!(editor.selected_ids().is_empty());
    assert_eq!(editor.pan(), [255.0, -45.0]);
}

#[test]
fn the_band_is_painted_in_the_palette_colours() {
    let (mut editor, _) = editor();
    let mut palette = editor.palette().clone();
    palette.selection_band_fill = Color::rgb(1.0, 0.0, 0.0);
    palette.selection_band_border = Color::rgb(0.0, 0.0, 1.0);
    editor.set_palette(palette);
    down(&mut editor, Point::new(20.0, 20.0), Modifiers::default());
    to(&mut editor, Point::new(60.0, 50.0));
    assert_eq!(
        editor.rubber_band_local_rect(),
        Some([20.0, 20.0, 40.0, 30.0])
    );
    let mut fb = Framebuffer::new(80, 80);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        editor.paint_rubber_band(&mut ctx);
    }
    let px = |x: u32, y: u32| {
        let i = ((y * fb.width() + x) * 4) as usize;
        fb.pixels()[i..i + 3].to_vec()
    };
    assert_eq!(px(40, 35), vec![255, 0, 0], "fill");
    assert_eq!(px(20, 35), vec![0, 0, 255], "outline");
    assert_eq!(px(10, 35), vec![0, 0, 0], "nothing outside");
}

/// Two hosted cards side by side at canvas top `400`: 1 at x 100, 2 at
/// x 400, in an 800 x 600 pane.
fn hosted_editor() -> (NodeEditor, Arc<Mutex<HostedModel>>) {
    let model = Arc::new(Mutex::new(HostedModel::default()));
    model.lock().unwrap().inner.nodes = vec![
        node_with_sockets(1, [100.0, 400.0]),
        node_with_sockets(2, [400.0, 400.0]),
    ];
    let shared: SharedModel = model.clone();
    let font = Arc::new(Font::from_slice(TEST_FONT_FOR_PICKER).unwrap());
    let mut editor = NodeEditor::new(shared).with_body_factory(move |_n: &NodeView| {
        let slider = Slider::new(0.0, 0.0, 100.0, Arc::clone(&font));
        Some(HostedNodeBody::new(Box::new(slider)))
    });
    editor.layout(Size::new(800.0, 600.0));
    (editor, model)
}

fn rings(editor: &mut NodeEditor) -> [bool; 2] {
    editor.layout(Size::new(800.0, 600.0));
    [1, 2].map(|id| card_by_id(editor, NodeId(id)).chrome.selected)
}

#[test]
fn the_cards_preview_the_band_while_it_drags() {
    let (mut editor, model) = hosted_editor();
    // The host's primary selection rings card 2 before the drag.
    model.lock().unwrap().inner.last_selection = Some(NodeId(2));
    assert_eq!(rings(&mut editor), [false, true]);

    down(&mut editor, Point::new(50.0, 450.0), Modifiers::default());
    to(&mut editor, Point::new(150.0, 380.0));
    assert_eq!(
        rings(&mut editor),
        [true, false],
        "only what the band holds"
    );
    to(&mut editor, Point::new(450.0, 380.0));
    assert_eq!(rings(&mut editor), [true, true]);
    to(&mut editor, Point::new(150.0, 380.0));
    assert_eq!(rings(&mut editor), [true, false], "dragged back off card 2");

    up(&mut editor, Point::new(150.0, 380.0), Modifiers::default());
    assert_eq!(rings(&mut editor), [true, false]);
    assert_eq!(model.lock().unwrap().inner.last_selection, Some(NodeId(1)));
}

#[test]
fn a_band_selection_raises_no_card() {
    let (mut editor, _) = hosted_editor();
    assert_eq!(editor.hosted_card_order(), &[NodeId(1), NodeId(2)]);
    band(
        &mut editor,
        Point::new(50.0, 450.0),
        Point::new(150.0, 380.0),
        Modifiers::default(),
    );
    editor.layout(Size::new(800.0, 600.0));
    assert_eq!(editor.selected_ids(), &set(&[1]));
    assert_eq!(editor.hosted_card_order(), &[NodeId(1), NodeId(2)]);
}
