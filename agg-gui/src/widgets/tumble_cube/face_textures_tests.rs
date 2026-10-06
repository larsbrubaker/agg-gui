//! Ports of MatterCAD's `TumbleCubeFaceTextureTests` (5 tests), plus the
//! per-instance hover behaviour of `TumbleCubeControl`.
//!
//! Every design tab builds a tumble cube and every one wants the same six
//! labelled faces, so the images are shared, keyed by the colours that
//! produced them.  The C# runs these `NotInParallel`; here the cache is
//! thread-local, so each test thread has its own and they cannot race.

use std::sync::Arc;

use super::*;
use crate::color::Color;

fn blue() -> Color {
    Color::rgb(0.0, 0.0, 1.0)
}
fn white() -> Color {
    Color::rgb(1.0, 1.0, 1.0)
}
fn gray() -> Color {
    Color::rgb(0.5, 0.5, 0.5)
}

#[test]
fn same_colors_and_face_share_one_image() {
    let first = get_face_image(blue(), white(), gray(), "Top");
    let second = get_face_image(blue(), white(), gray(), "Top");
    assert!(
        Arc::ptr_eq(&first, &second),
        "the second tab must not re-rasterize a label the first tab already drew"
    );
}

#[test]
fn different_colors_or_face_get_their_own_image() {
    let baseline = get_face_image(blue(), white(), gray(), "Front");
    let red = Color::rgb(1.0, 0.0, 0.0);
    let black = Color::rgb(0.0, 0.0, 0.0);
    let green = Color::rgb(0.0, 1.0, 0.0);
    assert!(!Arc::ptr_eq(
        &get_face_image(red, white(), gray(), "Front"),
        &baseline
    ));
    assert!(!Arc::ptr_eq(
        &get_face_image(blue(), black, gray(), "Front"),
        &baseline
    ));
    assert!(!Arc::ptr_eq(
        &get_face_image(blue(), white(), green, "Front"),
        &baseline
    ));
    assert!(
        !Arc::ptr_eq(&get_face_image(blue(), white(), gray(), "Back"), &baseline),
        "a theme change (or a different label) has to miss the cache rather than reuse a stale image"
    );
}

// The shared image is only safe because the hover highlight draws into a
// copy. If a caller ever highlighted the cached image itself, every other
// tab's cube would come up permanently lit.
#[test]
fn drawing_over_a_copy_leaves_the_shared_source_intact() {
    let source = get_face_image(blue(), white(), gray(), "Left");
    let mid = FACE_SIZE / 2;
    let before = face_pixel(&source, mid, mid);

    // Exactly what `CubeFaces::highlight` does to build its `active` image.
    let mut active = (*source).clone();
    for px in active.chunks_exact_mut(4) {
        px.copy_from_slice(&[255, 0, 0, 255]);
    }
    assert_eq!(
        face_pixel(&active, mid, mid),
        [255, 0, 0, 255],
        "the copy is what the highlight is drawn into"
    );
    assert_eq!(
        face_pixel(&source, mid, mid),
        before,
        "the copy must not share the cached image's pixel buffer"
    );

    let second = get_face_image(blue(), white(), gray(), "Left");
    assert!(Arc::ptr_eq(&second, &source));
    assert_eq!(
        face_pixel(&second, mid, mid),
        before,
        "the next tab has to get the undamaged image out of the cache"
    );
}

// Face labels are text rastered into a standalone image, so they keep
// whatever LCD path was in force when they were drawn. Without the epoch
// check the cache would serve labels that never picked up a toggle.
#[test]
fn an_lcd_setting_change_drops_the_cached_faces() {
    let original = crate::font_settings::lcd_enabled();
    let before = get_face_image(blue(), white(), gray(), "Right");
    // The production epoch bump: any typography setting change advances it.
    crate::font_settings::set_lcd_enabled(!original);
    let after = get_face_image(blue(), white(), gray(), "Right");
    crate::font_settings::clear_lcd_enabled_override();
    assert!(
        !Arc::ptr_eq(&after, &before),
        "the cached label was rastered through the old LCD path"
    );
}

#[test]
fn face_image_is_labelled_and_bordered() {
    let background = blue();
    let face = get_face_image(background, white(), Color::rgb(1.0, 0.0, 0.0), "Bottom");
    assert_eq!(face.len(), (FACE_SIZE * FACE_SIZE * 4) as usize);

    // The bug this guards is a face image that is just the background.
    let bg = [0, 0, 255, 255];
    let mut text_pixels = 0;
    for y in FACE_SIZE / 2 - 20..FACE_SIZE / 2 + 20 {
        for x in 0..FACE_SIZE {
            if face_pixel(&face, x, y) != bg {
                text_pixels += 1;
            }
        }
    }
    assert!(
        text_pixels > 0,
        "the label text has to be rasterized into the face image"
    );
    assert_ne!(
        face_pixel(&face, 3, 3),
        bg,
        "the face is outlined with the theme grid line color"
    );
}

#[test]
fn rust_only_hover_highlights_named_tiles_and_reset_restores() {
    let mut faces = CubeFaces::new(TumbleCubeStyle::default());
    let overlay = Color::rgba(1.0, 0.0, 0.0, 0.5);
    // Top/front edge: Top tile 1 (bottom middle), Front tile 7 (top middle).
    let hit = HitData::new(0, 1, &[(5, 7)]);
    assert!(faces.highlight(hit, overlay));
    assert!(
        !faces.highlight(hit, overlay),
        "the same hit must not redraw"
    );
    let top = &faces.faces[0];
    assert!(top.changed && !Arc::ptr_eq(&top.active, &top.source));
    // Tile 1 spans x 64..192, y 0..64 (Y up); tile 4 is untouched.
    assert_ne!(
        face_pixel(&top.active, 128, 10),
        face_pixel(&top.source, 128, 10)
    );
    assert_eq!(
        face_pixel(&top.active, 128, 128),
        face_pixel(&top.source, 128, 128)
    );
    let front = &faces.faces[5];
    assert_ne!(
        face_pixel(&front.active, 128, 250),
        face_pixel(&front.source, 128, 250)
    );
    assert!(!faces.faces[1].changed);

    assert!(faces.reset());
    assert!(faces
        .faces
        .iter()
        .all(|f| !f.changed && Arc::ptr_eq(&f.active, &f.source)));
    assert!(!faces.reset(), "nothing left to reset");
}

#[test]
fn rust_only_tile_rects_partition_the_face() {
    let mut area = 0;
    for t in 0..9 {
        let (l, b, r, tp) = tile_rect(t).unwrap();
        area += (r - l) * (tp - b);
    }
    assert_eq!(area, FACE_SIZE * FACE_SIZE);
    assert_eq!(tile_rect(9), None);
}
