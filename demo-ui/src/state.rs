//! Persistent demo state — window open flags and positions.
//!
//! Used to restore window layout between native restarts and WASM refreshes.
//! Serialization is a simple CSV format (no external deps):
//!   version=1
//!   demos=<count>
//!   tests=<count>
//!   d<i>=<open>,<x>,<y>,<w>,<h>,<maximized>
//!   t<i>=<open>,<x>,<y>,<w>,<h>,<maximized>
//!   about=<open>,<x>,<y>,<w>,<h>,<maximized>

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use agg_gui::{AccentColor, Rect, ThemePreference};

// ── WindowState ───────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct WindowState {
    pub open: bool,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    /// Whether this floating demo window was maximized within the canvas.
    pub maximized: bool,
}

impl WindowState {
    pub fn to_rect(&self) -> Rect {
        Rect::new(self.x, self.y, self.w, self.h)
    }

    /// True when width and height are both positive.  A never-laid-out window
    /// (sidebar entry never toggled visible) has its position cell stuck at
    /// `Rect::default()` = (0,0,0,0); persisting that would stretch the
    /// window to fill the canvas on restore, because `Window::set_bounds`
    /// treats zero-size bounds as "uninitialised, take parent's rect".
    pub fn has_valid_bounds(&self) -> bool {
        self.w > 0.0 && self.h > 0.0
    }
}

// ── SavedState ────────────────────────────────────────────────────────────────

pub struct SavedState {
    pub demos: Vec<WindowState>,
    pub tests: Vec<WindowState>,
    pub about: WindowState,
    /// Whether the left-side Backend panel is open.
    pub backend_open: bool,
    /// Top-bar theme selection.
    pub theme_pref: ThemePreference,
    /// Top-bar accent swatch selection.
    pub accent_color: AccentColor,
    /// OS-window logical width (in pixels).  `None` leaves the host default.
    pub window_w: Option<u32>,
    /// OS-window logical height (in pixels).
    pub window_h: Option<u32>,
    /// Whether the OS window was borderless-fullscreen when last saved.
    pub window_fullscreen: bool,
    /// Whether the OS window was maximized when last saved.  Independent of
    /// fullscreen — fullscreen wins on restore when both are true.
    pub window_maximized: bool,
    /// Inspector UI state — expanded tree nodes + selected node + split-bar.
    pub inspector: Option<InspectorPersist>,

    // ── System-window persistence ──────────────────────────────────────
    //
    // These mirror `agg_gui::font_settings` so the user's typography
    // choices survive across runs.  `font_name` matches one of the
    // demo-ui `FONT_OPTIONS` display names (e.g. "Cascadia Code") — we
    // persist the name, not an `Arc<Font>`, and re-load the bytes on
    // next startup from the bundled assets.
    /// Selected font's display name.  `None` or unknown name → keep the
    /// app default (Cascadia Code).
    pub font_name: Option<String>,
    /// Font-size multiplier applied system-wide.  Default 1.0.
    pub font_size_scale: f64,
    /// LCD subpixel rendering toggle.
    pub lcd_enabled: bool,
    /// Hinting toggle (Y-axis baseline snap).
    pub hinting_enabled: bool,
    /// Output gamma curve.  1.0 = off.
    pub gamma: f64,
    /// Horizontal glyph width scale.
    pub width_scale: f64,
    /// Extra letter-spacing as fraction of em.
    pub interval: f64,
    /// Faux-weight offset (synthetic bold).
    pub faux_weight: f64,
    /// Faux-italic shear factor.
    pub faux_italic: f64,
    /// LCD primary-weight tap ratio.
    pub primary_weight: f64,

    /// OS GL-surface MSAA sample count.  0 = off (halo-AA does all work);
    /// 2/4/8/16 request hardware multisampling at context creation.
    /// Change takes effect on next launch.
    pub msaa_samples: u8,

    /// Active tab index inside the System window (`Font` = 0, `Render` = 1).
    /// Defaults to 0 on first run so new users land on the Font settings.
    pub system_tab: usize,

    /// Window z-order — list of titles (DEMOS / TESTS / About) in
    /// **back-to-front** order.  Recorded each time a window is raised
    /// (click-to-front or sidebar rising-edge).  Empty / missing on
    /// first run; on restore, used to physically reorder the canvas
    /// `Stack`'s children so the user's last "this window was on top"
    /// choice survives across sessions.  Titles not present in the
    /// saved list keep their default DEMOS/TESTS-array position
    /// (rendered behind the persisted ones).
    pub z_order: Vec<String>,
}

/// Persisted inspector UI state.  Flat bit-vector of expanded nodes in DFS
/// order + optional selected-node index + properties-pane height.
#[derive(Clone, Debug, Default)]
pub struct InspectorPersist {
    pub expanded: Vec<bool>,
    pub selected: Option<usize>,
    pub props_h: f64,
    /// Whether the Inspector window itself was visible at save time.
    pub open: bool,
}

impl SavedState {
    pub fn serialize(&self) -> String {
        let mut out = String::new();
        out.push_str("version=1\n");
        out.push_str(&format!("demos={}\n", self.demos.len()));
        out.push_str(&format!("tests={}\n", self.tests.len()));
        for (i, w) in self.demos.iter().enumerate() {
            out.push_str(&format!(
                "d{}={},{},{},{},{},{}\n",
                i, w.open as u8, w.x, w.y, w.w, w.h, w.maximized as u8
            ));
        }
        for (i, w) in self.tests.iter().enumerate() {
            out.push_str(&format!(
                "t{}={},{},{},{},{},{}\n",
                i, w.open as u8, w.x, w.y, w.w, w.h, w.maximized as u8
            ));
        }
        out.push_str(&format!(
            "about={},{},{},{},{},{}\n",
            self.about.open as u8,
            self.about.x,
            self.about.y,
            self.about.w,
            self.about.h,
            self.about.maximized as u8
        ));
        out.push_str(&format!("backend={}\n", self.backend_open as u8));
        out.push_str(&format!("theme={}\n", self.theme_pref.key()));
        out.push_str(&format!("accent={}\n", self.accent_color.key()));
        if let (Some(w), Some(h)) = (self.window_w, self.window_h) {
            out.push_str(&format!(
                "window={},{},{},{}\n",
                w, h, self.window_fullscreen as u8, self.window_maximized as u8
            ));
        }
        if let Some(insp) = &self.inspector {
            // `inspector=selected,props_h,open;expanded-bits`
            let sel = insp.selected.map(|i| i as i64).unwrap_or(-1);
            let bits: String = insp
                .expanded
                .iter()
                .map(|b| if *b { '1' } else { '0' })
                .collect();
            out.push_str(&format!(
                "inspector={},{},{};{}\n",
                sel, insp.props_h, insp.open as u8, bits
            ));
        }
        // System settings — each on its own key so the parser can add
        // future entries without breaking old state files.
        if let Some(name) = &self.font_name {
            // Font names may contain spaces (e.g. "Cascadia Code") but
            // NEVER '=' / newline, which is the only thing we care about
            // for this line-oriented format.
            out.push_str(&format!("font_name={name}\n"));
        }
        // Z-order: pipe-separated titles in back-to-front order.  Titles
        // never contain '|' or '\n' so this round-trips cleanly without
        // escaping.  Skipped on first run / when no raises happened.
        if !self.z_order.is_empty() {
            out.push_str(&format!("z_order={}\n", self.z_order.join("|")));
        }
        out.push_str(&format!("font_size_scale={}\n", self.font_size_scale));
        out.push_str(&format!("lcd={}\n", self.lcd_enabled as u8));
        out.push_str(&format!("hinting={}\n", self.hinting_enabled as u8));
        out.push_str(&format!("gamma={}\n", self.gamma));
        out.push_str(&format!("width_scale={}\n", self.width_scale));
        out.push_str(&format!("interval={}\n", self.interval));
        out.push_str(&format!("faux_weight={}\n", self.faux_weight));
        out.push_str(&format!("faux_italic={}\n", self.faux_italic));
        out.push_str(&format!("primary_weight={}\n", self.primary_weight));
        out.push_str(&format!("msaa={}\n", self.msaa_samples));
        out.push_str(&format!("system_tab={}\n", self.system_tab));
        out
    }

    pub fn deserialize(s: &str) -> Option<Self> {
        let mut demos_count = None::<usize>;
        let mut tests_count = None::<usize>;
        let mut demos: Vec<Option<WindowState>> = Vec::new();
        let mut tests: Vec<Option<WindowState>> = Vec::new();
        let mut about = None::<WindowState>;
        let mut backend_open = false;
        let mut theme_pref = ThemePreference::System;
        let mut accent_color = AccentColor::default();
        let mut window_w: Option<u32> = None;
        let mut window_h: Option<u32> = None;
        let mut window_fullscreen = false;
        let mut window_maximized = false;
        let mut inspector: Option<InspectorPersist> = None;
        let mut font_name: Option<String> = None;
        let mut font_size_scale: f64 = 1.0;
        let mut lcd_enabled: bool = false;
        let mut hinting_enabled: bool = false;
        let mut gamma: f64 = 1.0;
        let mut width_scale: f64 = 1.0;
        let mut interval: f64 = 0.0;
        let mut faux_weight: f64 = 0.0;
        let mut faux_italic: f64 = 0.0;
        let mut primary_weight: f64 = 1.0 / 3.0;
        let mut msaa_samples: u8 = 0;
        let mut system_tab: usize = 0;
        let mut z_order: Vec<String> = Vec::new();

        for line in s.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (key, val) = line.split_once('=')?;
            match key {
                "version" => {
                    if val != "1" {
                        return None;
                    }
                }
                "demos" => {
                    let n: usize = val.parse().ok()?;
                    demos_count = Some(n);
                    demos = vec![None; n];
                }
                "tests" => {
                    let n: usize = val.parse().ok()?;
                    tests_count = Some(n);
                    tests = vec![None; n];
                }
                "about" => {
                    about = Some(parse_window_state(val)?);
                }
                "backend" => {
                    let v: u8 = val.parse().ok()?;
                    backend_open = v != 0;
                }
                "theme" => {
                    theme_pref = ThemePreference::from_key(val).unwrap_or(ThemePreference::System);
                }
                "accent" => {
                    accent_color = AccentColor::from_key(val).unwrap_or_default();
                }
                "window" => {
                    let mut it = val.splitn(4, ',');
                    window_w = it.next()?.parse().ok();
                    window_h = it.next()?.parse().ok();
                    let fs: u8 = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
                    window_fullscreen = fs != 0;
                    let mx: u8 = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
                    window_maximized = mx != 0;
                }
                "font_name" => {
                    font_name = Some(val.to_string());
                }
                "font_size_scale" => {
                    font_size_scale = val.parse().unwrap_or(1.0);
                }
                "lcd" => {
                    let v: u8 = val.parse().unwrap_or(0);
                    lcd_enabled = v != 0;
                }
                "hinting" => {
                    let v: u8 = val.parse().unwrap_or(0);
                    hinting_enabled = v != 0;
                }
                "gamma" => {
                    gamma = val.parse().unwrap_or(1.0);
                }
                "width_scale" => {
                    width_scale = val.parse().unwrap_or(1.0);
                }
                "interval" => {
                    interval = val.parse().unwrap_or(0.0);
                }
                "faux_weight" => {
                    faux_weight = val.parse().unwrap_or(0.0);
                }
                "faux_italic" => {
                    faux_italic = val.parse().unwrap_or(0.0);
                }
                "primary_weight" => {
                    primary_weight = val.parse().unwrap_or(1.0 / 3.0);
                }
                "msaa" => {
                    msaa_samples = val.parse().unwrap_or(0);
                }
                "system_tab" => {
                    system_tab = val.parse().unwrap_or(0);
                }
                "z_order" => {
                    z_order = val
                        .split('|')
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)
                        .collect();
                }
                "inspector" => {
                    let mut halves = val.splitn(2, ';');
                    let head = halves.next().unwrap_or("");
                    let bits = halves.next().unwrap_or("");
                    let mut hit = head.splitn(3, ',');
                    let sel_raw: i64 = hit.next().and_then(|s| s.parse().ok()).unwrap_or(-1);
                    let props_h: f64 = hit.next().and_then(|s| s.parse().ok()).unwrap_or(160.0);
                    let open_u8: u8 = hit.next().and_then(|s| s.parse().ok()).unwrap_or(0);
                    let expanded: Vec<bool> = bits.chars().map(|c| c == '1').collect();
                    let selected = if sel_raw < 0 {
                        None
                    } else {
                        Some(sel_raw as usize)
                    };
                    inspector = Some(InspectorPersist {
                        expanded,
                        selected,
                        props_h,
                        open: open_u8 != 0,
                    });
                }
                k if k.starts_with('d') => {
                    let i: usize = k[1..].parse().ok()?;
                    let ws = parse_window_state(val)?;
                    if i < demos.len() {
                        demos[i] = Some(ws);
                    }
                }
                k if k.starts_with('t') => {
                    let i: usize = k[1..].parse().ok()?;
                    let ws = parse_window_state(val)?;
                    if i < tests.len() {
                        tests[i] = Some(ws);
                    }
                }
                _ => {}
            }
        }

        let demos_count = demos_count?;
        let tests_count = tests_count?;
        if demos.len() != demos_count || tests.len() != tests_count {
            return None;
        }

        Some(SavedState {
            demos: demos.into_iter().collect::<Option<Vec<_>>>()?,
            tests: tests.into_iter().collect::<Option<Vec<_>>>()?,
            about: about?,
            backend_open,
            theme_pref,
            accent_color,
            window_w,
            window_h,
            window_fullscreen,
            window_maximized,
            inspector,
            font_name,
            font_size_scale,
            lcd_enabled,
            hinting_enabled,
            gamma,
            width_scale,
            interval,
            faux_weight,
            faux_italic,
            primary_weight,
            msaa_samples,
            system_tab,
            z_order,
        })
    }
}

fn parse_window_state(s: &str) -> Option<WindowState> {
    let mut it = s.splitn(6, ',');
    let open: u8 = it.next()?.parse().ok()?;
    let x: f64 = it.next()?.parse().ok()?;
    let y: f64 = it.next()?.parse().ok()?;
    let w: f64 = it.next()?.parse().ok()?;
    let h: f64 = it.next()?.parse().ok()?;
    let maximized: u8 = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    Some(WindowState {
        open: open != 0,
        x,
        y,
        w,
        h,
        maximized: maximized != 0,
    })
}

// ── StateAccessor ─────────────────────────────────────────────────────────────

/// Reads the current state of all demo windows from shared cells.
///
/// Created by `build_demo_ui` and passed to the platform harness, which calls
/// `current_state()` when it needs to persist the layout.
pub struct StateAccessor {
    pub demo_open: Vec<Rc<Cell<bool>>>,
    pub demo_pos: Vec<Rc<Cell<Rect>>>,
    pub demo_maximized: Vec<Rc<Cell<bool>>>,
    pub test_open: Vec<Rc<Cell<bool>>>,
    pub test_pos: Vec<Rc<Cell<Rect>>>,
    pub test_maximized: Vec<Rc<Cell<bool>>>,
    pub about_open: Rc<Cell<bool>>,
    pub about_pos: Rc<Cell<Rect>>,
    pub about_maximized: Rc<Cell<bool>>,
    pub backend_open: Rc<Cell<bool>>,
    /// Top-bar theme selection.
    pub theme_pref: Rc<Cell<ThemePreference>>,
    /// Top-bar accent swatch selection.
    pub accent_color: Rc<Cell<AccentColor>>,
    /// Latest OS-window size, updated by the platform harness on Resized.
    pub window_size: Rc<Cell<(u32, u32)>>,
    /// Whether the OS window is currently borderless-fullscreen.
    pub window_fullscreen: Rc<Cell<bool>>,
    /// Whether the OS window is currently maximized.
    pub window_maximized: Rc<Cell<bool>>,
    /// Pulled each tick by `current_state` to snapshot the Inspector panel's
    /// expand/select/split state.  Returns `None` when the inspector has
    /// never been laid out yet.
    pub inspector_snapshot: Rc<dyn Fn() -> Option<InspectorPersist>>,

    // ── System-window persistence ─────────────────────────────────────
    //
    // These cells are the System window's model: the ComboBox /
    // DragValue / ToggleSwitch write to them, `current_state` reads
    // them for disk save.  The auto-save loop picks any change up
    // within a frame.
    /// Name of the currently-selected font (matches an entry in
    /// `system::FONT_OPTIONS`).  `None` = default (Cascadia Code).
    pub font_name: Rc<RefCell<Option<String>>>,
    /// Font-size multiplier.  Mirrors
    /// [`agg_gui::font_settings::current_font_size_scale`].
    pub font_size_scale: Rc<Cell<f64>>,
    /// LCD subpixel toggle mirror.
    pub lcd_enabled: Rc<Cell<bool>>,
    /// Hinting toggle mirror.
    pub hinting_enabled: Rc<Cell<bool>>,
    /// Typography-style parameter mirrors — shared with the System window
    /// and the TrueType LCD Subpixel demo so changes in either route
    /// write through to disk via the auto-save loop.
    pub gamma: Rc<Cell<f64>>,
    pub width_scale: Rc<Cell<f64>>,
    pub interval: Rc<Cell<f64>>,
    pub faux_weight: Rc<Cell<f64>>,
    pub faux_italic: Rc<Cell<f64>>,
    pub primary_weight: Rc<Cell<f64>>,
    /// OS-level MSAA sample count (0/2/4/8/16).  Read by the backend panel
    /// dropdown; the platform harness reads the persisted value at boot to
    /// configure the GL surface — changing this at runtime therefore only
    /// takes effect after a restart.
    pub msaa_samples: Rc<Cell<u8>>,

    /// Active tab index inside the System window — persisted so users
    /// stay on the Render / Font tab they were last on.
    pub system_tab: Rc<Cell<usize>>,

    /// Shared z-order tracker — back-to-front list of window titles
    /// updated whenever any `Window` fires its `on_raised` callback.
    /// Read at save time so the saved state captures the user's last
    /// stacking choice.  See `SavedState::z_order` for the persistence
    /// format and restore semantics.
    pub z_order: Rc<RefCell<Vec<String>>>,
}

impl StateAccessor {
    pub fn current_state(&self) -> SavedState {
        let demos = self
            .demo_open
            .iter()
            .zip(&self.demo_pos)
            .zip(&self.demo_maximized)
            .map(|((o, p), m)| {
                let r = p.get();
                WindowState {
                    open: o.get(),
                    x: r.x,
                    y: r.y,
                    w: r.width,
                    h: r.height,
                    maximized: m.get(),
                }
            })
            .collect();
        let tests = self
            .test_open
            .iter()
            .zip(&self.test_pos)
            .zip(&self.test_maximized)
            .map(|((o, p), m)| {
                let r = p.get();
                WindowState {
                    open: o.get(),
                    x: r.x,
                    y: r.y,
                    w: r.width,
                    h: r.height,
                    maximized: m.get(),
                }
            })
            .collect();
        let r = self.about_pos.get();
        let about = WindowState {
            open: self.about_open.get(),
            x: r.x,
            y: r.y,
            w: r.width,
            h: r.height,
            maximized: self.about_maximized.get(),
        };
        let (ww, wh) = self.window_size.get();
        SavedState {
            demos,
            tests,
            about,
            backend_open: self.backend_open.get(),
            theme_pref: self.theme_pref.get(),
            accent_color: self.accent_color.get(),
            window_w: if ww > 0 { Some(ww) } else { None },
            window_h: if wh > 0 { Some(wh) } else { None },
            window_fullscreen: self.window_fullscreen.get(),
            window_maximized: self.window_maximized.get(),
            inspector: (self.inspector_snapshot)(),
            font_name: self.font_name.borrow().clone(),
            font_size_scale: self.font_size_scale.get(),
            lcd_enabled: self.lcd_enabled.get(),
            hinting_enabled: self.hinting_enabled.get(),
            gamma: self.gamma.get(),
            width_scale: self.width_scale.get(),
            interval: self.interval.get(),
            faux_weight: self.faux_weight.get(),
            faux_italic: self.faux_italic.get(),
            primary_weight: self.primary_weight.get(),
            msaa_samples: self.msaa_samples.get(),
            system_tab: self.system_tab.get(),
            z_order: self.z_order.borrow().clone(),
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Reproduces the "demo opens full-screen on re-open" bug:
    ///
    ///   - User opens some demos, closes the app.  Demos they NEVER opened
    ///     have `demo_pos_cells[i] == Rect::default()` (zero rect) because
    ///     `Window::layout()` short-circuits on invisible windows and never
    ///     writes the position cell.
    ///   - `current_state()` writes that zero rect into the saved file.
    ///   - Next launch, the restore path turns it into `with_bounds(0,0,0,0)`.
    ///   - Parent `Stack::set_bounds` sees zero-size bounds and overwrites
    ///     them with the full canvas rect (`window.rs` set_bounds fallback).
    ///   - Demo appears full-screen.
    ///
    /// The fix lives in the restore path: `has_valid_bounds()` lets the
    /// caller filter zero rects back to the tile-rect default.
    #[test]
    fn zero_sized_window_state_is_not_valid() {
        let never_laid_out = WindowState {
            open: false,
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 0.0,
            maximized: false,
        };
        assert!(
            !never_laid_out.has_valid_bounds(),
            "zero-size rect must be rejected so the restore path can fall back to tile_rect"
        );

        let real = WindowState {
            open: true,
            x: 60.0,
            y: 60.0,
            w: 360.0,
            h: 280.0,
            maximized: false,
        };
        assert!(real.has_valid_bounds());
    }

    /// Round-trip a saved state that has a mix of real and zero-size demo
    /// entries — mirrors what `current_state()` emits when some demos were
    /// opened last session and others never were.  After deserialising, the
    /// zero entries must still be detectable as invalid so restore falls
    /// back to defaults.
    #[test]
    fn serialised_zero_entries_round_trip_as_invalid() {
        let saved = SavedState {
            demos: vec![
                WindowState {
                    open: true,
                    x: 60.0,
                    y: 60.0,
                    w: 360.0,
                    h: 280.0,
                    maximized: true,
                },
                WindowState {
                    open: false,
                    x: 0.0,
                    y: 0.0,
                    w: 0.0,
                    h: 0.0,
                    maximized: false,
                },
            ],
            tests: vec![],
            about: WindowState {
                open: false,
                x: 80.0,
                y: 80.0,
                w: 440.0,
                h: 500.0,
                maximized: false,
            },
            backend_open: false,
            theme_pref: ThemePreference::Dark,
            accent_color: AccentColor::Purple,
            window_w: None,
            window_h: None,
            window_fullscreen: false,
            window_maximized: false,
            inspector: None,
            font_name: None,
            font_size_scale: 1.0,
            lcd_enabled: false,
            hinting_enabled: false,
            gamma: 1.0,
            width_scale: 1.0,
            interval: 0.0,
            faux_weight: 0.0,
            faux_italic: 0.0,
            primary_weight: 1.0 / 3.0,
            msaa_samples: 0,
            system_tab: 0,
            z_order: Vec::new(),
        };

        let text = saved.serialize();
        let back = SavedState::deserialize(&text).expect("round-trip must parse");
        assert_eq!(back.theme_pref, ThemePreference::Dark);
        assert_eq!(back.accent_color, AccentColor::Purple);
        assert!(
            back.demos[0].has_valid_bounds(),
            "opened demo must survive round-trip as valid"
        );
        assert!(
            !back.demos[1].has_valid_bounds(),
            "never-opened demo must remain flagged as invalid"
        );
        assert!(
            back.demos[0].maximized,
            "per-window maximized state must survive round-trip"
        );
    }
}
