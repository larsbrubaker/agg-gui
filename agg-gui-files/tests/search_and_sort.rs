//! What a search query matches (`name_matches`), how a search is stopped (`SearchCancel`) and
//! gives the frame a turn (`yield_now`), and the order a sort gives (`sort_items`, with the
//! stored sort flags).

use std::cmp::Ordering;
use std::future::Future;
use std::pin::pin;
use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::time::{Duration, UNIX_EPOCH};

use agg_gui_files::{
    culture_compare, name_matches, sort_items, system_time_ticks, yield_now, Item, SearchCancel,
    SortBehavior, SortKey,
};

/// The file types a test app opens.
fn opens(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.ends_with(".stl") || lower.ends_with(".mcx")
}

#[test]
fn stripping_an_extension_only_drops_extensions_the_app_recognizes() {
    let content_file: &dyn Fn(&str) -> bool = &opens;
    // A search for 'stl' in a folder of meshes would otherwise match every single one.
    assert!(!name_matches("gear box.stl", "stl", Some(content_file)));
    // '.2 bracket' is not a file extension - dropping it would make most of the name unsearchable.
    assert!(name_matches(
        "Rev 1.2 bracket",
        "bracket",
        Some(content_file)
    ));
    // Without the test the name is matched whole, which is what a non-file item needs.
    assert!(name_matches("gear box.stl", "stl", None));
    // A query carrying an underscore has to find the file it was copied from.
    assert!(name_matches("gear_box.stl", "gear_bo", Some(content_file)));
    // No query is not a match - it must not fail a caller mid-search.
    assert!(!name_matches("gear box.stl", None, None));
}

#[test]
fn every_term_must_appear_in_any_order_and_any_case() {
    assert!(name_matches("Gear Box Lid", "lid GEAR", None));
    assert!(!name_matches("Gear Box Lid", "gear hinge", None));
    assert!(
        name_matches("Gear Box Lid", "", None),
        "an empty query matches everything"
    );
    assert!(
        name_matches("Gear Box Lid", "  box  ", None),
        "extra spaces are no terms"
    );
}

/// Counts its wakes.
struct CountingWaker(AtomicUsize);

impl Wake for CountingWaker {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, AtomicOrdering::SeqCst);
    }
}

#[test]
fn cancelling_wakes_each_waiting_future_once_and_only_the_first_time() {
    let counter = Arc::new(CountingWaker(AtomicUsize::new(0)));
    let waker = Waker::from(Arc::clone(&counter));
    let cancel = SearchCancel::new();
    let shared = cancel.clone();
    cancel.register(&waker);
    cancel.register(&waker);
    assert!(!shared.is_cancelled());
    shared.cancel();
    assert!(cancel.is_cancelled(), "clones share the flag");
    assert_eq!(
        counter.0.load(AtomicOrdering::SeqCst),
        1,
        "a waker registered twice wakes once"
    );
    shared.cancel();
    assert_eq!(
        counter.0.load(AtomicOrdering::SeqCst),
        1,
        "a second cancel wakes nobody"
    );
}

#[test]
fn yield_now_gives_the_executor_one_turn() {
    let counter = Arc::new(CountingWaker(AtomicUsize::new(0)));
    let waker = Waker::from(Arc::clone(&counter));
    let mut cx = Context::from_waker(&waker);
    let mut turn = pin!(yield_now());
    assert_eq!(turn.as_mut().poll(&mut cx), Poll::Pending);
    assert_eq!(
        counter.0.load(AtomicOrdering::SeqCst),
        1,
        "it asks to be polled again at once"
    );
    assert_eq!(turn.as_mut().poll(&mut cx), Poll::Ready(()));
}

/// An item with a name and dates.
struct Dated(&'static str, i64, i64);

impl Item for Dated {
    fn id(&self) -> &str {
        self.0
    }
    fn name(&self) -> &str {
        self.0
    }
    fn is_protected(&self) -> bool {
        false
    }
    fn is_visible(&self) -> bool {
        true
    }
    fn date_created(&self) -> i64 {
        self.1
    }
    fn date_modified(&self) -> i64 {
        self.2
    }
}

fn sorted(sort: SortBehavior) -> Vec<&'static str> {
    let mut items = vec![
        Dated("beta", 3, 1),
        Dated("Alpha", 1, 2),
        Dated("alpha", 2, 2),
        Dated("Gamma", 2, 3),
    ];
    sort_items(&mut items, sort, |d| d as &dyn Item);
    items.iter().map(|d| d.0).collect()
}

#[test]
fn names_sort_case_insensitively_with_a_total_tie_break() {
    assert_eq!(culture_compare("alpha", "Beta"), Ordering::Less);
    assert_eq!(
        culture_compare("Alpha", "alpha"),
        Ordering::Less,
        "ordinal decides a case tie"
    );
    assert_eq!(
        sorted(SortBehavior::default()),
        ["Alpha", "alpha", "beta", "Gamma"]
    );
    assert_eq!(
        sorted(SortBehavior::new(SortKey::Name, false)),
        ["Gamma", "beta", "alpha", "Alpha"]
    );
}

#[test]
fn date_sorts_are_stable_so_equal_dates_keep_the_folders_order() {
    assert_eq!(
        sorted(SortBehavior::new(SortKey::CreatedDate, true)),
        ["Alpha", "alpha", "Gamma", "beta"]
    );
    assert_eq!(
        sorted(SortBehavior::new(SortKey::ModifiedDate, false)),
        ["Gamma", "Alpha", "alpha", "beta"]
    );
    // A key no sort knows reads as name ascending.
    let unknown = SortBehavior {
        sort_key: 64,
        ascending: false,
    };
    assert_eq!(sorted(unknown), ["Alpha", "alpha", "beta", "Gamma"]);
}

#[test]
fn sort_keys_are_the_stored_flag_numbers() {
    assert_eq!(SortKey::Default as i32, 0);
    assert_eq!(SortKey::Name as i32, 1);
    assert_eq!(SortKey::CreatedDate as i32, 2);
    assert_eq!(SortKey::ModifiedDate as i32, 4);
    let both = SortBehavior {
        sort_key: 1 | 4,
        ascending: true,
    };
    assert!(both.has_flag(SortKey::Name) && both.has_flag(SortKey::ModifiedDate));
    assert!(!both.has_flag(SortKey::CreatedDate));
}

#[test]
fn file_times_are_ticks_of_100_ns() {
    assert_eq!(system_time_ticks(UNIX_EPOCH), 0);
    assert_eq!(system_time_ticks(UNIX_EPOCH + Duration::from_micros(3)), 30);
    assert_eq!(
        system_time_ticks(UNIX_EPOCH - Duration::from_secs(1)),
        0,
        "a time before 1970 reads as unknown"
    );
}
