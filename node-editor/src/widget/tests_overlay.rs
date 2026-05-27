//! Overlay-sink hand-off tests for `NodeEditor` — extracted from
//! `tests.rs` to keep that file under the project's 800-line cap.
//!
//! The sink is the channel that lets app shells (today: AtomArtist's
//! `build_app`) reparent the color-picker dialog from this editor's
//! pane up to a screen-level host so the user can drag it anywhere.
//! The branch in `open_color_picker` is critical: with a sink installed
//! the editor MUST NOT keep the dialog as `self.overlay`, otherwise the
//! dialog would render twice (once here, once at the screen-level host)
//! and double-handle every event.

use super::tests_common::{fixture_with_typed_handle, install_test_font_once, mk_node, seed_nodes};
use super::*;

/// Sink branch: with an overlay sink installed, opening the color
/// picker hands the dialog off via the callback and leaves
/// `self.overlay` unset.
#[test]
fn open_color_picker_hands_off_to_sink_when_installed() {
    use std::cell::RefCell;
    install_test_font_once();
    let (model, memory) = fixture_with_typed_handle();
    let captured: Rc<RefCell<Option<(Box<dyn Widget>, Rc<Cell<bool>>)>>> =
        Rc::new(RefCell::new(None));
    let sink_captured = Rc::clone(&captured);
    let mut editor = NodeEditor::new(model).with_overlay_sink(move |dialog, flag| {
        *sink_captured.borrow_mut() = Some((dialog, flag));
    });
    editor.set_bounds(Rect::new(0.0, 0.0, 400.0, 300.0));
    seed_nodes(&mut editor, &memory, vec![mk_node(1, "N", [0.0, 0.0])]);

    editor.open_color_picker(NodeId(1), "Color".to_string(), [0.4, 0.6, 0.8, 1.0]);

    assert!(
        captured.borrow().is_some(),
        "sink must receive the constructed dialog when installed"
    );
    assert!(
        editor.overlay.is_none(),
        "with a sink installed the editor must NOT keep the dialog locally — that would paint it twice"
    );
    assert!(
        editor.overlay_close_flag.is_none(),
        "with a sink installed the close flag belongs to the host, not the editor"
    );
}

/// Back-compat branch: without a sink the editor still owns the
/// dialog as before (the gallery demo + standalone embedders rely on
/// this).
#[test]
fn open_color_picker_uses_local_overlay_when_no_sink() {
    install_test_font_once();
    let (model, memory) = fixture_with_typed_handle();
    let mut editor = NodeEditor::new(model);
    editor.set_bounds(Rect::new(0.0, 0.0, 400.0, 300.0));
    seed_nodes(&mut editor, &memory, vec![mk_node(1, "N", [0.0, 0.0])]);

    editor.open_color_picker(NodeId(1), "Color".to_string(), [0.4, 0.6, 0.8, 1.0]);

    assert!(
        editor.overlay.is_some(),
        "without a sink the editor MUST keep the dialog as its own overlay (legacy behaviour)"
    );
    assert!(
        editor.overlay_close_flag.is_some(),
        "without a sink the close flag is tracked locally so `drain_overlay_close` can tear the dialog down"
    );
}
