//! agg-sharp's `Markdig.Agg.Editing` tests (`Tests/Agg.Tests/MarkdigAgg/`),
//! ported 1:1 against `agg_gui::widgets::markdown_editing`, one file per C#
//! test class.

#[path = "markdown_editing/rich_document_tests.rs"]
mod rich_document_tests;
#[path = "markdown_editing/rich_inline_round_trip_tests.rs"]
mod rich_inline_round_trip_tests;
#[path = "markdown_editing/rich_markdown_parser_block_tests.rs"]
mod rich_markdown_parser_block_tests;
#[path = "markdown_editing/rich_markdown_parser_code_table_tests.rs"]
mod rich_markdown_parser_code_table_tests;
#[path = "markdown_editing/rich_markdown_parser_tests.rs"]
mod rich_markdown_parser_tests;
#[path = "markdown_editing/rich_markdown_writer_code_table_tests.rs"]
mod rich_markdown_writer_code_table_tests;
#[path = "markdown_editing/rich_markdown_writer_tests.rs"]
mod rich_markdown_writer_tests;
#[path = "markdown_editing/rust_only_parser_inline_tests.rs"]
mod rust_only_parser_inline_tests;
#[path = "markdown_editing/rust_only_pipe_table_tests.rs"]
mod rust_only_pipe_table_tests;
#[path = "markdown_editing/rust_only_writer_group_tests.rs"]
mod rust_only_writer_group_tests;
