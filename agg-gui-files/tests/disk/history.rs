//! The recorded writes of a folder on disk and their history steps, on the disk itself: a move
//! goes back with undo and returns with redo (a part numbered to avoid a taken name comes back
//! under its old one); a rename and a new folder undo and redo (a new folder with something in
//! it is kept, and the failure says why); a remove comes back from the trash; copies, from a
//! copy task or a move from another place, go to the trash on undo and come back on redo; an
//! undo never overwrites an item that took an old name since, and keeps only what came back;
//! the steps' descriptions are the notice's and the Undo and Redo buttons' texts.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use agg_gui_files::disk::DiskFolder;
use agg_gui_files::{
    Folder, HistoryStep, Item, MemoryFolder, Recorded, StepOutcome, WritableFolder,
};

use super::temp::{block_on, stl_only, stl_only_read_only, TempDir};

const PARTS: [&str; 3] = ["Alpha.stl", "Beta.stl", "Gamma.stl"];

fn undo(step: Box<dyn HistoryStep>) -> StepOutcome {
    let outcome = Rc::new(RefCell::new(None));
    let heard = Rc::clone(&outcome);
    step.undo(Box::new(move |o| *heard.borrow_mut() = Some(o)));
    let taken = outcome.borrow_mut().take();
    taken.expect("a disk step finishes within the call")
}

fn redo(step: Box<dyn HistoryStep>) -> StepOutcome {
    let outcome = Rc::new(RefCell::new(None));
    let heard = Rc::clone(&outcome);
    step.redo(Box::new(move |o| *heard.borrow_mut() = Some(o)));
    let taken = outcome.borrow_mut().take();
    taken.expect("a disk step finishes within the call")
}

fn step_of(recorded: Recorded) -> Box<dyn HistoryStep> {
    recorded.result.expect("the change lands");
    recorded.step.expect("the change is recorded")
}

fn item_named(folder: &dyn Folder, name: &str) -> Rc<dyn Item> {
    folder
        .items()
        .into_iter()
        .chain(
            folder
                .child_containers()
                .into_iter()
                .map(|l| l as Rc<dyn Item>),
        )
        .find(|i| i.name() == name)
        .unwrap_or_else(|| panic!("{name} is listed"))
}

fn all_in(folder: &Path) -> bool {
    PARTS.iter().all(|p| folder.join(p).is_file())
}

fn none_in(folder: &Path) -> bool {
    PARTS.iter().all(|p| !folder.join(p).exists())
}

/// "My Tools" with the three parts and an empty "Brackets" beside it, loaded.
fn my_tools(dir: &TempDir) -> (DiskFolder, DiskFolder) {
    let tools = dir.folder("My Tools");
    for part in PARTS {
        std::fs::write(tools.join(part), part).unwrap();
    }
    let mut source = DiskFolder::new(&tools, stl_only());
    source.load();
    let mut target = DiskFolder::new(dir.folder("Brackets"), stl_only());
    target.load();
    (source, target)
}

#[test]
fn moved_parts_go_back_with_undo_and_return_with_redo() {
    let dir = TempDir::new();
    let (mut source, mut target) = my_tools(&dir);
    let items: Vec<Rc<dyn Item>> = source.items();
    let step = step_of(target.move_in_recorded(&items, &mut source));
    assert_eq!(step.description(), "Moved 3 items to Brackets");
    assert!(none_in(source.full_path()) && all_in(target.full_path()));

    let undone = undo(step);
    assert!(undone.failures.is_empty(), "{:?}", undone.failures);
    assert!(all_in(source.full_path()) && none_in(target.full_path()));
    let back = undone.kept.expect("all three came back");
    assert_eq!(back.description(), "Moved 3 items to Brackets");

    let redone = redo(back);
    assert!(redone.failures.is_empty());
    assert!(none_in(source.full_path()) && all_in(target.full_path()));
    assert!(redone.kept.is_some());
}

#[test]
fn a_move_that_numbered_a_taken_name_is_undone_to_the_old_name() {
    let dir = TempDir::new();
    let (mut source, mut target) = my_tools(&dir);
    std::fs::write(target.full_path().join("Alpha.stl"), "already here").unwrap();
    target.load();
    let alpha = item_named(&source, "Alpha.stl");
    let step = step_of(target.move_in_recorded(&[alpha], &mut source));
    assert_eq!(step.description(), "Moved Alpha.stl to Brackets");
    assert_eq!(
        std::fs::read_to_string(target.full_path().join("Alpha (1).stl")).unwrap(),
        "Alpha.stl"
    );

    let undone = undo(step);
    assert!(undone.failures.is_empty());
    assert_eq!(
        std::fs::read_to_string(source.full_path().join("Alpha.stl")).unwrap(),
        "Alpha.stl"
    );
    assert!(!target.full_path().join("Alpha (1).stl").exists());
    assert_eq!(
        std::fs::read_to_string(target.full_path().join("Alpha.stl")).unwrap(),
        "already here",
        "the item that was there first is untouched"
    );
    let _ = redo(undone.kept.expect("it came back"));
    assert!(target.full_path().join("Alpha (1).stl").is_file());
}

#[test]
fn an_undo_never_overwrites_an_item_that_took_the_old_name() {
    let dir = TempDir::new();
    let (mut source, mut target) = my_tools(&dir);
    let alpha = item_named(&source, "Alpha.stl");
    let step = step_of(target.move_in_recorded(&[alpha], &mut source));
    std::fs::write(source.full_path().join("Alpha.stl"), "new work").unwrap();
    let undone = undo(step);
    assert_eq!(
        undone.failures,
        ["Alpha.stl: an item with this name is already there"]
    );
    assert!(
        undone.kept.is_none(),
        "nothing came back, so nothing is left to redo"
    );
    assert_eq!(
        std::fs::read_to_string(source.full_path().join("Alpha.stl")).unwrap(),
        "new work"
    );
    assert!(target.full_path().join("Alpha.stl").is_file());
}

#[test]
fn renames_undo_and_redo() {
    let dir = TempDir::new();
    let (mut tools, _) = my_tools(&dir);
    let alpha = item_named(&tools, "Alpha.stl");
    let step = step_of(tools.rename_recorded(&alpha, "Bolt"));
    assert_eq!(step.description(), "Renamed Alpha.stl to Bolt.stl");
    let root = tools.full_path().to_path_buf();
    assert!(root.join("Bolt.stl").is_file() && !root.join("Alpha.stl").exists());
    let undone = undo(step);
    assert!(root.join("Alpha.stl").is_file() && !root.join("Bolt.stl").exists());
    let _ = redo(undone.kept.expect("the rename came back"));
    assert!(root.join("Bolt.stl").is_file() && !root.join("Alpha.stl").exists());
}

#[test]
fn the_same_name_again_records_nothing() {
    let dir = TempDir::new();
    let (mut tools, _) = my_tools(&dir);
    let alpha = item_named(&tools, "Alpha.stl");
    let recorded = tools.rename_recorded(&alpha, "Alpha.stl");
    recorded.result.expect("nothing to do is no failure");
    assert!(recorded.step.is_none());
}

#[test]
fn a_new_folder_undoes_and_redoes_and_one_with_something_in_it_is_kept() {
    let dir = TempDir::new();
    let (mut tools, _) = my_tools(&dir);
    let hinges = tools.full_path().join("Hinges");
    let step = step_of(tools.create_folder_recorded("Hinges"));
    assert_eq!(step.description(), "Created folder Hinges");
    assert!(hinges.is_dir());
    let undone = undo(step);
    assert!(!hinges.exists(), "undo removes the new, empty folder");
    let redone = redo(undone.kept.expect("the folder can be made again"));
    assert!(hinges.is_dir(), "redo makes it again");

    std::fs::write(hinges.join("hinge.stl"), "h").unwrap();
    let kept = undo(redone.kept.expect("recorded again"));
    assert_eq!(kept.failures, ["Hinges: the folder isn't empty"]);
    assert!(kept.kept.is_none());
    assert!(hinges.join("hinge.stl").is_file());
}

#[test]
fn a_folder_already_there_is_not_recorded() {
    let dir = TempDir::new();
    let (mut tools, _) = my_tools(&dir);
    std::fs::create_dir(tools.full_path().join("Hinges")).unwrap();
    let recorded = tools.create_folder_recorded("Hinges");
    recorded.result.expect("an existing folder is kept");
    assert!(recorded.step.is_none(), "undoing must not take it away");
}

#[test]
fn removed_parts_come_back_from_the_trash() {
    let dir = TempDir::new();
    let (mut tools, _) = my_tools(&dir);
    let items = tools.items();
    let step = step_of(tools.remove_recorded(&items));
    let trash = if cfg!(windows) {
        "the Recycle Bin"
    } else {
        "the Trash"
    };
    assert_eq!(step.description(), format!("Moved 3 items to {trash}"));
    assert!(none_in(tools.full_path()));
    let undone = undo(step);
    assert!(undone.failures.is_empty());
    assert!(all_in(tools.full_path()));
    let redone = redo(undone.kept.expect("all came back"));
    assert!(none_in(tools.full_path()));
    let _ = undo(redone.kept.expect("all went again"));
    assert!(all_in(tools.full_path()));
}

#[test]
fn a_copy_task_records_its_copies_which_undo_sends_to_the_trash() {
    let dir = TempDir::new();
    let target = DiskFolder::new(dir.path(), stl_only()).with_name("Brackets");
    let elsewhere = MemoryFolder::new("Elsewhere");
    let gear = elsewhere.add_file("gear.stl", b"gear".to_vec());
    let outcome = Rc::new(RefCell::new(None));
    let heard = Rc::clone(&outcome);
    let task = target
        .copy_in(
            vec![gear],
            Box::new(move |done| *heard.borrow_mut() = Some(done)),
        )
        .expect("a folder on disk runs its own copy");
    block_on(task);
    let recorded = outcome.borrow_mut().take().expect("the copy finished");
    let step = step_of(recorded);
    assert_eq!(step.description(), "Copied gear.stl to Brackets");
    let copy = dir.path().join("gear.stl");
    assert!(copy.is_file());
    let undone = undo(step);
    assert!(!copy.exists(), "undo sends the copy to the trash");
    assert_eq!(elsewhere.items().len(), 1, "the original is never touched");
    let back = undone.kept.expect("the copy can come back");
    assert_eq!(back.description(), "Copied gear.stl to Brackets");
    let _ = redo(back);
    assert_eq!(std::fs::read(&copy).unwrap(), b"gear");
}

#[test]
fn a_move_from_another_place_records_the_copies() {
    let dir = TempDir::new();
    let mut target = DiskFolder::new(dir.path(), stl_only()).with_name("Brackets");
    let mut elsewhere = MemoryFolder::new("Elsewhere");
    let a = elsewhere.add_file("a.stl", b"a".to_vec());
    let b = elsewhere.add_file("b.stl", b"b".to_vec());
    let step = step_of(target.move_in_recorded(&[a, b], &mut elsewhere));
    assert_eq!(step.description(), "Copied 2 items to Brackets");
    let _ = undo(step);
    assert!(!dir.path().join("a.stl").exists() && !dir.path().join("b.stl").exists());
}

#[test]
fn a_read_only_folder_records_nothing() {
    let dir = TempDir::new();
    dir.file("part.stl", b"p");
    let mut folder = DiskFolder::new(dir.path(), stl_only_read_only());
    folder.load();
    let part = item_named(&folder, "part.stl");
    for recorded in [
        folder.rename_recorded(&part, "renamed"),
        folder.create_folder_recorded("New"),
        folder.remove_recorded(std::slice::from_ref(&part)),
    ] {
        assert!(recorded.result.is_err());
        assert!(recorded.step.is_none());
    }
    assert!(dir.path().join("part.stl").is_file());
    assert!(!dir.path().join("New").exists());
}
