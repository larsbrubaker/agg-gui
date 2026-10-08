//! `AutomationError`: how an automation run fails — the exceptions C#'s
//! `ShowWindowAndExecuteTests` throws, as one error type.
//!
//! Used by [`super::show_window_and_execute_tests`], which reports one
//! failure per run, picked in C#'s order (see [`super::rank_outcome`]).

use std::error::Error;
use std::fmt;

/// C#'s message when a body did not call `MarkTestComplete`.
pub const TEST_NOT_COMPLETED_MESSAGE: &str = "Test did not call MarkTestComplete(). The test may have exited before reaching its last statement.";

/// Why an automation run failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AutomationError {
    /// The body overstayed its wall-clock budget (C# `TimeoutException`
    /// "TestMethod timed out"). The budget starts when the window has loaded.
    Timeout,
    /// The window did not finish loading (first paint plus `on_load`) within
    /// the bring-up budget (C# `TimeoutException` "Reset event timed out").
    LoadTimeout,
    /// Building or bringing up the window panicked, with the panic's message
    /// (C#'s `ShowAsSystemWindow` failure, rethrown).
    BringUpFailed(String),
    /// The body panicked, with the panic's message (C# rethrows the body's
    /// exception).
    BodyPanicked(String),
    /// The body returned without calling `mark_test_complete` while
    /// `require_test_completion` was set.
    TestNotCompleted,
}

impl fmt::Display for AutomationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Timeout => f.write_str("TestMethod timed out"),
            Self::LoadTimeout => f.write_str("Reset event timed out"),
            Self::BringUpFailed(message) => {
                write!(f, "The test window failed to come up: {message}")
            }
            Self::BodyPanicked(message) => f.write_str(message),
            Self::TestNotCompleted => f.write_str(TEST_NOT_COMPLETED_MESSAGE),
        }
    }
}

impl Error for AutomationError {}
