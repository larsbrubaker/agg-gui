//! A browser view hands its memory the whole open path (its shared `OpenFolder`s) when it
//! remembers where it is: the root first and the open folder last, so an app storing the path's names (or a location built from
//! every folder on it) keeps the root's name too.

use std::cell::RefCell;
use std::rc::Rc;

use agg_gui_file_browser::{remember_open_location, BrowserMemory, OpenFolder};
use agg_gui_files::{Folder, MemoryFolder};

/// A memory that records the key and the folder names it was handed.
#[derive(Default)]
struct Recording(RefCell<Vec<(String, Vec<String>)>>);

impl BrowserMemory for Recording {
    fn remember_location(&self, key: &str, path: &[OpenFolder]) {
        let names = path.iter().map(|f| f.borrow().name().to_string()).collect();
        self.0.borrow_mut().push((key.to_string(), names));
    }
}

fn open_folder(folder: MemoryFolder) -> OpenFolder {
    Rc::new(RefCell::new(Box::new(folder) as Box<dyn Folder>))
}

#[test]
fn the_memory_is_handed_the_open_path_from_the_root_down() {
    let open: Vec<OpenFolder> = ["Home", "Projects", "Brackets"]
        .into_iter()
        .map(|name| open_folder(MemoryFolder::new(name)))
        .collect();
    let memory = Recording::default();
    remember_open_location(&memory, "save dialog", &open);
    assert_eq!(
        *memory.0.borrow(),
        vec![(
            "save dialog".to_string(),
            vec![
                "Home".to_string(),
                "Projects".to_string(),
                "Brackets".to_string()
            ]
        )],
        "the root comes first, the open folder last"
    );
}

#[test]
fn a_browser_at_its_root_hands_over_the_root_alone() {
    let open: Vec<OpenFolder> = vec![open_folder(MemoryFolder::new("Home"))];
    let memory = Recording::default();
    remember_open_location(&memory, "main", &open);
    assert_eq!(memory.0.borrow()[0].1, vec!["Home".to_string()]);
}
