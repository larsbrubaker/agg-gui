//! A browser view lends its memory the whole open path when it remembers where it is: the root
//! first and the open folder last, so an app storing the path's names (or a location built from
//! every folder on it) keeps the root's name too.

use std::cell::RefCell;

use agg_gui_file_browser::{remember_open_location, BrowserMemory};
use agg_gui_files::{Folder, MemoryFolder};

/// A memory that records the key and the folder names it was lent.
#[derive(Default)]
struct Recording(RefCell<Vec<(String, Vec<String>)>>);

impl BrowserMemory for Recording {
    fn remember_location(&self, key: &str, path: &[&dyn Folder]) {
        let names = path.iter().map(|f| f.name().to_string()).collect();
        self.0.borrow_mut().push((key.to_string(), names));
    }
}

#[test]
fn the_memory_is_lent_the_open_path_from_the_root_down() {
    let open: Vec<Box<dyn Folder>> = vec![
        Box::new(MemoryFolder::new("Home")),
        Box::new(MemoryFolder::new("Projects")),
        Box::new(MemoryFolder::new("Brackets")),
    ];
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
fn a_browser_at_its_root_lends_the_root_alone() {
    let open: Vec<Box<dyn Folder>> = vec![Box::new(MemoryFolder::new("Home"))];
    let memory = Recording::default();
    remember_open_location(&memory, "main", &open);
    assert_eq!(memory.0.borrow()[0].1, vec!["Home".to_string()]);
}
