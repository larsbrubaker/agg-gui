//! agg-sharp's `Markdig.Agg.Editing` tests (`Tests/Agg.Tests/MarkdigAgg/`),
//! ported 1:1 against `agg_gui::widgets::markdown_editing`, one file per C#
//! test class.

#[path = "markdown_editing/markdown_format_toolbar_tests.rs"]
mod markdown_format_toolbar_tests;
#[path = "markdown_editing/rich_block_layout_tests.rs"]
mod rich_block_layout_tests;
#[path = "markdown_editing/rich_block_operations_tests.rs"]
mod rich_block_operations_tests;
#[path = "markdown_editing/rich_document_tests.rs"]
mod rich_document_tests;
#[path = "markdown_editing/rich_edit_history_tests.rs"]
mod rich_edit_history_tests;
#[path = "markdown_editing/rich_edit_operations_tests.rs"]
mod rich_edit_operations_tests;
#[path = "markdown_editing/rich_editor_clipboard_tests.rs"]
mod rich_editor_clipboard_tests;
#[path = "markdown_editing/rich_editor_keyboard_tests.rs"]
mod rich_editor_keyboard_tests;
#[path = "markdown_editing/rich_editor_mouse_tests.rs"]
mod rich_editor_mouse_tests;
#[path = "markdown_editing/rich_editor_polish_tests.rs"]
mod rich_editor_polish_tests;
#[path = "markdown_editing/rich_inline_round_trip_tests.rs"]
mod rich_inline_round_trip_tests;
#[path = "markdown_editing/rich_markdown_edit_widget_tests.rs"]
mod rich_markdown_edit_widget_tests;
#[path = "markdown_editing/rich_markdown_editor_tests.rs"]
mod rich_markdown_editor_tests;
#[path = "markdown_editing/rich_markdown_parser_block_tests.rs"]
mod rich_markdown_parser_block_tests;
#[path = "markdown_editing/rich_markdown_parser_code_table_tests.rs"]
mod rich_markdown_parser_code_table_tests;
#[path = "markdown_editing/rich_markdown_parser_tests.rs"]
mod rich_markdown_parser_tests;
#[path = "markdown_editing/rich_markdown_writer_code_table_tests.rs"]
mod rich_markdown_writer_code_table_tests;
#[path = "markdown_editing/rich_markdown_writer_group_tests.rs"]
mod rich_markdown_writer_group_tests;
#[path = "markdown_editing/rich_markdown_writer_tests.rs"]
mod rich_markdown_writer_tests;
#[path = "markdown_editing/rich_paste_operations_tests.rs"]
mod rich_paste_operations_tests;
#[path = "markdown_editing/rich_style_operations_tests.rs"]
mod rich_style_operations_tests;
#[path = "markdown_editing/rich_table_code_operations_tests.rs"]
mod rich_table_code_operations_tests;
#[path = "markdown_editing/rich_table_grid_pixel_tests.rs"]
mod rich_table_grid_pixel_tests;
#[path = "markdown_editing/rich_table_layout_tests.rs"]
mod rich_table_layout_tests;
#[path = "markdown_editing/rust_only_app_pointer_tests.rs"]
mod rust_only_app_pointer_tests;
#[path = "markdown_editing/rust_only_block_painter_tests.rs"]
mod rust_only_block_painter_tests;
#[path = "markdown_editing/rust_only_code_block_and_commands_tests.rs"]
mod rust_only_code_block_and_commands_tests;
#[path = "markdown_editing/rust_only_device_scale_tests.rs"]
mod rust_only_device_scale_tests;
#[path = "markdown_editing/rust_only_document_view_tests.rs"]
mod rust_only_document_view_tests;
#[path = "markdown_editing/rust_only_edit_error_tests.rs"]
mod rust_only_edit_error_tests;
#[path = "markdown_editing/rust_only_image_loader_tests.rs"]
mod rust_only_image_loader_tests;
#[path = "markdown_editing/rust_only_parser_inline_tests.rs"]
mod rust_only_parser_inline_tests;
#[path = "markdown_editing/rust_only_pipe_table_tests.rs"]
mod rust_only_pipe_table_tests;
#[path = "markdown_editing/rust_only_raw_host_theme_tests.rs"]
mod rust_only_raw_host_theme_tests;
#[path = "markdown_editing/rust_only_rich_editor_auto_scroll_tests.rs"]
mod rust_only_rich_editor_auto_scroll_tests;
#[path = "markdown_editing/rust_only_rich_editor_wheel_tests.rs"]
mod rust_only_rich_editor_wheel_tests;
#[path = "markdown_editing/rust_only_toolbar_look_tests.rs"]
mod rust_only_toolbar_look_tests;
#[path = "markdown_editing/rust_only_writer_group_tests.rs"]
mod rust_only_writer_group_tests;
