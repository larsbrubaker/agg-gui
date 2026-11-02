//! The zip provider (`agg_gui_files::zip_folder`, the `zip` feature): an archive listed one
//! directory level at a time, its entries' bytes and IDs, its folders' location keys, and the
//! rows a missing or unreadable archive lists. Each test writes its archive into a fresh
//! temporary folder.
#![cfg(feature = "zip")]

#[path = "zip/fixture.rs"]
mod fixture;
#[path = "zip/listing.rs"]
mod listing;
#[path = "zip/places.rs"]
mod places;
