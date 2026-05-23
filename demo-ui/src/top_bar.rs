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
use agg_gui::widgets::menu::MenuStyle;
use agg_gui::{
    set_visuals, AccentColor, DrawCtx, Event, EventResult, FlexRow, Font, Key,
    MenuBar, MenuEntry, MenuItem, Modifiers, Rect, Size, SizedBox, ThemePreference, TopMenu,
    Visuals, Widget,
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

// ── Mobile-only "Demos" hamburger button ─────────────────────────────────────
//
// Identical in behaviour to the prior `MenuButton` (same name, same
// breakpoint, same toggle target).  Kept as a separate inline widget
// instead of being folded into the menu bar because mobile users expect
// a one-tap hamburger to reveal the demos sidebar — burying it under a
// menu would add a step every time they switch demos.

struct MenuButton {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    visible: bool,
}

impl MenuButton {
    const W: f64 = 92.0;
    const H: f64 = 24.0;
    const MOBILE_BREAKPOINT: f64 = 720.0;

    fn new(font: Arc<Font>, open: Rc<Cell<bool>>) -> Self {
        let open_active = Rc::clone(&open);
        let open_click = open;
        let btn = agg_gui::Button::new("\u{F0C9} Demos", font)
            .with_font_size(12.0)
            .with_subtle()
            .with_outlined()
            .with_active_fn(move || open_active.get())
            .on_click(move || {
                open_click.set(!open_click.get());
                agg_gui::animation::request_draw();
            });
        Self {
            bounds: Rect::default(),
            children: vec![Box::new(btn)],
            visible: false,
        }
    }
}

impl Widget for MenuButton {
    fn type_name(&self) -> &'static str {
        "MenuButton"
    }
    fn is_visible(&self) -> bool {
        self.visible
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }
    fn layout(&mut self, available: Size) -> Size {
        // Report the button's INTRINSIC height (button H + a couple of
        // pixels of breathing room), not `available.height`.  Reporting
        // `available.height` made the row claim the parent's full
        // height — fine when the parent capped that at 36, but
        // `MenuBarStrip` passes the full window height down, and a
        // button claiming the whole window would push the strip's
        // size-to-content logic to fill the screen.
        let intrinsic_h = Self::H + 4.0;
        let slot_h = available.height.min(intrinsic_h);
        self.visible = available.width > 0.0 && available.width < Self::MOBILE_BREAKPOINT;
        if !self.visible {
            self.bounds = Rect::new(0.0, 0.0, 0.0, intrinsic_h);
            return Size::new(0.0, intrinsic_h);
        }
        let w = Self::W + 8.0;
        self.bounds = Rect::new(0.0, 0.0, w, intrinsic_h);
        let gy = ((slot_h - Self::H) * 0.5).max(0.0);
        let child = &mut self.children[0];
        child.layout(Size::new(Self::W, Self::H));
        child.set_bounds(Rect::new(4.0, gy, Self::W, Self::H));
        Size::new(w, intrinsic_h)
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
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
    last_snapshot: Cell<Option<(bool, ThemePreference, AccentColor)>>,
}

impl MenuChrome {
    fn new(
        font: Arc<Font>,
        show_backend: Rc<Cell<bool>>,
        theme_pref: Rc<Cell<ThemePreference>>,
        accent_color: Rc<Cell<AccentColor>>,
    ) -> Self {
        // Action handler captures clones of every state cell so it can
        // mutate the canonical state without any borrow back to the bar
        // itself.  The visual refresh (radio marks etc.) happens on the
        // next `layout()` pass via the snapshot-diff in `refresh_menus`.
        let on_action = {
            let show_backend = Rc::clone(&show_backend);
            let theme_pref = Rc::clone(&theme_pref);
            let accent_color = Rc::clone(&accent_color);
            move |action: &str| {
                handle_action(action, &show_backend, &theme_pref, &accent_color);
                agg_gui::animation::request_draw();
            }
        };
        let initial_menus = build_menus(
            show_backend.get(),
            theme_pref.get(),
            accent_color.get(),
        );
        // CLAUDE.md mandates Font Awesome glyphs throughout the UI.
        // The framework's default `MenuStyle` ships portable Unicode
        // characters (\u{25B8}, \u{2713}, \u{25CF}) for hosts that
        // don't bundle FA; here we swap in the matching FA glyphs so
        // the submenu chevron, checks, and radio marks visually
        // belong with every other icon in the demo.
        let fa_menu_style = MenuStyle {
            submenu_chevron: '\u{F054}',
            check_glyph: '\u{F00C}',
            radio_glyph: '\u{F111}',
            ..MenuStyle::default()
        };
        let bar = MenuBar::new(Arc::clone(&font), initial_menus, on_action)
            .with_font_size(13.0)
            .with_menu_style(fa_menu_style)
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
        );
        if self.last_snapshot.get() == Some(snapshot) {
            return;
        }
        self.last_snapshot.set(Some(snapshot));
        let menus = build_menus(snapshot.0, snapshot.1, snapshot.2);
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
        MenuEntry::Separator,
        MenuItem::submenu("Theme", theme_submenu)
            .icon('\u{F042}')
            .into(),
        MenuItem::submenu("Color", accent_submenu)
            .icon('\u{F53F}')
            .into(),
    ];

    let help_items = vec![
        MenuItem::action("View on GitHub", "help.github")
            .icon('\u{F09B}')
            .into(),
    ];

    vec![
        TopMenu::new("View", view_items),
        TopMenu::new("Help", help_items),
    ]
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
) {
    match action {
        "view.backend" => {
            show_backend.set(!show_backend.get());
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
/// Layout: `[MenuChrome (View / Help)] [flex(1.0) spacer] [Demos hamburger on mobile]`.
/// All chrome that used to live inline (Backend toggle, theme segmented
/// control, "View on GitHub" link, accent dropdown) is now inside the
/// View / Help menus.  The mobile Demos hamburger stays inline — it's a
/// nav drawer trigger, not chrome.
pub fn build_top_bar_inner(
    font: Arc<Font>,
    show_backend: Rc<Cell<bool>>,
    mobile_menu_open: Rc<Cell<bool>>,
    theme_pref: Rc<Cell<ThemePreference>>,
    accent_color: Rc<Cell<AccentColor>>,
) -> Box<dyn Widget> {
    let menu_chrome = MenuChrome::new(
        Arc::clone(&font),
        show_backend,
        theme_pref,
        accent_color,
    );
    Box::new(
        FlexRow::new()
            .with_gap(0.0)
            .add(Box::new(menu_chrome))
            .add_flex(Box::new(SizedBox::new()), 1.0)
            .add(Box::new(MenuButton::new(font, mobile_menu_open))),
    )
}
