//! The folder model's shared pieces: the content-changed event a folder raises when its listing
//! lands (listeners compared by identity, so a browser can take its own back off), a folder's
//! defaults (the cheap child-folder answer, the listing failure, no place), why content could not
//! be given (`LoadError`), the place a folder lives in (`PlaceId`) and the picture handed to a
//! tile (`Picture`, top row first).

use std::cell::Cell;
use std::rc::Rc;

use agg_gui_files::{
    ContentChangedEvent, ContentChangedListener, Folder, FolderLink, Item, ListingFailure,
    LoadError, Picture, PlaceId,
};

fn counting(runs: &Rc<Cell<i32>>) -> ContentChangedListener {
    let counter = Rc::clone(runs);
    Rc::new(move || counter.set(counter.get() + 1))
}

#[test]
fn a_content_changed_listener_taken_off_is_not_run() {
    let event = ContentChangedEvent::default();
    let runs = Rc::new(Cell::new(0));
    let listener = counting(&runs);
    event.subscribe(Rc::clone(&listener));
    event.raise();
    event.unsubscribe(&listener);
    event.raise();
    assert_eq!(runs.get(), 1, "taking the listener off stops it running");
}

#[test]
fn a_listener_subscribed_twice_needs_two_removals_and_an_equal_closure_is_not_it() {
    let event = ContentChangedEvent::default();
    let runs = Rc::new(Cell::new(0));
    let listener = counting(&runs);
    event.subscribe(Rc::clone(&listener));
    event.subscribe(Rc::clone(&listener));
    // A different `Rc` around the same kind of closure is a different listener.
    event.unsubscribe(&counting(&runs));
    event.raise();
    assert_eq!(runs.get(), 2, "only the subscribed Rc takes a listener off");
    event.unsubscribe(&listener);
    event.raise();
    assert_eq!(runs.get(), 3, "one removal takes off one subscription");
}

#[test]
fn a_listener_may_subscribe_another_while_the_event_is_raised() {
    let event = ContentChangedEvent::default();
    let runs = Rc::new(Cell::new(0));
    let later = counting(&runs);
    let inner_event = event.clone();
    let adder: ContentChangedListener = Rc::new(move || inner_event.subscribe(Rc::clone(&later)));
    event.subscribe(adder);
    event.raise();
    assert_eq!(
        runs.get(),
        0,
        "a listener added during a raise waits for the next one"
    );
    event.raise();
    assert_eq!(runs.get(), 1);
}

/// A link to a folder, for listings.
struct Link(&'static str, bool);

impl Item for Link {
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
        self.1
    }
    fn as_container_link(&self) -> Option<&dyn FolderLink> {
        Some(self)
    }
    fn into_container_link(self: Rc<Self>) -> Option<Rc<dyn FolderLink>> {
        Some(self)
    }
}

impl FolderLink for Link {
    fn get_container(&self) -> Result<Box<dyn Folder>, String> {
        Err("not opened in this test".to_string())
    }
    fn is_read_only(&self) -> bool {
        true
    }
}

/// A folder listing `links` once loaded, or still waiting when `pending`.
struct Listing {
    links: Vec<Rc<dyn FolderLink>>,
    loaded: bool,
    pending: bool,
    failed: bool,
}

impl Listing {
    fn of(links: Vec<Rc<dyn FolderLink>>) -> Self {
        Self {
            links,
            loaded: false,
            pending: false,
            failed: false,
        }
    }
}

impl Folder for Listing {
    fn id(&self) -> &str {
        "listing"
    }
    fn name(&self) -> &str {
        "Listing"
    }
    fn is_protected(&self) -> bool {
        true
    }
    fn child_containers(&self) -> Vec<Rc<dyn FolderLink>> {
        if self.loaded {
            self.links.clone()
        } else {
            Vec::new()
        }
    }
    fn items(&self) -> Vec<Rc<dyn Item>> {
        Vec::new()
    }
    fn load(&mut self) {
        self.loaded = !self.pending;
    }
    fn is_load_pending(&self) -> bool {
        self.pending
    }
    fn last_load_failed(&self) -> bool {
        self.failed
    }
}

#[test]
fn has_child_containers_by_default_loads_and_counts_only_visible_folders() {
    let mut hidden_only = Listing::of(vec![Rc::new(Link("Hidden", false))]);
    assert_eq!(
        hidden_only.has_child_containers(),
        Some(false),
        "a folder holding only hidden folders shows no expand arrow"
    );
    let mut with_visible = Listing::of(vec![
        Rc::new(Link("Hidden", false)),
        Rc::new(Link("Shown", true)),
    ]);
    assert_eq!(with_visible.has_child_containers(), Some(true));
    let mut waiting = Listing::of(vec![Rc::new(Link("Shown", true))]);
    waiting.pending = true;
    assert_eq!(
        waiting.has_child_containers(),
        None,
        "a listing still on its way says nothing about what the folder holds"
    );
}

#[test]
fn a_folder_that_failed_to_list_says_other_unless_it_knows_more() {
    let mut listing = Listing::of(Vec::new());
    assert_eq!(listing.listing_failure(), None);
    listing.failed = true;
    assert_eq!(listing.listing_failure(), Some(ListingFailure::Other));
    assert_eq!(
        listing.place(),
        None,
        "a folder is in no place unless it says so"
    );
    assert_eq!(listing.location_key(), None);
    assert!(
        listing.as_writable().is_none(),
        "folders are read-only unless they say so"
    );
}

#[test]
fn a_cancelled_load_reads_as_the_cancellation_message_and_failures_as_their_own() {
    assert!(LoadError::Cancelled.is_cancellation());
    assert!(!LoadError::Download("x".into()).is_cancellation());
    assert_eq!(
        String::from(LoadError::Cancelled),
        "The operation was canceled."
    );
    assert_eq!(
        LoadError::Download("Check your connection.".into()).to_string(),
        "Check your connection."
    );
    assert_eq!(
        LoadError::from("could not be opened"),
        LoadError::Failed("could not be opened".to_string()),
        "plain text is a failure, not a cancellation"
    );
}

#[test]
fn places_compare_by_name() {
    assert_eq!(PlaceId::new("computer"), PlaceId::COMPUTER);
    assert_ne!(PlaceId::new("cloud"), PlaceId::COMPUTER);
    assert_eq!(PlaceId::new(String::from("cloud")).as_str(), "cloud");
    assert_eq!(PlaceId::COMPUTER.to_string(), "computer");
}

#[test]
fn a_picture_holds_exactly_its_pixels_top_row_first() {
    // Two by two: the top row red then green, the bottom row blue then half-clear white.
    let rgba = vec![
        255, 0, 0, 255, 0, 255, 0, 255, //
        0, 0, 255, 255, 255, 255, 255, 128,
    ];
    let picture = Picture::new(2, 2, rgba.clone()).expect("16 bytes is 2 x 2 RGBA");
    assert_eq!(
        picture.pixel(0, 0),
        Some([255, 0, 0, 255]),
        "row 0 is the top row"
    );
    assert_eq!(
        picture.pixel(1, 1),
        Some([255, 255, 255, 128]),
        "alpha is kept straight"
    );
    assert_eq!(picture.pixel(2, 0), None);
    assert_eq!(picture.pixel(0, 2), None);
    assert!(
        Picture::new(2, 2, rgba[..12].to_vec()).is_none(),
        "short pixels are refused"
    );
    assert!(Picture::new(u32::MAX, u32::MAX, Vec::new()).is_none());
}

#[test]
fn a_hand_built_picture_short_of_its_size_answers_no_pixel() {
    // Built field by field, so nothing checked the bytes against the size.
    let short = Picture {
        width: u32::MAX,
        height: u32::MAX,
        rgba: vec![1, 2, 3, 4].into(),
    };
    assert_eq!(
        short.pixel(0, 0),
        Some([1, 2, 3, 4]),
        "the bytes it has still read"
    );
    assert_eq!(short.pixel(1, 0), None, "a pixel past its bytes is none");
    assert_eq!(
        short.pixel(u32::MAX - 1, u32::MAX - 1),
        None,
        "a pixel whose offset would not fit in memory is none, not an overflow"
    );
}
