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
    // The hint an empty field shows: a shared placeholder cell can change it
    // with nothing else in the field changing.
    pub(super) placeholder: String,
    // The input-method composition shown inline (text and caret range).
    pub(super) preedit: Option<crate::ime::Preedit>,
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
    // (e.g. the user hovers the control, which flips `hovered`). The font is
    // recognised by `Font::id`, never its address: a freed font's address
    // can be handed to the next font, which would keep the stale bitmap.
    pub(super) font_id: u64,
    pub(super) font_size_bits: u64,
    // Whether characters are masked. Toggling the reveal cell keeps `text`
    // unchanged (the real text), so the cache would otherwise keep blitting the
    // stale masked/plaintext bitmap until another sig field happened to change.
    pub(super) masking: bool,
    // The effective text insets (left, right, top, bottom) as bits: the text
    // clip, caret and text origin all derive from them, so a live
    // `set_text_insets` must drop the cached bitmap.
    pub(super) insets_bits: [u64; 4],
    // Whether the background and border are painted (`with_frame`).
    pub(super) frame: bool,
    // Whether the field is enabled: a disabled field draws its hint faded.
    pub(super) enabled: bool,
}
