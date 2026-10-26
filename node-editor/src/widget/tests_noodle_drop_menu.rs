//! A new noodle let go on empty canvas opens the host's add menu
//! (`NodeEditor::with_add_menu`) at the release, naming the socket it came
//! from in `AddMenuRequest::from_socket`; drops on sockets and cards, picked-up
//! noodles, drops beside a ringed socket and editors with no host menu keep
//! their own behaviour. See `connect::finish_connection` and `add_menu`.

use std::cell::RefCell;

use super::add_menu::AddMenuRequest;
use super::tests_common::{fixture_with_typed_handle, mk_node, seed_nodes, Memory};
use super::*;
use crate::connection::SocketRef;
use crate::model::{NodeView, NoodleView, SocketView};
use agg_gui::{Key, Modifiers, MouseButton, Point};

/// Open canvas, away from every card.
const EMPTY: Point = Point { x: 300.0, y: 100.0 };

fn node(id: u64, pos: [f64; 2], inputs: &[&str], outputs: &[&str]) -> NodeView {
    let socket = |name: &&str| SocketView {
        name: (*name).into(),
        socket_type: SocketTypeId(0),
        display_label: None,
    };
    let mut n = mk_node(id, &format!("N{id}"), pos);
    n.inputs = inputs.iter().map(socket).collect();
    n.outputs = outputs.iter().map(socket).collect();
    n
}

/// Source 1 (`out`) on the left, target 2 (`a`, `b`) on the right, no
/// noodles, canvas space shown 1:1.
fn fixture(deferred: bool) -> (NodeEditor, Arc<Mutex<Memory>>) {
    let (model, memory) = fixture_with_typed_handle();
    let mut editor = NodeEditor::new(model).with_deferred_noodle_pickup(deferred);
    editor.set_bounds(Rect::new(0.0, 0.0, 800.0, 600.0));
    seed_nodes(
        &mut editor,
        &memory,
        vec![
            node(1, [20.0, 500.0], &[], &["out"]),
            node(2, [400.0, 500.0], &["a", "b"], &[]),
        ],
    );
    editor.layout(Size::new(800.0, 600.0));
    (editor, memory)
}

/// A fixed-size stand-in for a host's add-menu widget.
#[derive(Default)]
struct MenuBody {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl Widget for MenuBody {
    fn type_name(&self) -> &'static str {
        "MenuBody"
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
        Size::new(100.0, 80.0)
    }
    fn paint(&mut self, _ctx: &mut dyn agg_gui::DrawCtx) {}
    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Consumed
    }
}

type Requests = Rc<RefCell<Vec<AddMenuRequest>>>;

/// [`fixture`] with a host add menu whose requests land in the list.
fn with_menu(deferred: bool) -> (NodeEditor, Arc<Mutex<Memory>>, Requests) {
    let (editor, memory) = fixture(deferred);
    let requests: Requests = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&requests);
    let editor = editor.with_add_menu(move |request| {
        sink.borrow_mut().push(request);
        Some(Box::new(MenuBody::default()) as Box<dyn Widget>)
    });
    (editor, memory, requests)
}

/// Where `socket` on the `side` of `node` is drawn.
fn at(editor: &NodeEditor, node: u64, side: SocketSide, socket: &str) -> Point {
    let layouts = editor.snapshot_layouts();
    let s = layouts
        .iter()
        .find(|l| l.node_id == NodeId(node))
        .and_then(|l| l.sockets().find(|s| s.side == side && s.name == socket))
        .expect("socket is laid out");
    Point::new(s.center[0], s.center[1])
}

fn mouse(editor: &mut NodeEditor, event: Event) {
    editor.on_event(&event);
}

/// Press at `from`, move to `to` and let go there.
fn drag(editor: &mut NodeEditor, from: Point, to: Point) {
    let modifiers = Modifiers::default();
    let button = MouseButton::Left;
    mouse(
        editor,
        Event::MouseDown {
            pos: from,
            button,
            modifiers,
        },
    );
    mouse(editor, Event::MouseMove { pos: to });
    mouse(
        editor,
        Event::MouseUp {
            pos: to,
            button,
            modifiers,
        },
    );
}

fn connect_1_to_2a(editor: &mut NodeEditor) {
    let (from, to) = (
        at(editor, 1, SocketSide::Output, "out"),
        at(editor, 2, SocketSide::Input, "a"),
    );
    drag(editor, from, to);
}

fn noodle_1_to_2a() -> NoodleView {
    NoodleView {
        from_node: NodeId(1),
        from_socket: "out".into(),
        to_node: NodeId(2),
        to_socket: "a".into(),
    }
}

fn socket_ref(node: u64, side: SocketSide, socket: &str) -> SocketRef {
    SocketRef {
        node: NodeId(node),
        side,
        socket: socket.into(),
        socket_type: SocketTypeId(0),
    }
}

#[test]
fn a_noodle_let_go_on_empty_canvas_opens_the_add_menu_naming_its_socket() {
    let (mut editor, memory, requests) = with_menu(false);
    let from = at(&editor, 1, SocketSide::Output, "out");
    drag(&mut editor, from, EMPTY);
    assert_eq!(requests.borrow().len(), 1, "the builder is asked once");
    let request = requests.borrow()[0].clone();
    assert_eq!(
        request.from_socket,
        Some(socket_ref(1, SocketSide::Output, "out"))
    );
    assert_eq!(request.canvas_pos, [EMPTY.x, EMPTY.y], "at the release");
    assert_eq!(request.local_pos, EMPTY);
    assert!(editor.is_add_menu_open());
    assert!(memory.lock().unwrap().noodles.is_empty());
    assert!(
        matches!(editor.interaction, CanvasState::Idle),
        "the in-flight noodle is gone"
    );
}

#[test]
fn a_noodle_dragged_back_from_an_input_names_the_input() {
    let (mut editor, _memory, requests) = with_menu(false);
    let from = at(&editor, 2, SocketSide::Input, "b");
    drag(&mut editor, from, EMPTY);
    assert_eq!(
        requests.borrow()[0].from_socket,
        Some(socket_ref(2, SocketSide::Input, "b"))
    );
}

#[test]
fn shift_a_and_a_right_click_name_no_socket() {
    let (mut editor, _memory, requests) = with_menu(false);
    mouse(&mut editor, Event::MouseMove { pos: EMPTY });
    editor.on_event(&Event::KeyDown {
        key: Key::Char('A'),
        modifiers: Modifiers {
            shift: true,
            ..Modifiers::default()
        },
    });
    editor.close_add_menu();
    mouse(
        &mut editor,
        Event::MouseDown {
            pos: EMPTY,
            button: MouseButton::Right,
            modifiers: Modifiers::default(),
        },
    );
    let requests = requests.borrow();
    assert_eq!(requests.len(), 2);
    assert!(requests.iter().all(|r| r.from_socket.is_none()));
}

#[test]
fn closing_the_menu_without_a_pick_leaves_the_graph_unchanged() {
    let (mut editor, memory, _requests) = with_menu(false);
    let from = at(&editor, 1, SocketSide::Output, "out");
    drag(&mut editor, from, EMPTY);
    assert!(editor
        .on_unconsumed_key(&Key::Escape, Modifiers::default())
        .is_consumed());
    assert!(!editor.is_add_menu_open());
    let m = memory.lock().unwrap();
    assert!(m.noodles.is_empty() && m.moves.is_empty() && m.nodes.len() == 2);
    assert_eq!(m.escapes, 0, "Escape closed the menu, not the host's");
}

#[test]
fn with_no_host_menu_a_drop_on_empty_canvas_does_nothing() {
    let (mut editor, memory) = fixture(false);
    let from = at(&editor, 1, SocketSide::Output, "out");
    drag(&mut editor, from, EMPTY);
    assert!(!editor.is_add_menu_open());
    assert!(editor.open_context_menu().is_none());
    assert!(memory.lock().unwrap().noodles.is_empty());
}

#[test]
fn a_drop_on_a_socket_still_connects_and_opens_no_menu() {
    let (mut editor, memory, requests) = with_menu(false);
    connect_1_to_2a(&mut editor);
    let noodles = memory.lock().unwrap().noodles.clone();
    let expected = noodle_1_to_2a();
    assert_eq!(noodles.len(), 1);
    assert_eq!(
        (&noodles[0].from_node, &noodles[0].from_socket),
        (&expected.from_node, &expected.from_socket)
    );
    assert_eq!(
        (&noodles[0].to_node, &noodles[0].to_socket),
        (&expected.to_node, &expected.to_socket)
    );
    assert!(requests.borrow().is_empty());
    assert!(!editor.is_add_menu_open());
}

#[test]
fn a_refused_drop_on_a_card_opens_no_menu() {
    // The model makes no auto-pick, so the card refuses the noodle.
    let (mut editor, memory, requests) = with_menu(false);
    let from = at(&editor, 1, SocketSide::Output, "out");
    let layouts = editor.snapshot_layouts();
    let l = layouts.iter().find(|l| l.node_id == NodeId(2)).unwrap();
    let body = Point::new(
        l.top_left[0] + l.size[0] / 2.0,
        l.top_left[1] - l.size[1] / 2.0,
    );
    drag(&mut editor, from, body);
    assert!(requests.borrow().is_empty());
    assert!(memory.lock().unwrap().noodles.is_empty());
}

#[test]
fn a_drop_beside_a_ringed_socket_opens_no_menu() {
    // 12 px left of input `a`: off its hit area, inside the snap ring.
    let (mut editor, _memory, requests) = with_menu(false);
    let from = at(&editor, 1, SocketSide::Output, "out");
    let a = at(&editor, 2, SocketSide::Input, "a");
    drag(&mut editor, from, Point::new(a.x - 12.0, a.y));
    assert!(requests.borrow().is_empty());
    assert!(!editor.is_add_menu_open());
}

#[test]
fn a_noodle_picked_up_and_let_go_on_empty_canvas_disconnects_without_a_menu() {
    for deferred in [false, true] {
        let (mut editor, memory, requests) = with_menu(deferred);
        connect_1_to_2a(&mut editor);
        let a = at(&editor, 2, SocketSide::Input, "a");
        drag(&mut editor, a, EMPTY);
        assert!(requests.borrow().is_empty(), "deferred: {deferred}");
        assert!(!editor.is_add_menu_open(), "deferred: {deferred}");
        assert!(
            memory.lock().unwrap().noodles.is_empty(),
            "deferred: {deferred}"
        );
    }
}
