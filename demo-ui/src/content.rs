use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{Font, Widget};

use crate::rendering_test;
use crate::windows;

// ── Demo content dispatcher ────────────────────────────────────────────────────

pub(crate) fn build_demo_content(
    title: &str,
    font: Arc<Font>,
    screenshot_request: Rc<Cell<bool>>,
    screenshot_image: Rc<RefCell<Option<(Arc<Vec<u8>>, u32, u32)>>>,
    screenshot_capturing: Rc<Cell<bool>>,
    screenshot_available: Rc<Cell<bool>>,
    screenshot_save_pending: Rc<Cell<bool>>,
    screenshot_copy_pending: Rc<Cell<bool>>,
    screenshot_continuous: Rc<Cell<bool>>,
    screenshot_capture_seq: Rc<Cell<u64>>,
) -> Box<dyn Widget> {
    match title {
        // basic.rs
        "\u{F121} Code Editor" => windows::code_editor(font),
        "\u{F1DE} Sliders" => windows::sliders(font),
        "\u{F040} TextEdit" => windows::text_edit(font),
        "\u{F086} Tooltips" => windows::tooltips(font),
        // code_example.rs
        "\u{F1C9} Code Example" => windows::code_example(font),
        // gallery.rs
        "\u{F009} Widget Gallery" => windows::widget_gallery(font),
        // animation.rs
        "\u{F1FE} Bézier Curve" => windows::bezier_curve(font),
        "\u{F001} Dancing Strings" => windows::dancing_strings(font),
        "\u{F1FC} Painting" => windows::painting(font),
        // frame_demo.rs
        "\u{F096} Frame" => windows::frame_demo(font),
        // lion.rs — halo-AA pipeline proof
        "\u{F1B0} Lion" => windows::lion_demo(font),
        // misc.rs
        "\u{F108} Extra Viewport" => windows::extra_viewport(font),
        "\u{F0D0} Highlighting" => windows::highlighting(font),
        "\u{F1B2} Interactive Container" => windows::interactive_container(font),
        "\u{F031} Font Book" => windows::font_book(font),
        "\u{F03A} Misc Demos" => windows::misc_demos(font),
        // interaction.rs
        "\u{F0B2} Drag and Drop" => windows::drag_and_drop(font),
        "\u{F07D} Scrolling" => windows::scrolling_demo(font),
        "\u{F0DB} Panels" => windows::panels_demo(font),
        "\u{F075} Popups" => windows::popups_demo(font),
        "\u{F0C9} Menus" => windows::menu_demo(font),
        "\u{F0C3} Rendering Test" => rendering_test::rendering_test_view(font),
        "\u{F013} System" => windows::system_view(font),
        "\u{F031} LCD Subpixel" => windows::truetype_lcd_view(font),
        "\u{F002} Scene" => windows::scene_demo(font),
        "\u{F030} Screenshot" => windows::screenshot_demo(
            font,
            screenshot_request,
            screenshot_image,
            screenshot_capturing,
            screenshot_available,
            screenshot_save_pending,
            screenshot_copy_pending,
            screenshot_continuous,
            screenshot_capture_seq,
        ),
        // text_demos.rs
        "\u{F0C9} Strip" => windows::strip_demo(font),
        "\u{F0CE} Table" => windows::table_demo(font),
        "\u{F036} Text Layout" => windows::text_layout(font),
        "\u{F0E2} Undo Redo" => windows::undo_redo(font),
        "\u{F013} Window Options" => windows::window_options(font),
        "\u{F2D0} Modals" => windows::modals_demo(font),
        "\u{F0A4} Multi Touch" => windows::multi_touch(font),
        // 3D Animation title is matched in the caller; fallthrough here is fine.
        _ => windows::coming_soon(),
    }
}
