//! Tests for [`super::TumbleCube`] driven through `Widget::on_event` with a
//! recording camera.  Adapted from AtomArtist's `tumble_cube/widget_tests.rs`:
//! its drag tests asserted AtomArtist's own orbit math, which now lives in
//! the host camera, so here they assert the trait calls the C# makes
//! (`Start/Do/EndRotateAroundOrigin`, `AnimateRotation`) instead.

use std::cell::RefCell;
use std::rc::Rc;

use super::*;
use crate::event::Modifiers;
use crate::widgets::tumble_cube::math::{self, Mat3};

#[derive(Debug, Clone, PartialEq)]
enum Call {
    Begin(Point),
    Rotate(Point),
    End,
    Animate(Mat3),
}

struct RecordingCamera {
    rotation: Mat3,
    calls: Vec<Call>,
}

impl TumbleCubeCamera for RecordingCamera {
    fn view_rotation(&self) -> Mat3 {
        self.rotation
    }
    fn begin_rotate(&mut self, pos: Point) {
        self.calls.push(Call::Begin(pos));
    }
    fn rotate(&mut self, pos: Point) {
        self.calls.push(Call::Rotate(pos));
    }
    fn end_rotate(&mut self) {
        self.calls.push(Call::End);
    }
    fn animate_rotation(&mut self, target: Mat3) {
        self.calls.push(Call::Animate(target));
    }
}

/// Main view looking at the Front face (along +Y, Z up).
fn front_view() -> Mat3 {
    math::look_at([0.0, 1.0, 0.0], [0.0, 0.0, 1.0])
}

fn cube_with(rotation: Mat3) -> (TumbleCube, Rc<RefCell<RecordingCamera>>) {
    let cam = Rc::new(RefCell::new(RecordingCamera {
        rotation,
        calls: Vec::new(),
    }));
    let mut cube = TumbleCube::new(cam.clone());
    cube.layout(Size::new(500.0, 500.0));
    (cube, cam)
}

fn down(cube: &mut TumbleCube, x: f64, y: f64, button: MouseButton) -> EventResult {
    cube.on_event(&Event::MouseDown {
        pos: Point::new(x, y),
        button,
        modifiers: Modifiers::default(),
    })
}
fn up(cube: &mut TumbleCube, x: f64, y: f64, button: MouseButton) -> EventResult {
    cube.on_event(&Event::MouseUp {
        pos: Point::new(x, y),
        button,
        modifiers: Modifiers::default(),
    })
}
fn mv(cube: &mut TumbleCube, x: f64, y: f64) -> EventResult {
    cube.on_event(&Event::MouseMove {
        pos: Point::new(x, y),
    })
}

fn rot_close(a: &Mat3, b: &Mat3) -> bool {
    (0..3).all(|r| (0..3).all(|c| (a[r][c] - b[r][c]).abs() < 1e-9))
}

#[test]
fn click_on_front_face_animates_to_front_view() {
    let (mut cube, cam) = cube_with(front_view());
    down(&mut cube, 50.0, 50.0, MouseButton::Left);
    up(&mut cube, 50.0, 50.0, MouseButton::Left);
    let calls = &cam.borrow().calls;
    assert_eq!(calls[0], Call::Begin(Point::new(50.0, 50.0)));
    assert_eq!(calls[1], Call::End);
    let Call::Animate(target) = &calls[2] else {
        panic!("expected an animate, got {calls:?}")
    };
    assert!(rot_close(target, &front_view()), "{target:?}");
}

#[test]
fn click_on_top_front_edge_animates_between_the_faces() {
    // From the front view the Top/Front edge is the band just above the
    // front face's top border — the projected top edge of the cube.
    let (mut cube, cam) = cube_with(front_view());
    let view = cube.view();
    let y = (50..100)
        .rev()
        .map(|y| y as f64)
        .find(|&y| view.hit(50.0, y).is_some())
        .unwrap();
    let hit = view.hit(50.0, y).unwrap();
    assert_eq!(hit.face_index[..2], [5, 0], "{hit:?}");
    down(&mut cube, 50.0, y, MouseButton::Left);
    up(&mut cube, 50.0, y, MouseButton::Left);
    let Call::Animate(target) = cam.borrow().calls[2] else {
        panic!()
    };
    // Looks along (0, 1, -1)/√2: halfway between Front and Top.
    let forward = math::transform([0.0, 1.0, -1.0], &target);
    assert!((forward[2] + 2f64.sqrt()).abs() < 1e-9, "{forward:?}");
}

#[test]
fn drag_forwards_every_move_to_the_camera_and_does_not_animate() {
    let (mut cube, cam) = cube_with(front_view());
    down(&mut cube, 50.0, 50.0, MouseButton::Left);
    mv(&mut cube, 55.0, 45.0);
    mv(&mut cube, 60.0, 40.0);
    up(&mut cube, 60.0, 40.0, MouseButton::Left);
    assert_eq!(
        cam.borrow().calls,
        vec![
            Call::Begin(Point::new(50.0, 50.0)),
            Call::Rotate(Point::new(55.0, 45.0)),
            Call::Rotate(Point::new(60.0, 40.0)),
            Call::End,
        ]
    );
}

#[test]
fn release_away_from_press_is_a_drag_not_a_click() {
    // The C# only orients when the release is at exactly the press point.
    let (mut cube, cam) = cube_with(front_view());
    down(&mut cube, 50.0, 50.0, MouseButton::Left);
    up(&mut cube, 51.0, 50.0, MouseButton::Left);
    assert!(!cam
        .borrow()
        .calls
        .iter()
        .any(|c| matches!(c, Call::Animate(_))));
}

#[test]
fn right_button_rotates_but_never_orients() {
    let (mut cube, cam) = cube_with(front_view());
    down(&mut cube, 50.0, 50.0, MouseButton::Right);
    up(&mut cube, 50.0, 50.0, MouseButton::Right);
    assert_eq!(
        cam.borrow().calls,
        vec![Call::Begin(Point::new(50.0, 50.0)), Call::End]
    );
}

#[test]
fn click_beside_the_cube_does_not_orient() {
    let (mut cube, cam) = cube_with(front_view());
    down(&mut cube, 2.0, 2.0, MouseButton::Left);
    up(&mut cube, 2.0, 2.0, MouseButton::Left);
    assert_eq!(
        cam.borrow().calls,
        vec![Call::Begin(Point::new(2.0, 2.0)), Call::End]
    );
}

/// The cube floats over a 3-D viewport; a release that was not part of a
/// cube gesture must fall through so it can end the viewport's own drag
/// (AtomArtist's "doesn't release the object" regression).
#[test]
fn idle_mouse_up_falls_through_but_gesture_release_is_consumed() {
    let (mut cube, cam) = cube_with(front_view());
    assert_eq!(
        up(&mut cube, 50.0, 50.0, MouseButton::Left),
        EventResult::Ignored
    );
    assert!(cam.borrow().calls.is_empty());
    down(&mut cube, 50.0, 50.0, MouseButton::Left);
    assert_eq!(
        up(&mut cube, 50.0, 50.0, MouseButton::Left),
        EventResult::Consumed
    );
}

#[test]
fn hover_highlights_the_face_and_leaving_clears_it() {
    let (mut cube, cam) = cube_with(front_view());
    assert_eq!(
        mv(&mut cube, 50.0, 50.0),
        EventResult::Ignored,
        "hover must not claim the move"
    );
    assert!(cube.faces().faces[5].changed, "front face centre lit");
    assert_eq!(cube.faces().last_hit(), HitData::new(5, 4, &[]));
    mv(&mut cube, -10.0, 50.0);
    assert!(
        cube.faces().faces.iter().all(|f| !f.changed),
        "leaving the cube clears the highlight"
    );
    assert!(
        cam.borrow().calls.is_empty(),
        "hover never touches the camera"
    );
}

#[test]
fn moves_while_pressed_do_not_hover() {
    let (mut cube, _cam) = cube_with(front_view());
    down(&mut cube, 50.0, 50.0, MouseButton::Left);
    mv(&mut cube, 50.0, 51.0);
    assert!(cube.faces().faces.iter().all(|f| !f.changed));
}

#[test]
fn rust_only_cpu_render_draws_the_lit_front_face() {
    let (cube, _cam) = cube_with(front_view());
    let view = cube.view();
    let px = cpu_render::render(&view, &cube.faces().faces, 100, 100);
    let at = |x: usize, y_down: usize| {
        let i = (y_down * 100 + x) * 4;
        [px[i], px[i + 1], px[i + 2], px[i + 3]]
    };
    assert_eq!(at(1, 1)[3], 0, "corner is outside the cube: transparent");
    // The front face's border sits just inside the cube's projected edge;
    // well inside the face (away from the label) is the background colour,
    // lit by the default rig: 0.2 ambient + 0.7 key light + 0.5 fill
    // light, each at the angle the face's eye-space normal (0, 0, 1) makes.
    let lighting = crate::widgets::tumble_cube::geometry::CubeLighting::default();
    let f = lighting.factor([0.0, 0.0, 1.0]);
    let expected = ((0xf1 as f64 / 255.0 * f).min(1.0) * 255.0).round() as i32;
    let p = at(50, 30);
    assert_eq!(p[3], 255);
    assert!((p[0] as i32 - expected).abs() <= 2, "{p:?} vs {expected}");
}

use crate::widgets::tumble_cube::cpu_render;
use crate::widgets::tumble_cube::hit_test::HitData;
