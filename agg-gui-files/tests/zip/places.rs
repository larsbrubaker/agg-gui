//! Where a directory inside an archive is: in no place (an archive keeps its items, so what
//! leaves it is a copy and nothing lands in it), and keyed by the archive and the directory, the
//! same from its folder and its row, never equal to a folder on disk's key.

use std::path::Path;

use agg_gui_files::zip_folder::{zip_location_key, ZipFolder, ZipFolderLink};
use agg_gui_files::{Folder, FolderLink, Item};

#[test]
fn a_zip_directory_and_its_row_are_keyed_by_the_archive_and_directory_in_no_place() {
    let archive = Path::new("/designs/Knife.zip");
    let mut link = ZipFolderLink::new(archive, None);
    let top = ZipFolder::new(archive.to_string_lossy(), "");
    assert_eq!(link.location_key(), Some(zip_location_key(archive, "")));
    assert_eq!(top.location_key(), link.location_key());
    link.set_current_directory("Blades".to_string());
    assert_eq!(link.name(), "Blades");
    let blades = ZipFolder::new(archive.to_string_lossy(), "Blades");
    assert_eq!(link.location_key(), blades.location_key());
    if cfg!(unix) {
        assert_eq!(
            zip_location_key(archive, "Blades"),
            "\"/designs/Knife.zip\"\0Blades"
        );
    }
    assert_eq!(link.place(), None);
    assert_eq!(top.place(), None);
    assert_eq!(link.tile_icon(), Some("zip_folder.png"));
}
