//! A search's results on the browser's path: pushed as one more folder below the folder searched
//! (`push_search_results`), so the breadcrumbs show them under it, and listed in sections, one per
//! source. While any part of the search runs a "Searching" line leads the list; each section then
//! gets a header (its name, and "(n)", "No results" or its error once it is done) over its
//! results, each section sorted on its own, its folders (found by name: folder rows,
//! `BrowserEntry::Container`, from `Item::into_container_link`, so they open, take drops and can
//! be a Move's or an Open's folder) before its files ([`search_listing`]; a browser's item filter
//! keeps only its kinds of results, counted so, and the folder rules its folders). A rolled-up
//! section keeps its
//! header and hides its rows; what is rolled up is remembered by name (ignoring case) for the
//! session ([`SearchSectionCollapseState`]), so every search and every browser agree.
//!
//! Showing the same search's results again (a batch landing, a re-sort, a section rolled up or
//! opened) keeps the list's place, and the selection is matched back onto the rebuilt rows by
//! identity key; selected results a rolled-up section hides are carried by that key until it
//! opens. Part of `state/mod.rs`, whose fields it reads; the header rows themselves are the list
//! widget's.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use agg_gui_files::{
    sort_items, tr, ContentChangedListener, Folder, FolderLink, Item, PictureFuture, PlaceId,
    SearchResultsFolder, SortBehavior,
};

use super::browser_entry::item_identity_key;
use super::{BrowserEntry, BrowserState};

thread_local! {
    /// The session's rolled-up sections (UI thread only).
    static SESSION: RefCell<SearchSectionCollapseState> =
        RefCell::new(SearchSectionCollapseState::default());
}

/// Which search result sections the user has rolled up, by name. Names rather than sections,
/// because streaming results rebuild every section several times while the user looks at them;
/// held for the run of the application (a reading position, not a preference).
#[derive(Default)]
pub struct SearchSectionCollapseState {
    /// Names kept lower-cased.
    collapsed_sections: HashSet<String>,
}

impl SearchSectionCollapseState {
    /// Every section expanded.
    pub fn new() -> Self {
        Self::default()
    }

    /// The state every browser in this session shares.
    pub fn session<R>(f: impl FnOnce(&mut SearchSectionCollapseState) -> R) -> R {
        SESSION.with(|s| f(&mut s.borrow_mut()))
    }

    /// Whether the section is rolled up (names compared ignoring case); a section with no name
    /// never is.
    pub fn is_collapsed<'a>(&self, section_name: impl Into<Option<&'a str>>) -> bool {
        section_name
            .into()
            .is_some_and(|name| self.collapsed_sections.contains(&name.to_lowercase()))
    }

    /// Rolls the section up or opens it; nothing for a section with no name.
    pub fn set_collapsed<'a>(&mut self, section_name: impl Into<Option<&'a str>>, collapsed: bool) {
        let Some(name) = section_name.into() else {
            return;
        };
        let key = name.to_lowercase();
        if collapsed {
            self.collapsed_sections.insert(key);
        } else {
            self.collapsed_sections.remove(&key);
        }
    }

    /// Flips one section and answers what it became.
    pub fn toggle<'a>(&mut self, section_name: impl Into<Option<&'a str>>) -> bool {
        let section_name = section_name.into();
        let collapsed = !self.is_collapsed(section_name);
        self.set_collapsed(section_name, collapsed);
        collapsed
    }
}

/// One line of the list: a header, or the row of `entries()[index]`.
#[derive(Clone, Debug, PartialEq)]
pub enum ListSlot {
    /// The "Searching for '{query}'" line.
    Searching { query: String },
    /// A source's section header.
    Section(SectionHeader),
    /// The row for this index of the state's entries.
    Entry(usize),
}

/// What a section header shows.
#[derive(Clone, Debug, PartialEq)]
pub struct SectionHeader {
    pub name: String,
    pub collapsed: bool,
    /// "(n)", "No results" or the error; `None` while the section is still searching.
    pub status: Option<String>,
}

/// The lines the list shows for `results` and the rows among them (the visible items of each
/// section, sorted on their own, none for a rolled-up section).
pub fn search_listing(
    results: &SearchResultsFolder,
    sort: SortBehavior,
) -> (Vec<ListSlot>, Vec<BrowserEntry>) {
    listing_kept(results, sort, &|_| true)
}

/// [`search_listing`] of the results `keep` keeps (a browser's item filter), each section's
/// count theirs.
pub(super) fn listing_kept(
    results: &SearchResultsFolder,
    sort: SortBehavior,
    keep: &dyn Fn(&dyn Item) -> bool,
) -> (Vec<ListSlot>, Vec<BrowserEntry>) {
    let mut slots = Vec::new();
    let mut entries = Vec::new();
    if results.is_searching() {
        slots.push(ListSlot::Searching {
            query: results.query().to_string(),
        });
    }
    for section in results.sections() {
        let mut items: Vec<_> = section
            .items()
            .into_iter()
            .filter(|item| item.is_visible() && keep(item.as_ref()))
            .collect();
        sort_items(&mut items, sort, |item| item.as_ref());
        // Folders found by name lead their section, as folders lead any folder's listing.
        let (mut items, files): (Vec<_>, Vec<_>) = items
            .into_iter()
            .partition(|item| item.as_container_link().is_some());
        items.extend(files);
        let collapsed = SearchSectionCollapseState::session(|s| s.is_collapsed(section.name()));
        let status = if let Some(error) = section.error_message() {
            Some(error)
        } else if !section.is_searching() {
            Some(if items.is_empty() {
                tr("No results")
            } else {
                format!("({})", items.len())
            })
        } else {
            None
        };
        slots.push(ListSlot::Section(SectionHeader {
            name: section.name().to_string(),
            collapsed,
            status,
        }));
        if collapsed {
            continue;
        }
        for item in items {
            slots.push(ListSlot::Entry(entries.len()));
            // A folder found by name is a folder row, so it opens, takes drops and can be a
            // Move's or an Open's folder like any other.
            entries.push(match Rc::clone(&item).into_container_link() {
                Some(link) => BrowserEntry::Container(link),
                None => BrowserEntry::Item(item),
            });
        }
    }
    (slots, entries)
}

/// A search's results on the path: whoever runs the search keeps the results too (to end it),
/// so the path holds them shared. The search was started before they were pushed, so `load` has
/// nothing left to do.
struct SharedResults(Rc<SearchResultsFolder>);

impl Folder for SharedResults {
    fn id(&self) -> &str {
        self.0.id()
    }
    fn name(&self) -> &str {
        self.0.name()
    }
    fn is_protected(&self) -> bool {
        self.0.is_protected()
    }
    fn child_containers(&self) -> Vec<Rc<dyn FolderLink>> {
        self.0.child_containers()
    }
    fn items(&self) -> Vec<Rc<dyn Item>> {
        self.0.items()
    }
    fn load(&mut self) {}
    fn default_sort(&self) -> Option<SortBehavior> {
        self.0.default_sort()
    }
    fn subscribe_content_changed(&self, listener: ContentChangedListener) {
        self.0.subscribe_content_changed(listener)
    }
    fn unsubscribe_content_changed(&self, listener: &ContentChangedListener) {
        self.0.unsubscribe_content_changed(listener)
    }
    fn get_thumbnail(&self, item: &Rc<dyn Item>, width: i32, height: i32) -> Option<PictureFuture> {
        self.0.get_thumbnail(item, width, height)
    }
    /// The results themselves, so an app's "Open File Location" knows it is looking at a search.
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        self.0.as_any()
    }
    fn place(&self) -> Option<PlaceId> {
        self.0.place()
    }
    fn location_key(&self) -> Option<String> {
        self.0.location_key()
    }
}

impl BrowserState {
    /// Shows a search's results (already started) below the folder searched.
    pub fn push_search_results(&mut self, results: Rc<SearchResultsFolder>) {
        let depth = self.path.len();
        self.results = Some((Rc::clone(&results), depth));
        self.push_container(Box::new(SharedResults(results)));
    }

    /// The search results the browser is showing, if it is showing some.
    pub fn showing_search_results(&self) -> Option<&Rc<SearchResultsFolder>> {
        self.results
            .as_ref()
            .filter(|(_, depth)| depth + 1 == self.path.len())
            .map(|(results, _)| results)
    }

    /// The lines the list shows: for search results the section headers between the rows,
    /// otherwise one per entry.
    pub fn list_slots(&self) -> Vec<ListSlot> {
        if let Some(results) = self.showing_search_results() {
            return listing_kept(results, self.sort, &|i| self.keeps_result(i)).0;
        }
        (0..self.entries().len()).map(ListSlot::Entry).collect()
    }

    /// A click on a search section's header: rolls it up or opens it, and the results are shown
    /// again, so the selection follows its items.
    pub fn toggle_search_section(&mut self, name: &str) {
        SearchSectionCollapseState::session(|s| s.toggle(name));
        self.redisplay();
    }

    /// The same search's results shown again: the keys of the selection and of what rolled-up
    /// sections hide select the rows that carry them, in listing order; those in a section still
    /// rolled up stay carried.
    pub(super) fn restore_search_selection(&mut self) {
        let Some(results) = self.showing_search_results().cloned() else {
            return;
        };
        let mut keys: HashSet<String> = std::mem::take(&mut self.parked_selection)
            .into_iter()
            .collect();
        keys.extend(self.selected.iter().map(BrowserEntry::identity_key));
        self.selected = self
            .entries()
            .into_iter()
            .filter(|entry| keys.contains(&entry.identity_key()))
            .collect();
        for section in results.sections() {
            if !SearchSectionCollapseState::session(|s| s.is_collapsed(section.name())) {
                continue;
            }
            for item in section.items().into_iter().filter(|item| item.is_visible()) {
                let key = item_identity_key(item.as_ref());
                if keys.contains(&key) && !self.parked_selection.contains(&key) {
                    self.parked_selection.push(key);
                }
            }
        }
    }
}
