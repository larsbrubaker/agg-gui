//! Top-bar chrome: a real menu bar (View / Help) plus the mobile-only
//! Demos hamburger.
//!
//! Replaces the older row of inline chrome buttons (Backend / Theme
//! toggle / Color dropdown / "View on GitHub") with a desktop-style
//! `MenuBar`.  All those controls now live inside the View and Help
//! menus.  The mobile hamburger that opens the demos sidebar is kept
//! separate — it's a navigation drawer trigger, not chrome, and folding
//! it into a menu makes the mobile flow noticeably worse.
//!
//! Exports:
//! - `detect_system_theme` / `apply_theme_visuals` — unchanged helpers
//!   called by `app_builder.rs` during startup.
//! - `build_top_bar_inner` — builds the FlexRow that fills the
//!   `TopMenuBar`.  Signature kept identical to the previous
//!   implementation so `app_builder.rs` doesn't need to change.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::widget::{BackbufferCache, BackbufferMode};
use agg_gui::{
    set_visuals, AccentColor, DrawCtx, Event, EventResult, FlexRow, Font, Key, MenuBar, MenuEntry,
    MenuItem, Modifiers, Rect, Size, SizedBox, ThemePreference, TopMenu, Visuals, Widget,
};

// ── Theme helpers (re-exported from the previous module) ─────────────────────

/// Detect OS colour scheme and return the matching `ThemePreference`.
pub fn detect_system_theme() -> ThemePreference {
    match dark_light::detect() {
        dark_light::Mode::Light | dark_light::Mode::Default => ThemePreference::Light,
        dark_light::Mode::Dark => ThemePreference::Dark,
    }
}

/// Apply visuals matching the selected theme and accent swatch.
pub fn apply_theme_visuals(pref: ThemePreference, accent: AccentColor) {
    let base = match pref {
        ThemePreference::Light => Visuals::light(),
        ThemePreference::Dark => Visuals::dark(),
        ThemePreference::System => match detect_system_theme() {
            ThemePreference::Light => Visuals::light(),
            _ => Visuals::dark(),
        },
    };
    set_visuals(base.with_accent_color(accent));
}

// ── Demos menu data ──────────────────────────────────────────────────────────
//
// The "Demos" top-level menu mirrors the sidebar: one submenu per group, one
// leaf per demo.  The host (`app_builder`) hands us the same grouped
// (label, open-cell) pairs the sidebar is built from, so the two launchers
// stay in lock-step.  Earlier this slot held a mobile-only hamburger that
// toggled the sidebar drawer; it's been replaced by this real dropdown so a
// phone user can open any demo directly from the menu bar instead of paging
// through a drawer.
pub type DemoMenuGroups = Vec<(&'static str, Vec<(String, Rc<Cell<bool>>)>)>;

/// Build the `Demos` top-level menu's entries from the grouped demo list, plus
/// a flat index → open-cell table the action handler uses to open the picked
/// demo.  Action ids are `demo.<flat-index>`; the index is just this demo's
/// position in the returned `cells` vector.
fn build_demos_menu(groups: &DemoMenuGroups) -> (Vec<MenuEntry>, Vec<Rc<Cell<bool>>>) {
    let mut cells: Vec<Rc<Cell<bool>>> = Vec::new();
    let mut top_items: Vec<MenuEntry> = Vec::new();
    for (group_name, demos) in groups {
        if demos.is_empty() {
            continue;
        }
        let mut sub: Vec<MenuEntry> = Vec::with_capacity(demos.len());
        for (label, cell) in demos {
            let idx = cells.len();
            cells.push(Rc::clone(cell));
            // Labels already carry their Font Awesome glyph prefix, so the
            // text column renders the icon inline — no separate `.icon()`.
            sub.push(MenuItem::action(label.clone(), format!("demo.{idx}")).into());
        }
        top_items.push(MenuItem::submenu(*group_name, sub).icon('\u{F009}').into());
    }
    (top_items, cells)
}

// ── Menu chrome wrapper ──────────────────────────────────────────────────────

/// URL of the project on GitHub — opened by the Help → "View on GitHub" action.
/// Single source of truth so the README badge and the in-app link stay in
/// sync.
const GITHUB_URL: &str = "https://github.com/larsbrubaker/agg-gui";

/// Wraps an `agg_gui::MenuBar` so it can be hosted directly in the top bar
/// while its menus are kept in lock-step with the canonical app-state
/// cells (`show_backend`, `theme_pref`, `accent_color`).
///
/// Why a wrapper instead of putting `MenuBar` straight into the FlexRow:
/// `MenuItem::checked` / `MenuItem::radio` are baked into the item tree
/// at construction time.  When the user picks a theme, only the app
/// state changes — the popup's check/radio marks would lag behind until
/// the bar was rebuilt.  This wrapper diffs the cells against a cached
/// snapshot in `layout` and calls [`MenuBar::set_menus`] whenever the
/// snapshot moves, so the popup always paints the current selection.
///
/// The inner bar is held as a concrete field (not as a child in the
/// widget tree) so we can call `set_menus` on it; in exchange the
/// wrapper forwards every Widget hook the menu bar relies on
/// (`layout` / `paint` / events / global overlay / shortcuts / cache).
struct MenuChrome {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>, // intentionally empty — see struct comment
    bar: MenuBar,
    show_backend: Rc<Cell<bool>>,
    theme_pref: Rc<Cell<ThemePreference>>,
    accent_color: Rc<Cell<AccentColor>>,
    snap_enabled: Rc<Cell<bool>>,
    /// Prebuilt `Demos` menu entries (one submenu per group).  The demo set is
    /// fixed for the app's lifetime, so this is cloned into a fresh `TopMenu`
    /// on every `refresh_menus` rather than rebuilt.
    demos_items: Vec<MenuEntry>,
    last_snapshot: Cell<Option<(bool, ThemePreference, AccentColor, bool)>>,
}

impl MenuChrome {
    fn new(
        font: Arc<Font>,
        show_backend: Rc<Cell<bool>>,
        theme_pref: Rc<Cell<ThemePreference>>,
        accent_color: Rc<Cell<AccentColor>>,
        snap_enabled: Rc<Cell<bool>>,
        demo_menu_groups: DemoMenuGroups,
    ) -> Self {
        let (demos_items, demo_cells) = build_demos_menu(&demo_menu_groups);
        // Action handler captures clones of every state cell so it can
        // mutate the canonical state without any borrow back to the bar
        // itself.  The visual refresh (radio marks etc.) happens on the
        // next `layout()` pass via the snapshot-diff in `refresh_menus`.
        let on_action = {
            let show_backend = Rc::clone(&show_backend);
            let theme_pref = Rc::clone(&theme_pref);
            let accent_color = Rc::clone(&accent_color);
            let snap_enabled = Rc::clone(&snap_enabled);
            move |action: &str| {
                if let Some(idx) = action.strip_prefix("demo.") {
                    // Demos menu: open the picked demo's window directly.
                    if let Some(cell) = idx.parse::<usize>().ok().and_then(|i| demo_cells.get(i)) {
                        cell.set(true);
                    }
                } else {
                    handle_action(
                        action,
                        &show_backend,
                        &theme_pref,
                        &accent_color,
                        &snap_enabled,
                    );
                }
                agg_gui::animation::request_draw();
            }
        };
        let initial_menus = compose_menus(
            show_backend.get(),
            theme_pref.get(),
            accent_color.get(),
            snap_enabled.get(),
            &demos_items,
        );
        let bar = MenuBar::new(Arc::clone(&font), initial_menus, on_action)
            .with_font_size(13.0)
            // Tight width — the FlexRow that hosts us spans the whole
            // top bar and has a flexing spacer plus the mobile Demos
            // button on its right; without this the bar would claim
            // every spare pixel and squash its siblings.
            .with_fit_width(true);
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            bar,
            show_backend,
            theme_pref,
            accent_color,
            snap_enabled,
            demos_items,
            last_snapshot: Cell::new(None),
        }
    }

    /// Rebuild the menu list if any backing cell changed since the last
    /// rebuild.  Cheap: when nothing changed, just an early return.
    fn refresh_menus(&mut self) {
        let snapshot = (
            self.show_backend.get(),
            self.theme_pref.get(),
            self.accent_color.get(),
            self.snap_enabled.get(),
        );
        if self.last_snapshot.get() == Some(snapshot) {
            return;
        }
        self.last_snapshot.set(Some(snapshot));
        let menus = compose_menus(
            snapshot.0,
            snapshot.1,
            snapshot.2,
            snapshot.3,
            &self.demos_items,
        );
        self.bar.set_menus(menus);
    }
}

impl Widget for MenuChrome {
    fn type_name(&self) -> &'static str {
        "MenuChrome"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
        self.bar.set_bounds(Rect::new(0.0, 0.0, b.width, b.height));
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }
    fn layout(&mut self, available: Size) -> Size {
        self.refresh_menus();
        let used = self.bar.layout(available);
        self.bounds = Rect::new(0.0, 0.0, used.width, used.height);
        self.bar
            .set_bounds(Rect::new(0.0, 0.0, used.width, used.height));
        used
    }
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        // The bar paints its own background + bar buttons at the
        // wrapper's origin — wrapper bounds == bar bounds, so no
        // translate needed.
        self.bar.paint(ctx);
    }
    fn paint_global_overlay(&mut self, ctx: &mut dyn DrawCtx) {
        self.bar.paint_global_overlay(ctx);
    }
    fn hit_test_global_overlay(&self, local_pos: agg_gui::Point) -> bool {
        self.bar.hit_test_global_overlay(local_pos)
    }
    fn has_active_modal(&self) -> bool {
        self.bar.has_active_modal()
    }
    fn on_event(&mut self, event: &Event) -> EventResult {
        self.bar.on_event(event)
    }
    fn on_unconsumed_key(&mut self, key: &Key, modifiers: Modifiers) -> EventResult {
        self.bar.on_unconsumed_key(key, modifiers)
    }
    fn backbuffer_cache_mut(&mut self) -> Option<&mut BackbufferCache> {
        // Forward to the inner bar so the wrapper transparently
        // inherits the bar's hover/open re-raster behaviour.
        self.bar.backbuffer_cache_mut()
    }
    fn backbuffer_mode(&self) -> BackbufferMode {
        // Mirror the bar's LCD-vs-RGBA decision.  See
        // `MenuBar::backbuffer_mode`.
        self.bar.backbuffer_mode()
    }
}

// ── Menu list construction ───────────────────────────────────────────────────

/// Build the `View` + `Help` top-level menu lists with check / radio
/// marks set from the supplied state snapshot.
///
/// Action ids follow a `view.<group>.<value>` / `help.<value>` scheme
/// — see `handle_action` for the routing table.
fn build_menus(
    backend_open: bool,
    theme_pref: ThemePreference,
    accent: AccentColor,
    snap_enabled: bool,
) -> Vec<TopMenu> {
    // `.radio()` (mutex) over `.checked()` (toggle) so the popup's
    // toggle handler clears sibling selections in-place when the user
    // picks a new theme without closing the menu.
    let theme_submenu = vec![
        MenuItem::action("Light", "view.theme.light")
            .icon('\u{F185}')
            .radio(theme_pref == ThemePreference::Light)
            .keep_open()
            .into(),
        MenuItem::action("Dark", "view.theme.dark")
            .icon('\u{F186}')
            .radio(theme_pref == ThemePreference::Dark)
            .keep_open()
            .into(),
        MenuItem::action("System", "view.theme.system")
            .icon('\u{F108}')
            .radio(theme_pref == ThemePreference::System)
            .keep_open()
            .into(),
    ];

    let accent_submenu: Vec<MenuEntry> = AccentColor::ALL
        .iter()
        .map(|a| {
            MenuItem::action(a.label(), format!("view.accent.{}", a.key()))
                .swatch(a.color())
                .radio(accent == *a)
                .keep_open()
                .into()
        })
        .collect();

    let view_items = vec![
        MenuItem::action("Backend Panel", "view.backend")
            .icon('\u{F109}')
            .checked(backend_open)
            .keep_open()
            .into(),
        MenuItem::action("Window Snapping", "view.snap")
            // FA `\u{F076}` = th-large, the closest stock icon for a
            // grid-aligned layout aid.
            .icon('\u{F076}')
            .checked(snap_enabled)
            .keep_open()
            .into(),
        MenuEntry::Separator,
        MenuItem::submenu("Theme", theme_submenu)
            .icon('\u{F042}')
            .into(),
        MenuItem::submenu("Color", accent_submenu)
            .icon('\u{F53F}')
            .into(),
    ];

    let help_items = vec![MenuItem::action("View on GitHub", "help.github")
        .icon('\u{F09B}')
        .into()];

    vec![
        TopMenu::new("View", view_items),
        TopMenu::new("Help", help_items),
    ]
}

/// Assemble the full bar: `Demos` (data-driven, prebuilt) followed by the
/// state-driven `View` / `Help` menus.  `Demos` is leftmost because it's the
/// primary navigation — opening a demo is the most common bar action.
fn compose_menus(
    backend_open: bool,
    theme_pref: ThemePreference,
    accent: AccentColor,
    snap_enabled: bool,
    demos_items: &[MenuEntry],
) -> Vec<TopMenu> {
    let mut menus = Vec::with_capacity(3);
    if !demos_items.is_empty() {
        menus.push(TopMenu::new("\u{F009} Demos", demos_items.to_vec()));
    }
    menus.extend(build_menus(backend_open, theme_pref, accent, snap_enabled));
    menus
}

/// Dispatch a menu action id back into a mutation of the relevant state
/// cell.  Unknown ids are ignored — the menu list is the only producer
/// of these strings, so any miss is a build error rather than a runtime
/// concern.
fn handle_action(
    action: &str,
    show_backend: &Rc<Cell<bool>>,
    theme_pref: &Rc<Cell<ThemePreference>>,
    accent_color: &Rc<Cell<AccentColor>>,
    snap_enabled: &Rc<Cell<bool>>,
) {
    match action {
        "view.backend" => {
            show_backend.set(!show_backend.get());
        }
        "view.snap" => {
            let new = !snap_enabled.get();
            snap_enabled.set(new);
            // Mirror into the framework's thread-local so every
            // Snappable widget picks up the change on the next drag
            // without having to thread the cell through their APIs.
            agg_gui::snap::set_enabled(new);
        }
        "view.theme.light" => set_theme(theme_pref, accent_color, ThemePreference::Light),
        "view.theme.dark" => set_theme(theme_pref, accent_color, ThemePreference::Dark),
        "view.theme.system" => set_theme(theme_pref, accent_color, ThemePreference::System),
        "help.github" => crate::url::open_url(GITHUB_URL),
        other if other.starts_with("view.accent.") => {
            let key = &other["view.accent.".len()..];
            if let Some(accent) = AccentColor::from_key(key) {
                accent_color.set(accent);
                apply_theme_visuals(theme_pref.get(), accent);
            }
        }
        _ => {}
    }
}

fn set_theme(
    theme_pref: &Rc<Cell<ThemePreference>>,
    accent_color: &Rc<Cell<AccentColor>>,
    pref: ThemePreference,
) {
    theme_pref.set(pref);
    apply_theme_visuals(pref, accent_color.get());
}

// ── Factory ───────────────────────────────────────────────────────────────────

/// Build the FlexRow child for `TopMenuBar`.
///
/// Layout: `[MenuChrome (Demos / View / Help)] [flex(1.0) spacer]`.
/// All chrome that used to live inline (Backend toggle, theme segmented
/// control, "View on GitHub" link, accent dropdown) is inside the View / Help
/// menus, and the mobile demos hamburger is now the `Demos` dropdown inside
/// the bar (see [`build_demos_menu`]).  `mobile_menu_open` is retained in the
/// signature for the sidebar drawer state but no longer toggled from here.
pub fn build_top_bar_inner(
    font: Arc<Font>,
    show_backend: Rc<Cell<bool>>,
    _mobile_menu_open: Rc<Cell<bool>>,
    theme_pref: Rc<Cell<ThemePreference>>,
    accent_color: Rc<Cell<AccentColor>>,
    snap_enabled: Rc<Cell<bool>>,
    demo_menu_groups: DemoMenuGroups,
) -> Box<dyn Widget> {
    let menu_chrome = MenuChrome::new(
        Arc::clone(&font),
        show_backend,
        theme_pref,
        accent_color,
        snap_enabled,
        demo_menu_groups,
    );
    Box::new(
        FlexRow::new()
            .with_gap(0.0)
            .add(Box::new(menu_chrome))
            .add_flex(Box::new(SizedBox::new()), 1.0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(v: bool) -> Rc<Cell<bool>> {
        Rc::new(Cell::new(v))
    }

    fn item(entry: &MenuEntry) -> &MenuItem {
        match entry {
            MenuEntry::Item(it) => it,
            MenuEntry::Separator => panic!("expected a menu item, found a separator"),
        }
    }

    #[test]
    fn demos_menu_groups_and_indexes_every_demo() {
        let (a, b, c) = (cell(false), cell(false), cell(false));
        let groups: DemoMenuGroups = vec![
            (
                "Widgets",
                vec![("Button".into(), Rc::clone(&a)), ("Slider".into(), Rc::clone(&b))],
            ),
            ("Empty", vec![]), // skipped — no leaves
            ("Layout", vec![("Flex".into(), Rc::clone(&c))]),
        ];

        let (items, cells) = build_demos_menu(&groups);

        // Empty groups are dropped; the flat cell table follows group-then-entry
        // order so `demo.<i>` indexes straight into it.
        assert_eq!(items.len(), 2, "only non-empty groups become submenus");
        assert_eq!(cells.len(), 3);
        assert!(Rc::ptr_eq(&cells[0], &a));
        assert!(Rc::ptr_eq(&cells[1], &b));
        assert!(Rc::ptr_eq(&cells[2], &c));

        let widgets = item(&items[0]);
        assert_eq!(widgets.label, "Widgets");
        assert_eq!(widgets.submenu.len(), 2);
        assert_eq!(item(&widgets.submenu[0]).action.as_deref(), Some("demo.0"));
        assert_eq!(item(&widgets.submenu[1]).action.as_deref(), Some("demo.1"));

        let layout = item(&items[1]);
        assert_eq!(layout.label, "Layout");
        assert_eq!(item(&layout.submenu[0]).action.as_deref(), Some("demo.2"));
    }

    #[test]
    fn compose_menus_puts_demos_before_view_and_help() {
        let groups: DemoMenuGroups = vec![("Widgets", vec![("Button".into(), cell(false))])];
        let (items, _cells) = build_demos_menu(&groups);
        let menus = compose_menus(
            false,
            ThemePreference::Dark,
            AccentColor::ALL[0],
            false,
            &items,
        );
        let labels: Vec<&str> = menus.iter().map(|m| m.label.as_str()).collect();
        assert_eq!(labels.len(), 3);
        assert!(
            labels[0].contains("Demos"),
            "Demos must be the leftmost menu, got {labels:?}"
        );
        assert!(labels.iter().any(|l| *l == "View"));
        assert!(labels.iter().any(|l| *l == "Help"));
    }

    #[test]
    fn compose_menus_omits_demos_when_empty() {
        let menus = compose_menus(false, ThemePreference::Dark, AccentColor::ALL[0], false, &[]);
        let labels: Vec<&str> = menus.iter().map(|m| m.label.as_str()).collect();
        assert_eq!(labels, vec!["View", "Help"]);
    }
}
