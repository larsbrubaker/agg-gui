//! Port of agg-sharp `Tests/Agg.Tests/Agg.UI/MacMenuBarTests.cs`: the parts of
//! the native menu builder that are decisions rather than AppKit calls (which
//! standard chord a role carries, which items make it into a built menu, which
//! item a chord belongs to). Nothing here touches AppKit, so it runs on any OS
//! against `agg_gui_shell::menu_bar`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use agg_gui_shell::menu_bar::{
    children_of, is_enabled, key_equivalent_for, match_key_equivalent, modifier_flags,
    top_level_menus, visible_items, MenuBarModel, MenuItemModel, MenuItemRole,
};

fn item(text: &str) -> MenuItemModel {
    MenuItemModel::new(text)
}

fn with_role(text: &str, role: MenuItemRole) -> MenuItemModel {
    MenuItemModel { role, ..item(text) }
}

fn submenu(text: &str, provider: impl Fn() -> Vec<MenuItemModel> + 'static) -> MenuItemModel {
    MenuItemModel {
        sub_menu_items: Some(Rc::new(provider)),
        ..item(text)
    }
}

fn gate(f: impl Fn() -> bool + 'static) -> Option<Rc<dyn Fn() -> bool>> {
    Some(Rc::new(f))
}

#[test]
fn roles_carry_their_standard_chords() {
    assert_eq!(key_equivalent_for(MenuItemRole::Settings), ",");
    assert_eq!(key_equivalent_for(MenuItemRole::Quit), "q");
    assert_eq!(key_equivalent_for(MenuItemRole::OpenFile), "o");
}

#[test]
fn roles_without_a_standard_chord_get_none() {
    // About and Help have conventional positions but no conventional shortcut,
    // and an item with no role must never be given one.
    assert_eq!(key_equivalent_for(MenuItemRole::None), "");
    assert_eq!(key_equivalent_for(MenuItemRole::About), "");
    assert_eq!(key_equivalent_for(MenuItemRole::Help), "");
}

#[test]
fn visibility_gates_are_evaluated_when_the_menu_is_built() {
    let show_optional = Rc::new(Cell::new(false));
    let shown = show_optional.clone();
    let items = vec![
        item("Always"),
        MenuItemModel {
            is_visible: gate(move || shown.get()),
            ..item("Optional")
        },
        MenuItemModel {
            is_visible: gate(|| false),
            ..item("Never")
        },
    ];

    let first = visible_items(Some(&items));
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].text, "Always");

    // The same model, a second opening, a different answer.
    show_optional.set(true);

    let second = visible_items(Some(&items));
    assert_eq!(second.len(), 2);
    assert_eq!(second[1].text, "Optional");
}

#[test]
fn an_empty_or_missing_list_is_no_items() {
    assert_eq!(visible_items(None).len(), 0);
    assert_eq!(visible_items(Some(&[])).len(), 0);
}

/// The menu bar draws no dividers, so a separator among the top level menus has
/// nowhere to go. Its gates still apply like any other entry's.
#[test]
fn the_menu_bar_takes_visible_non_separator_menus_only() {
    let menus = vec![
        item("File"),
        MenuItemModel::separator(),
        MenuItemModel {
            is_visible: gate(|| false),
            ..item("Hidden")
        },
        MenuItemModel {
            is_enabled: gate(|| false),
            ..item("Off")
        },
    ];

    let top_level = top_level_menus(&menus);

    assert_eq!(top_level.len(), 2);
    assert_eq!(top_level[0].text, "File");
    assert_eq!(top_level[1].text, "Off");

    // A disabled menu is still a menu in the bar - drawn greyed, not dropped.
    assert!(is_enabled(&top_level[0]));
    assert!(!is_enabled(&top_level[1]));
}

/// A native menu's children are asked for again every time it is about to
/// open, so a list that grew while the application ran opens showing it.
#[test]
fn asking_for_children_runs_the_provider_each_time() {
    let recents = Rc::new(RefCell::new(vec![item("First")]));
    let times_asked = Rc::new(Cell::new(0));
    let (provided, asked) = (recents.clone(), times_asked.clone());
    let container = submenu("Open Recent", move || {
        asked.set(asked.get() + 1);
        provided.borrow().clone()
    });

    assert_eq!(children_of(&container).len(), 1);

    recents.borrow_mut().push(item("Second"));

    let reopened = children_of(&container);
    assert_eq!(reopened.len(), 2);
    assert_eq!(reopened[1].text, "Second");
    assert_eq!(times_asked.get(), 2);
}

#[test]
fn an_item_that_describes_no_children_has_none() {
    // A leaf, and a submenu whose provider came back empty: the model owns any
    // "nothing here" placeholder it wants shown.
    assert_eq!(children_of(&item("Leaf")).len(), 0);
    assert_eq!(children_of(&submenu("", Vec::new)).len(), 0);
}

#[test]
fn an_ungated_item_is_enabled() {
    assert!(is_enabled(&item("Plain")));
    assert!(!is_enabled(&MenuItemModel {
        is_enabled: gate(|| false),
        ..MenuItemModel::default()
    }));
    assert!(is_enabled(&MenuItemModel {
        is_enabled: gate(|| true),
        ..MenuItemModel::default()
    }));
}

type Gate = Option<Rc<dyn Fn() -> bool>>;
type Provider = Option<Rc<dyn Fn() -> Vec<MenuItemModel>>>;

/// A bar shaped like MatterCAD's: an application menu carrying Settings and
/// Quit, a File menu carrying Open, and a Help menu whose entries carry no
/// chord. `menu_asked` counts every time one of the three menus is asked for
/// its contents.
fn sample_menu_bar(
    settings_enabled: Gate,
    open_visible: Gate,
    recent_files: Provider,
    menu_asked: Option<Rc<Cell<i32>>>,
) -> MenuBarModel {
    let counted = move |items: Vec<MenuItemModel>, asked: &Option<Rc<Cell<i32>>>| {
        if let Some(asked) = asked {
            asked.set(asked.get() + 1);
        }
        items
    };
    let (asked_app, asked_file, asked_help) = (menu_asked.clone(), menu_asked.clone(), menu_asked);
    let recent_files: Rc<dyn Fn() -> Vec<MenuItemModel>> =
        recent_files.unwrap_or_else(|| Rc::new(Vec::new));
    MenuBarModel {
        menus: vec![
            submenu("App", move || {
                counted(
                    vec![
                        with_role("About", MenuItemRole::About),
                        MenuItemModel {
                            is_enabled: settings_enabled.clone(),
                            ..with_role("Settings", MenuItemRole::Settings)
                        },
                        MenuItemModel::separator(),
                        with_role("Quit", MenuItemRole::Quit),
                    ],
                    &asked_app,
                )
            }),
            submenu("File", move || {
                counted(
                    vec![
                        MenuItemModel {
                            is_visible: open_visible.clone(),
                            ..with_role("Open", MenuItemRole::OpenFile)
                        },
                        MenuItemModel {
                            sub_menu_items: Some(recent_files.clone()),
                            ..item("Open Recent")
                        },
                    ],
                    &asked_file,
                )
            }),
            submenu("Help", move || {
                counted(vec![with_role("Help", MenuItemRole::Help)], &asked_help)
            }),
        ],
    }
}

fn plain_sample() -> MenuBarModel {
    sample_menu_bar(None, None, None, None)
}

fn matched(bar: &MenuBarModel, characters: &str, extra_modifiers: u64) -> Option<String> {
    match_key_equivalent(
        Some(bar),
        characters,
        modifier_flags::COMMAND | extra_modifiers,
    )
    .map(|item| item.text)
}

fn matches(bar: &MenuBarModel, characters: &str) -> Option<String> {
    matched(bar, characters, 0)
}

#[test]
fn a_command_chord_finds_the_item_whose_role_carries_it() {
    let bar = plain_sample();

    assert_eq!(matches(&bar, "o").as_deref(), Some("Open"));
    assert_eq!(matches(&bar, ",").as_deref(), Some("Settings"));
    assert_eq!(matches(&bar, "q").as_deref(), Some("Quit"));
}

#[test]
fn a_chord_no_item_claims_matches_nothing() {
    let bar = plain_sample();

    // Cmd-9 belongs to no role, Cmd-C is one the application handles itself:
    // both come back empty rather than landing on the first item.
    assert_eq!(matches(&bar, "9"), None);
    assert_eq!(matches(&bar, "c"), None);
    assert_eq!(matches(&bar, ""), None);
    assert!(match_key_equivalent(None, "o", modifier_flags::COMMAND).is_none());
}

/// A role's chord is Command and the key: Command with another modifier is a
/// different chord, and a key with no Command at all is ordinary typing.
#[test]
fn only_a_plain_command_chord_matches() {
    let bar = plain_sample();

    assert!(match_key_equivalent(Some(&bar), "o", 0).is_none());
    assert_eq!(matched(&bar, "o", modifier_flags::SHIFT), None);
    assert_eq!(matched(&bar, "o", modifier_flags::OPTION), None);
    assert_eq!(matched(&bar, "o", modifier_flags::CONTROL), None);

    // Caps lock is not part of a chord's identity, and with it down the layout
    // spells the key in upper case.
    assert_eq!(
        matched(&bar, "O", modifier_flags::CAPS_LOCK).as_deref(),
        Some("Open")
    );
}

#[test]
fn a_hidden_or_disabled_item_cannot_be_reached_by_its_chord() {
    let settings_enabled = Rc::new(Cell::new(false));
    let open_visible = Rc::new(Cell::new(false));
    let (enabled, visible) = (settings_enabled.clone(), open_visible.clone());

    let bar = sample_menu_bar(
        gate(move || enabled.get()),
        gate(move || visible.get()),
        None,
        None,
    );

    // Greyed out in the menu, so unreachable by keyboard too.
    assert_eq!(matches(&bar, ","), None);
    assert_eq!(matches(&bar, "o"), None);

    settings_enabled.set(true);
    open_visible.set(true);

    assert_eq!(matches(&bar, ",").as_deref(), Some("Settings"));
    assert_eq!(matches(&bar, "o").as_deref(), Some("Open"));
}

/// Matching answers without asking the expensive providers: a submenu's
/// contents are only gathered when that submenu is about to be shown.
#[test]
fn matching_never_opens_a_submenu() {
    let recent_files_asked = Rc::new(Cell::new(0));
    let asked = recent_files_asked.clone();

    let bar = sample_menu_bar(
        None,
        None,
        Some(Rc::new(move || {
            asked.set(asked.get() + 1);
            Vec::new()
        })),
        None,
    );

    assert_eq!(matches(&bar, "o").as_deref(), Some("Open"));
    assert_eq!(matches(&bar, "9"), None);

    assert_eq!(recent_files_asked.get(), 0);
}

/// A chord no role could carry is turned away before the menus are so much as
/// enumerated.
#[test]
fn a_chord_no_role_carries_costs_no_provider_at_all() {
    let menus_asked = Rc::new(Cell::new(0));

    let bar = sample_menu_bar(None, None, None, Some(menus_asked.clone()));

    assert_eq!(matches(&bar, "9"), None);
    assert_eq!(matches(&bar, "c"), None);
    assert_eq!(matches(&bar, "v"), None);
    assert_eq!(menus_asked.get(), 0);

    // And a chord a role does carry still reaches them.
    assert_eq!(matches(&bar, "o").as_deref(), Some("Open"));
    assert!(menus_asked.get() > 0);
}

/// The search goes one level down and no further, the only depth the builder
/// draws a shortcut at.
#[test]
fn a_role_item_nested_deeper_is_not_matched() {
    let submenu_asked = Rc::new(Cell::new(0));
    let asked = submenu_asked.clone();

    let bar = MenuBarModel {
        menus: vec![submenu("File", move || {
            let asked = asked.clone();
            vec![submenu("Open Recent", move || {
                asked.set(asked.get() + 1);
                vec![with_role("Buried Open", MenuItemRole::OpenFile)]
            })]
        })],
    };

    assert_eq!(matches(&bar, "o"), None);
    assert_eq!(submenu_asked.get(), 0);
}

/// Two items claiming one chord is a model bug, but the matcher stays
/// predictable: menu order then item order, first one wins.
#[test]
fn the_first_item_to_claim_a_chord_gets_it() {
    let bar = MenuBarModel {
        menus: vec![
            submenu("File", || {
                vec![
                    with_role("Open", MenuItemRole::OpenFile),
                    with_role("Open Again", MenuItemRole::OpenFile),
                ]
            }),
            submenu("Elsewhere", || {
                vec![with_role("Open Elsewhere", MenuItemRole::OpenFile)]
            }),
        ],
    };

    assert_eq!(matches(&bar, "o").as_deref(), Some("Open"));
}
