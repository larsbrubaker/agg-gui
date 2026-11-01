//! How a listing is ordered: the sort keys and [`SortBehavior`] (kept in the per-folder view
//! state an app stores, `{"SortKey":1,"Ascending":true}`), [`sort_items`] (the order a key and
//! direction give), and the item dates it sorts on (ticks: 100 ns units, compared only with each
//! other). `item` declares the item dates and `folder` a folder's `default_sort`; a browser's list
//! view applies them.

use std::cmp::{Ordering, Reverse};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::item::Item;

/// What a listing sorts on. The numbers are flags, as stored view states hold them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    Default = 0,
    Name = 1,
    CreatedDate = 2,
    ModifiedDate = 4,
}

/// A listing's sort: a key and a direction. The key is kept as its stored flag number so any value
/// an app stored reads and writes back unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SortBehavior {
    pub sort_key: i32,
    pub ascending: bool,
}

impl SortBehavior {
    /// A behavior sorting on `key`.
    pub fn new(key: SortKey, ascending: bool) -> Self {
        Self {
            sort_key: key as i32,
            ascending,
        }
    }

    /// Whether the sort includes `key`, which a sort menu's radio items read.
    pub fn has_flag(&self, key: SortKey) -> bool {
        self.sort_key & key as i32 == key as i32
    }
}

impl Default for SortBehavior {
    /// Name, ascending: the list view's sort until the user picks another.
    fn default() -> Self {
        Self::new(SortKey::Name, true)
    }
}

/// Names compared as a reader expects (case-insensitive first), as a case-folded compare with the
/// ordinal compare as the tie-break, so the order is total.
pub fn culture_compare(a: &str, b: &str) -> Ordering {
    a.to_lowercase()
        .cmp(&b.to_lowercase())
        .then_with(|| a.cmp(b))
}

/// A file time as ticks (100 ns since 1970; ticks are only compared with each other, so the epoch
/// does not matter). 0 when the platform cannot say. A time past what an `i64` of ticks holds
/// (about the year 30,000) saturates.
pub fn system_time_ticks(time: SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_nanos() / 100).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// `items` ordered by `sort` (dates or name, ascending or descending; anything else is name
/// ascending). The sort is stable, so equal keys keep the folder's order.
pub fn sort_items<T, F>(items: &mut [T], sort: SortBehavior, item: F)
where
    F: Fn(&T) -> &dyn Item,
{
    let created = SortKey::CreatedDate as i32;
    let modified = SortKey::ModifiedDate as i32;
    match (sort.sort_key, sort.ascending) {
        (k, true) if k == created => items.sort_by_key(|a| item(a).date_created()),
        (k, false) if k == created => items.sort_by_key(|a| Reverse(item(a).date_created())),
        (k, true) if k == modified => items.sort_by_key(|a| item(a).date_modified()),
        (k, false) if k == modified => items.sort_by_key(|a| Reverse(item(a).date_modified())),
        (k, false) if k == SortKey::Name as i32 => {
            items.sort_by(|a, b| culture_compare(item(b).name(), item(a).name()))
        }
        _ => items.sort_by(|a, b| culture_compare(item(a).name(), item(b).name())),
    }
}
