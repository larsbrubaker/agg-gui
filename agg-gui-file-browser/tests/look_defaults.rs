//! The look a browser has when its app gives none: agg-gui's theme colours, every icon drawn as
//! a Font Awesome glyph the bundled face has, and the long-standing widget names automation and
//! help articles find the browser's buttons by.

use std::collections::BTreeSet;

use agg_gui::theme::{current_visuals, Visuals};
use agg_gui::Size;
use agg_gui_file_browser::{
    glyph_face, BrowserIcon, BrowserLook, BrowserNames, BrowserStyle, DefaultLook,
};

#[test]
fn the_default_names_are_the_long_standing_ones() {
    let names = BrowserNames::default();
    assert_eq!(names.bread_crumb, "FolderBreadCrumbWidget");
    assert_eq!(names.up_button, "Library Up Button");
    assert_eq!(
        names.bread_crumb_button_for("Home"),
        "Bread Crumb Button Home"
    );
    assert_eq!(names.rows, "Library ListView");
    assert_eq!(names.header_markdown, "Library Header Markdown");
    assert_eq!(names.view_options_button, "Library View Options Button");
    assert_eq!(names.sort_button, "Print Library View Options");
    assert_eq!(names.undo_button, "Library Undo Button");
    assert_eq!(names.redo_button, "Library Redo Button");
    assert_eq!(names.context_menu, "Library Context Menu");
    assert_eq!(names.overflow_menu, "Print Library Overflow Menu");
    assert_eq!(names.item_row_for("Bracket"), "Row Item Bracket");
    assert_eq!(
        names.folder_row_for("Downloads"),
        "Downloads Row Item Collection"
    );
}

#[test]
fn a_look_names_its_widgets_its_own_way() {
    let names = BrowserNames {
        up_button: "Up".into(),
        bread_crumb_button: "Crumb: {name}".into(),
        ..BrowserNames::default()
    };
    let look = DefaultLook::with_names(names);
    assert_eq!(look.names().up_button, "Up");
    assert_eq!(look.names().bread_crumb_button_for("Home"), "Crumb: Home");
}

#[test]
fn every_default_icon_is_a_glyph_the_bundled_face_has() {
    let face = glyph_face();
    for &icon in BrowserIcon::ALL {
        assert!(
            face.has_glyph(icon.glyph()),
            "{icon:?} U+{:04X}",
            icon.glyph() as u32
        );
    }
    let keys: BTreeSet<&str> = BrowserIcon::ALL.iter().map(|i| i.key()).collect();
    assert_eq!(
        keys.len(),
        BrowserIcon::ALL.len(),
        "each icon has its own name"
    );
    let glyphs: BTreeSet<char> = BrowserIcon::ALL.iter().map(|i| i.glyph()).collect();
    assert_eq!(
        glyphs.len(),
        BrowserIcon::ALL.len(),
        "each icon has its own glyph"
    );
}

#[test]
fn the_default_look_draws_glyphs_and_names_icons_by_their_key() {
    let look = DefaultLook::default();
    for &icon in BrowserIcon::ALL {
        assert!(look.icon(icon, Size::new(16.0, 16.0), true).is_none());
        assert_eq!(look.icon_name(icon), icon.key());
    }
}

#[test]
fn the_default_style_is_the_current_themes() {
    let visuals = current_visuals();
    let style = DefaultLook::default().style();
    assert_eq!(style, BrowserStyle::from_visuals(&visuals));
    assert_eq!(style, BrowserStyle::default());
    assert_eq!(style.text_color, visuals.text_color);
    assert_eq!(style.hover_fill, visuals.widget_bg_hovered);
    assert_eq!(style.accent, visuals.accent);
    assert!(style.font_size > 0.0 && style.button_height > 0.0);
}

#[test]
fn a_light_and_a_dark_theme_give_their_own_text_colour() {
    let light = BrowserStyle::from_visuals(&Visuals::light());
    let dark = BrowserStyle::from_visuals(&Visuals::dark());
    assert_eq!(light.text_color, Visuals::light().text_color);
    assert_eq!(dark.text_color, Visuals::dark().text_color);
    assert_ne!(light.text_color, dark.text_color);
}

#[test]
fn the_default_look_draws_the_folder_key_as_a_folder_and_nothing_else() {
    let look = agg_gui_file_browser::DefaultLook::default();
    let side = Size::new(20.0, 20.0);
    let tint = agg_gui::Color::rgba(0.2, 0.4, 0.8, 1.0);
    let folder = agg_gui_file_browser::BrowserLook::icon_for_key(&look, "folder", side, tint);
    assert!(folder.is_some(), "a folder tree's rows look like folders");
    let other = agg_gui_file_browser::BrowserLook::icon_for_key(&look, "cube.svg", side, tint);
    assert!(other.is_none());
}
