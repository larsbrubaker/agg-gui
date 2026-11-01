//! A [`PlaceId`]: the storage a folder lives in (`folder`'s and `item`'s `place`). Items dropped
//! between folders of two different places are copied, keeping the originals; within one place
//! they are moved. A folder in no such place (a catalog, search results) answers `None`, and
//! nothing moves or copies into or out of it by drag. The crate names the computer's own disk;
//! an app names its other places (`PlaceId::new("cloud")`).

use std::borrow::Cow;
use std::fmt;

/// The storage a folder lives in, compared by its name.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PlaceId(Cow<'static, str>);

impl PlaceId {
    /// The computer's own disk.
    pub const COMPUTER: PlaceId = PlaceId(Cow::Borrowed("computer"));

    /// The place called `name`.
    pub fn new(name: impl Into<Cow<'static, str>>) -> Self {
        Self(name.into())
    }

    /// The place's name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PlaceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
