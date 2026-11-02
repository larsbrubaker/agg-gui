//! Searching from a browser view: the [`SearchController`] (typing debounced into a
//! `SearchResultsFolder` the browser shows below the folder searched, cleared or abandoned back to
//! it) and the [`SearchBox`] that drives it. The results list in sections as they arrive through
//! the state (`state/search.rs`, `search_listing`) and the list's section headers
//! (`list/section_header.rs`).

pub mod controller;
pub mod search_box;

pub use controller::{SearchController, SearchEndedListener, SearchMaker, DEBOUNCE};
pub use search_box::SearchBox;
