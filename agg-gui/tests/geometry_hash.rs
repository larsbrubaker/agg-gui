//! `Hash` for the value types `Point` and `Insets`: values that compare equal
//! hash equal (including `-0.0` against `0.0`), and different values hash
//! differently in practice, so both can key hash maps and sets.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use agg_gui::geometry::Point;
use agg_gui::layout_props::Insets;

fn hash_of<T: Hash>(value: &T) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

#[test]
fn equal_points_hash_equal() {
    assert_eq!(
        hash_of(&Point::new(1.5, -2.0)),
        hash_of(&Point::new(1.5, -2.0))
    );
    assert_eq!(Point::new(-0.0, 0.0), Point::new(0.0, -0.0));
    assert_eq!(
        hash_of(&Point::new(-0.0, 0.0)),
        hash_of(&Point::new(0.0, -0.0))
    );
}

#[test]
fn different_points_hash_differently() {
    assert_ne!(
        hash_of(&Point::new(1.0, 2.0)),
        hash_of(&Point::new(2.0, 1.0))
    );
    assert_ne!(
        hash_of(&Point::new(1.0, 2.0)),
        hash_of(&Point::new(1.0, 2.5))
    );
}

#[test]
fn equal_insets_hash_equal() {
    let a = Insets {
        left: 1.0,
        right: 2.0,
        top: 3.0,
        bottom: 4.0,
    };
    assert_eq!(hash_of(&a), hash_of(&a.clone()));
    let zero = Insets {
        left: -0.0,
        right: 0.0,
        top: -0.0,
        bottom: 0.0,
    };
    assert_eq!(zero, Insets::ZERO);
    assert_eq!(hash_of(&zero), hash_of(&Insets::ZERO));
}

#[test]
fn different_insets_hash_differently() {
    let a = Insets {
        left: 1.0,
        right: 2.0,
        top: 3.0,
        bottom: 4.0,
    };
    let b = Insets {
        left: 2.0,
        right: 1.0,
        top: 3.0,
        bottom: 4.0,
    };
    assert_ne!(hash_of(&a), hash_of(&b));
    assert_ne!(hash_of(&Insets::all(1.0)), hash_of(&Insets::all(2.0)));
}
