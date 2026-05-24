//! Reflection-driven property row vocabulary.
//!
//! This module owns the shared schema types that describe how a single
//! editable property should render in a property panel — what widget
//! to mount, its numeric range / step / decimal-precision, its label,
//! its description, and whether it belongs to an "advanced" section.
//!
//! Living here (in `agg-gui`, not in any host crate) is deliberate. The
//! schema is **widget vocabulary** — what the panel renderer can do —
//! not a host-side concept tied to a particular value system. Host
//! crates (atomartist, MatterCAD-rust) feed their reflected property
//! structs into this vocabulary and the row factory mounts the right
//! widget without per-host code.
//!
//! Modeled after MatterCAD's `PropertyEditor` + per-type
//! `IPropertyEditorFactory` pair. The host walks a `#[derive(Reflect)]`
//! struct, looks up each field's [`EditorKind`], and emits one row per
//! field. The "is this row advanced?" decision is data-driven via
//! [`NodeFieldAttrs::advanced`] rather than per-node show/hide code.
//!
//! ## Phased migration
//!
//! Phase 1 (this commit): vocabulary types only. atomartist-lib
//! re-exports these from `atomartist_lib::registry` so downstream
//! callers keep building without churn.
//!
//! Phase 2 (next): factory function `build_row(spec, value, callback)`
//! mounting the actual widget per [`EditorKind`] variant.
//!
//! Phase 3 (then): full `PropertyPanel` widget that takes a list of
//! field specs + a value getter/setter and renders the entire panel,
//! including section headers, advanced gating, and tooltips.

mod editor;

pub use editor::{EditorKind, NodeFieldAttrs, NumberAttrs, VisibleWhen};
