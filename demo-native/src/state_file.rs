//! Saved-state file for the native demo: where it lives, and the read/write
//! pair the host's auto-save tick uses.
//!
//! The blob holds the whole session (open windows, positions, font settings,
//! …) *and* the window bounds, so `agg-gui-shell`'s `WindowBoundsStore` impl
//! only records the bounds and lets this module do the single write — see
//! `host::StateFileBounds`.

const STATE_FILE_NAME: &str = ".agg-gui-demo-state";

pub fn state_file_path() -> std::path::PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(STATE_FILE_NAME)))
        .unwrap_or_else(|| std::path::PathBuf::from(STATE_FILE_NAME))
}

pub fn load_saved_state() -> Option<demo_ui::SavedState> {
    let s = std::fs::read_to_string(state_file_path()).ok()?;
    demo_ui::SavedState::deserialize(&s)
}

/// Build the serialized form of the current state. Substitutes the
/// last-known windowed size when the window is currently maximized or
/// fullscreen, so the next launch doesn't restore a windowed window at the
/// maximized rect.
pub fn serialize_state(
    accessor: &demo_ui::StateAccessor,
    last_windowed: Option<(u32, u32)>,
) -> String {
    let mut state = accessor.current_state();
    if state.window_maximized || state.window_fullscreen {
        if let Some((w, h)) = last_windowed {
            state.window_w = Some(w);
            state.window_h = Some(h);
        }
    }
    state.serialize()
}

pub fn save_state_to_disk(text: &str) {
    let _ = std::fs::write(state_file_path(), text);
}
