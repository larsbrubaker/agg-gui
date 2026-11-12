//! Rust-only tests of `device_lost_flag.rs`: a lost device means no read of
//! a mapped buffer, and every clone of a flag sees the loss.

use super::*;

#[test]
fn a_device_nobody_watches_allows_the_read() {
    assert_eq!(mapped_read_allowed(None), Ok(()));
}

#[test]
fn a_live_device_allows_the_read() {
    let flag = DeviceLostFlag::default();
    assert_eq!(mapped_read_allowed(Some(flag.loss())), Ok(()));
}

#[test]
fn a_lost_device_refuses_the_read_with_its_reason() {
    let flag = DeviceLostFlag::default();
    flag.record_for_test("Unknown: the driver reset");
    assert_eq!(
        mapped_read_allowed(Some(flag.loss())),
        Err("Unknown: the driver reset".to_owned())
    );
}

#[test]
fn every_clone_sees_a_loss_recorded_after_it_was_made() {
    let flag = DeviceLostFlag::default();
    let kept = flag.clone();
    assert!(!kept.is_lost());
    assert_eq!(kept.lost_reason(), None);
    flag.record_for_test("Destroyed");
    assert!(kept.is_lost());
    assert_eq!(kept.lost_reason().as_deref(), Some("Destroyed"));
}
