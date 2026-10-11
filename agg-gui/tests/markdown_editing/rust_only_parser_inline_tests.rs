//! Rust-only tests for the reader's scan of the viewer's extensions
//! (`markdown_editing::parser_inlines`).

use agg_gui::widgets::markdown_editing::markdown_parser::parse;
use agg_gui::widgets::markdown_editing::{markdown_writer, InlineAtomKind};

#[test]
fn rust_only_text_with_multibyte_characters_scans_for_bare_links() {
    // The link-prefix check once sliced through the 'é' of "wwé" and panicked.
    let source = "wwé café www.x.com";
    let document = parse(source);
    let atoms: Vec<&str> = document.blocks[0]
        .inlines
        .iter()
        .filter_map(|inline| inline.as_atom())
        .filter(|atom| atom.kind == InlineAtomKind::Autolink)
        .map(|atom| atom.raw_markdown.as_str())
        .collect();
    assert_eq!(atoms, ["www.x.com"]);
    assert_eq!(markdown_writer::write(&document), source);
}
