//! Tests for [`TextField`]'s runtime password-reveal affordance.
//!
//! A password field masks its characters unless a bound "reveal" cell forces
//! plaintext. This exercises `masking_active` against the public builders that
//! back the demo's eye-toggle button, using real production code paths.

use super::*;
use crate::text::Font;
use crate::widgets::TextField;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(TEST_FONT).unwrap())
}

#[test]
fn non_password_field_never_masks() {
    let tf = TextField::new(font()).with_text("hello");
    assert!(!tf.masking_active());
}

#[test]
fn password_field_masks_without_reveal_cell() {
    let tf = TextField::new(font()).with_password_mode(true).with_text("secret");
    assert!(tf.masking_active());
}

#[test]
fn reveal_cell_toggles_masking_live() {
    let reveal = Rc::new(Cell::new(false));
    let tf = TextField::new(font())
        .with_password_mode(true)
        .with_password_reveal_cell(Rc::clone(&reveal))
        .with_text("secret");
    assert!(tf.masking_active(), "masked while reveal is false");
    reveal.set(true);
    assert!(!tf.masking_active(), "plaintext while reveal is true");
    reveal.set(false);
    assert!(tf.masking_active(), "masked again after reveal cleared");
}

#[test]
fn reveal_cell_is_inert_without_password_mode() {
    let reveal = Rc::new(Cell::new(true));
    let tf = TextField::new(font()).with_password_reveal_cell(Rc::clone(&reveal));
    assert!(!tf.masking_active());
}
