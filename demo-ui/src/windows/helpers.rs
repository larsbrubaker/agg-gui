//! Shared helpers for the demo window builders.
//!
//! Provides [`source_link`], the demo-ui analog of egui's
//! `egui_github_link_file!` macro: a small right-aligned "(source code)"
//! hyperlink that points at the file implementing a given demo on GitHub.
//! Every demo window that mirrors an egui demo pushes one as its footer so
//! developers can jump straight to the implementation, exactly like the
//! original egui demos do.

use std::sync::Arc;

use agg_gui::{Font, HAnchor, Hyperlink, Widget};

/// Base URL for demo window source files in the agg-gui repository.
const SOURCE_BASE_URL: &str =
    "https://github.com/larsbrubaker/agg-gui/blob/main/demo-ui/src/windows";

/// Build a right-aligned "(source code)" hyperlink pointing at
/// `demo-ui/src/windows/<file>`.
///
/// `file` is the path *relative to* `demo-ui/src/windows`, e.g. `"basic.rs"`
/// or `"text_demos/table_demo.rs"`. Callers push the returned widget as the
/// final child of a demo's root column to reproduce egui's footer link.
pub fn source_link(file: &str, font: Arc<Font>) -> Box<dyn Widget> {
    let url = format!("{SOURCE_BASE_URL}/{file}");
    Box::new(
        Hyperlink::new("(source code)", font)
            .with_font_size(11.0)
            .with_h_anchor(HAnchor::RIGHT)
            .on_click(move || crate::url::open_url(&url)),
    )
}
