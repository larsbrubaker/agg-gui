//! Why an item could not give its content (its stream, its picture, or whatever an app makes from
//! it). A cancellation (the user pressed Stop, or a download timed out) is kept apart from a
//! failure, so a caller that runs the load as a task can handle it as a Stop rather than putting
//! an error in front of the user, and content that had to be downloaded and did not arrive is kept
//! apart too, so its message (which says to check the connection) reaches the user wherever a
//! failure reading a file shows only a plain "could not be opened". Used by the item traits in
//! `item`.

use std::fmt;

/// An item's load that did not produce content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    /// The load was cancelled.
    Cancelled,
    /// The load failed, with the message for the user.
    Failed(String),
    /// The content had to be downloaded and did not arrive, with the message for the user.
    Download(String),
}

impl LoadError {
    /// Whether this is a cancellation rather than a failure.
    pub fn is_cancellation(&self) -> bool {
        matches!(self, LoadError::Cancelled)
    }
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // The wording users have always seen for a cancelled operation.
            LoadError::Cancelled => f.write_str("The operation was canceled."),
            LoadError::Failed(message) | LoadError::Download(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for LoadError {}

impl From<String> for LoadError {
    fn from(message: String) -> Self {
        LoadError::Failed(message)
    }
}

impl From<&str> for LoadError {
    fn from(message: &str) -> Self {
        LoadError::Failed(message.to_string())
    }
}

/// For callers that report any load problem as text (a message line): a cancellation reads as
/// the cancellation message.
impl From<LoadError> for String {
    fn from(error: LoadError) -> Self {
        error.to_string()
    }
}
