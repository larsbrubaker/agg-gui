//! `MemoryFolder`'s recorded writes: a move from another memory folder takes the very entries
//! across (IDs kept, a folder joining the place it lands in) and each write hands back a step
//! whose undo takes it back and whose redo makes it again, IDs kept throughout.

use std::cell::RefCell;
use std::rc::Rc;

use agg_gui_files::{
    Folder, HistoryStep, Item, MemoryFolder, PlaceId, StepOutcome, WritableFolder,
};

fn ids_and_names(folder: &dyn Folder) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = folder
        .child_containers()
        .iter()
        .map(|l| (l.id().to_string(), l.name().to_string()))
        .collect();
    out.extend(
        folder
            .items()
            .iter()
            .map(|i| (i.id().to_string(), i.name().to_string())),
    );
    out
}

/// Runs `step`'s undo (or redo) and answers the step it hands back, with no failures.
fn run(step: Box<dyn HistoryStep>, undo: bool) -> Box<dyn HistoryStep> {
    let got: Rc<RefCell<Option<StepOutcome>>> = Rc::new(RefCell::new(None));
    let keep = Rc::clone(&got);
    let done = Box::new(move |outcome| *keep.borrow_mut() = Some(outcome));
    match undo {
        true => step.undo(done),
        false => step.redo(done),
    }
    let outcome = got.borrow_mut().take().expect("finished at once");
    assert!(outcome.failures.is_empty(), "{:?}", outcome.failures);
    outcome.kept.expect("a step to reverse it")
}

fn item(folder: &dyn Folder, name: &str) -> Rc<dyn Item> {
    let mut all: Vec<Rc<dyn Item>> = folder.items();
    all.extend(
        folder
            .child_containers()
            .into_iter()
            .map(|l| l as Rc<dyn Item>),
    );
    all.into_iter()
        .find(|i| i.name() == name)
        .unwrap_or_else(|| panic!("{name} listed"))
}

#[test]
fn a_move_keeps_ids_and_undoes_and_redoes() {
    let home = MemoryFolder::new("Home").with_place(PlaceId::new("memory"));
    let parts = home.add_folder("Parts").expect("free");
    home.add_file("gear.stl", b"g".to_vec());
    let brackets = home.add_folder("Brackets").expect("free");
    brackets.add_file("corner.stl", b"c".to_vec());
    let before = ids_and_names(&home);
    let moving = vec![item(&home, "gear.stl"), item(&home, "Brackets")];
    let (mut target, mut source) = (parts.clone(), home.clone());
    let recorded = target.move_in_recorded(&moving, &mut source);
    assert!(recorded.result.is_ok());
    let step = recorded.step.expect("recorded");
    assert_eq!(step.description(), "Moved 2 items to Parts");
    let moved_ids: Vec<String> = ids_and_names(&parts).into_iter().map(|p| p.0).collect();
    for m in &moving {
        assert!(
            moved_ids.contains(&m.id().to_string()),
            "{} kept its ID",
            m.name()
        );
    }
    assert_eq!(folder_ids(&home), [item(&home, "Parts").id().to_string()]);
    // The folder moved with its contents and joined the place.
    let moved = item(&parts, "Brackets");
    let link = moved.as_container_link().expect("a folder");
    assert_eq!(link.place(), Some(PlaceId::new("memory")));
    assert_eq!(link.get_container().expect("opens").items().len(), 1);

    let redo = run(step, true);
    assert_eq!(sorted(ids_and_names(&home)), sorted(before.clone()));
    assert!(parts.items().is_empty());
    let undo = run(redo, false);
    assert_eq!(ids_and_names(&parts).len(), 2);
    let _ = run(undo, true);
    assert_eq!(sorted(ids_and_names(&home)), sorted(before));
}

fn folder_ids(folder: &dyn Folder) -> Vec<String> {
    ids_and_names(folder).into_iter().map(|p| p.0).collect()
}

fn sorted(mut v: Vec<(String, String)>) -> Vec<(String, String)> {
    v.sort();
    v
}

#[test]
fn a_move_into_a_name_taken_numbers_it_and_undo_gives_its_name_back() {
    let home = MemoryFolder::new("Home");
    let parts = home.add_folder("Parts").expect("free");
    parts.add_file("gear.stl", b"old".to_vec());
    let gear = home.add_file("gear.stl", b"new".to_vec());
    let (mut target, mut source) = (parts.clone(), home.clone());
    let step = target
        .move_in_recorded(std::slice::from_ref(&gear), &mut source)
        .step
        .expect("recorded");
    assert_eq!(item(&parts, "gear (2).stl").id(), gear.id());
    let _ = run(step, true);
    assert_eq!(item(&home, "gear.stl").id(), gear.id());
}

#[test]
fn a_folder_cant_move_into_itself() {
    let home = MemoryFolder::new("Home");
    let parts = home.add_folder("Parts").expect("free");
    let inner = parts.add_folder("Inner").expect("free");
    let (mut target, mut source) = (inner.clone(), home.clone());
    let recorded = target.move_in_recorded(&[item(&home, "Parts")], &mut source);
    assert!(recorded.result.is_err());
    assert!(recorded.step.is_none());
    assert_eq!(folder_ids(&home).len(), 1);
}

#[test]
fn a_rename_undoes_and_redoes_with_its_id_kept() {
    let mut home = MemoryFolder::new("Home");
    let gear = home.add_file("gear.stl", b"g".to_vec());
    let step = home
        .rename_recorded(&gear, "cog.stl")
        .step
        .expect("recorded");
    assert_eq!(step.description(), "Renamed gear.stl to cog.stl");
    let redo = run(step, true);
    assert_eq!(item(&home, "gear.stl").id(), gear.id());
    let _ = run(redo, false);
    assert_eq!(item(&home, "cog.stl").id(), gear.id());
}

#[test]
fn a_new_folder_undoes_and_redoes_and_one_already_there_records_nothing() {
    let mut home = MemoryFolder::new("Home");
    let step = home.create_folder_recorded("Parts").step.expect("recorded");
    assert_eq!(step.description(), "Created folder Parts");
    let id = item(&home, "Parts").id().to_string();
    let redo = run(step, true);
    assert!(home.child_containers().is_empty());
    let _ = run(redo, false);
    assert_eq!(item(&home, "Parts").id(), id);
    let again = home.create_folder_recorded("Parts");
    assert!(again.result.is_ok() && again.step.is_none());
}

#[test]
fn a_remove_undoes_with_the_same_entries_and_redoes() {
    let mut home = MemoryFolder::new("Home");
    let parts = home.add_folder("Parts").expect("free");
    parts.add_file("inside.stl", b"i".to_vec());
    let gear = home.add_file("gear.stl", b"g".to_vec());
    let before = sorted(ids_and_names(&home));
    let step = home
        .remove_recorded(&[gear, item(&home, "Parts")])
        .step
        .expect("recorded");
    assert_eq!(step.description(), "Removed 2 items");
    assert!(ids_and_names(&home).is_empty());
    let redo = run(step, true);
    assert_eq!(sorted(ids_and_names(&home)), before);
    let back = item(&home, "Parts")
        .as_container_link()
        .expect("folder")
        .get_container();
    assert_eq!(
        back.expect("opens").items().len(),
        1,
        "its contents came back"
    );
    let _ = run(redo, false);
    assert!(ids_and_names(&home).is_empty());
}
