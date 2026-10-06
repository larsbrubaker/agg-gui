//! Tumble cube (view cube) — a labelled cube in a corner of a 3-D view
//! that mirrors the camera's orientation; hover highlights a face, edge
//! or corner, a click turns the view to it, and a drag rotates the view.
//!
//! A port of MatterCAD's `TumbleCubeControl` / `TumbleCubeFaceTextures`
//! (the behaviour spec), reusing structure from AtomArtist's earlier Rust
//! port.  The app supplies its camera through [`TumbleCubeCamera`]; see
//! that trait for the matrix convention.
//!
//! * [`hit_test`] — `ConnectedFaces` / `HitData` / `GetHitData`.
//! * [`orient`] — `GetDirectionForFace` → target rotation.
//! * [`view`] — the cube's own camera, rays and matrices.
//! * [`geometry`] — face frames (texture placement) and lighting.
//! * [`face_textures`] — cached labels + per-instance hover copies.
//! * [`cpu_render`] — software renderer (fallback / headless).
//! * [`widget`] — the [`TumbleCube`] widget and the GPU renderer hook.
//! * [`math`] — the small row-vector matrix kit everything uses.

pub mod camera;
pub mod cpu_render;
pub mod face_textures;
pub mod geometry;
pub mod hit_test;
pub mod math;
pub mod orient;
pub mod view;
pub mod widget;

pub use camera::TumbleCubeCamera;
pub use face_textures::{get_face_image, TumbleCubeStyle, FACE_SIZE};
pub use hit_test::{get_hit_data, HitData, FACE_NAMES};
pub use view::CubeView;
pub use widget::{TumbleCube, TumbleCubeFrame, TumbleCubeGpuRenderer, TUMBLE_CUBE_SIZE};
