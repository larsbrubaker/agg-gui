//! `TextFieldSig` — the backbuffer-cache invalidation signature for
//! [`TextField`](super::TextField).
//!
//! Split out of `text_field.rs` to keep that file under the project's 800-line
//! cap. The signature captures every input that affects the cached bitmap
//! (bg + text + selection + border) so `layout` can drop a stale cache when any
//! of them changes. It deliberately excludes cursor-blink phase, which paints
//! in `paint_overlay` after the cache blit.

/// Snapshot of all state that influences the cached [`TextField`](super::TextField)
/// bitmap. `layout` compares the current sig against the last one and
/// invalidates the backbuffer on any difference.
#[derive(Clone, PartialEq)]
pub(super) struct TextFieldSig {
    pub(super) text: String,
    pub(super) cursor: usize,
    pub(super) anchor: usize,
    pub(super) focused: bool,
    pub(super) hovered: bool,
    pub(super) scroll_x_bits: u64,
    pub(super) w_bits: u64,
    pub(super) h_bits: u64,
    // Font identity + size: the cached bitmap was rasterised with a specific
    // typeface at a specific point size, so any live swap in the System
    // window (which runs through `font_settings::set_system_font` /
    // `set_font_size_scale`) must invalidate — otherwise the stale bitmap
    // keeps blitting until some other field in the sig happens to change
    // (e.g. the user hovers the control, which flips `hovered`).
    pub(super) font_ptr: usize,
    pub(super) font_size_bits: u64,
    // Whether characters are masked. Toggling the reveal cell keeps `text`
    // unchanged (the real text), so the cache would otherwise keep blitting the
    // stale masked/plaintext bitmap until another sig field happened to change.
    pub(super) masking: bool,
}
