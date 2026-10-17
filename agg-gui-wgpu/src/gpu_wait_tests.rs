//! Rust-only tests for `gpu_wait.rs`: a bounded wait on a GPU that never
//! finishes ends with a timeout inside its bound, a lost device is reported
//! with its reason (without polling it, when the loss is already known), and
//! a panic wgpu raises inside the poll becomes a loss rather than escaping.
//! No GPU is needed: the poll is a stand-in.

use std::cell::Cell;
use std::time::{Duration, Instant};

use super::{bounded_wait, DeviceLoss, GpuWaitError};

/// A GPU that never reaches the submission, as wgpu's poll reports one: the
/// wait sleeps out the timeout it is given, then reports `Timeout`. Records
/// the timeout so the test can check the bound really reached the poll.
fn never_finishing(seen: &Cell<Option<Duration>>) -> impl FnOnce(Duration) -> PollResult + '_ {
    move |timeout| {
        seen.set(Some(timeout));
        std::thread::sleep(timeout);
        Err(wgpu::PollError::Timeout)
    }
}

type PollResult = Result<wgpu::PollStatus, wgpu::PollError>;

#[test]
fn a_gpu_that_never_finishes_times_out_within_the_bound() {
    let budget = Duration::from_millis(200);
    let seen = Cell::new(None);
    let started = Instant::now();
    let result = bounded_wait(never_finishing(&seen), None, budget);
    let waited = started.elapsed();

    assert_eq!(result, Err(GpuWaitError::TimedOut { budget }));
    assert_eq!(
        seen.get(),
        Some(budget),
        "the budget must reach the poll as its timeout, never an unbounded wait"
    );
    assert!(
        waited < budget + Duration::from_secs(2),
        "the wait returned after {waited:?}, far past its {budget:?} bound"
    );
}

#[test]
fn a_finished_wait_succeeds() {
    let result = bounded_wait(
        |_| Ok(wgpu::PollStatus::WaitSucceeded),
        Some(&DeviceLoss::default()),
        Duration::from_secs(1),
    );
    assert_eq!(result, Ok(()));
}

#[test]
fn a_known_loss_is_reported_without_polling() {
    let loss = DeviceLoss::default();
    loss.record("Unknown: Parent device is lost".to_string());
    let polled = Cell::new(false);
    let result = bounded_wait(
        |_| {
            polled.set(true);
            Ok(wgpu::PollStatus::QueueEmpty)
        },
        Some(&loss),
        Duration::from_secs(1),
    );
    assert_eq!(
        result,
        Err(GpuWaitError::DeviceLost(
            "Unknown: Parent device is lost".to_string()
        ))
    );
    assert!(
        !polled.get(),
        "a lost device must not be polled: wgpu panics"
    );
}

#[test]
fn a_loss_during_the_wait_names_its_reason_instead_of_the_timeout() {
    let loss = DeviceLoss::default();
    let result = bounded_wait(
        |_| {
            loss.record("Unknown: the GPU was reset".to_string());
            Err(wgpu::PollError::Timeout)
        },
        Some(&loss),
        Duration::from_millis(10),
    );
    assert_eq!(
        result,
        Err(GpuWaitError::DeviceLost(
            "Unknown: the GPU was reset".to_string()
        ))
    );
}

#[test]
fn a_panic_inside_the_poll_is_reported_as_a_loss() {
    let result = bounded_wait(
        |_| -> PollResult { panic!("Device::poll: the device is lost") },
        None,
        Duration::from_millis(10),
    );
    assert_eq!(
        result,
        Err(GpuWaitError::DeviceLost(
            "Device::poll: the device is lost".to_string()
        ))
    );
}

#[test]
fn a_refused_wait_is_reported_as_a_failure() {
    let result = bounded_wait(
        |_| Err(wgpu::PollError::WrongSubmissionIndex(9, 3)),
        None,
        Duration::from_millis(10),
    );
    assert!(matches!(result, Err(GpuWaitError::Failed(_))), "{result:?}");
}
