//! Persistent demo state — window open flags and positions.
//!
//! Used to restore window layout between native restarts and WASM refreshes.
//! Serialization is a simple CSV format (no external deps):
//!   version=1
//!   demos=<count>
//!   tests=<count>
//!   d<i>=<open>,<x>,<y>,<w>,<h>
//!   t<i>=<open>,<x>,<y>,<w>,<h>
//!   about=<open>,<x>,<y>,<w>,<h>

use std::cell::Cell;
use std::rc::Rc;

use agg_gui::Rect;

// ── WindowState ───────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct WindowState {
    pub open: bool,
    pub x: f64, pub y: f64, pub w: f64, pub h: f64,
}

impl WindowState {
    pub fn to_rect(&self) -> Rect {
        Rect::new(self.x, self.y, self.w, self.h)
    }
}

// ── SavedState ────────────────────────────────────────────────────────────────

pub struct SavedState {
    pub demos: Vec<WindowState>,
    pub tests: Vec<WindowState>,
    pub about: WindowState,
    /// Whether the left-side Backend panel is open.
    pub backend_open: bool,
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
}

/// Persisted inspector UI state.  Flat bit-vector of expanded nodes in DFS
/// order + optional selected-node index + properties-pane height.
#[derive(Clone, Debug, Default)]
pub struct InspectorPersist {
    pub expanded: Vec<bool>,
    pub selected: Option<usize>,
    pub props_h:  f64,
    /// Whether the Inspector window itself was visible at save time.
    pub open:     bool,
}

impl SavedState {
    pub fn serialize(&self) -> String {
        let mut out = String::new();
        out.push_str("version=1\n");
        out.push_str(&format!("demos={}\n", self.demos.len()));
        out.push_str(&format!("tests={}\n", self.tests.len()));
        for (i, w) in self.demos.iter().enumerate() {
            out.push_str(&format!("d{}={},{},{},{},{}\n",
                i, w.open as u8, w.x, w.y, w.w, w.h));
        }
        for (i, w) in self.tests.iter().enumerate() {
            out.push_str(&format!("t{}={},{},{},{},{}\n",
                i, w.open as u8, w.x, w.y, w.w, w.h));
        }
        out.push_str(&format!("about={},{},{},{},{}\n",
            self.about.open as u8, self.about.x, self.about.y,
            self.about.w, self.about.h));
        out.push_str(&format!("backend={}\n", self.backend_open as u8));
        if let (Some(w), Some(h)) = (self.window_w, self.window_h) {
            out.push_str(&format!("window={},{},{},{}\n",
                w, h,
                self.window_fullscreen as u8,
                self.window_maximized  as u8));
        }
        if let Some(insp) = &self.inspector {
            // `inspector=selected,props_h,open;expanded-bits`
            let sel = insp.selected.map(|i| i as i64).unwrap_or(-1);
            let bits: String = insp.expanded.iter()
                .map(|b| if *b { '1' } else { '0' })
                .collect();
            out.push_str(&format!("inspector={},{},{};{}\n",
                sel, insp.props_h, insp.open as u8, bits));
        }
        out
    }

    pub fn deserialize(s: &str) -> Option<Self> {
        let mut demos_count = None::<usize>;
        let mut tests_count = None::<usize>;
        let mut demos: Vec<Option<WindowState>> = Vec::new();
        let mut tests: Vec<Option<WindowState>> = Vec::new();
        let mut about = None::<WindowState>;
        let mut backend_open = false;
        let mut window_w: Option<u32> = None;
        let mut window_h: Option<u32> = None;
        let mut window_fullscreen = false;
        let mut window_maximized  = false;
        let mut inspector: Option<InspectorPersist> = None;

        for line in s.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') { continue; }
            let (key, val) = line.split_once('=')?;
            match key {
                "version" => { if val != "1" { return None; } }
                "demos"   => { let n: usize = val.parse().ok()?; demos_count = Some(n); demos = vec![None; n]; }
                "tests"   => { let n: usize = val.parse().ok()?; tests_count = Some(n); tests = vec![None; n]; }
                "about"   => { about = Some(parse_window_state(val)?); }
                "backend" => { let v: u8 = val.parse().ok()?; backend_open = v != 0; }
                "window"  => {
                    let mut it = val.splitn(4, ',');
                    window_w = it.next()?.parse().ok();
                    window_h = it.next()?.parse().ok();
                    let fs: u8 = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
                    window_fullscreen = fs != 0;
                    let mx: u8 = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
                    window_maximized  = mx != 0;
                }
                "inspector" => {
                    let mut halves = val.splitn(2, ';');
                    let head = halves.next().unwrap_or("");
                    let bits = halves.next().unwrap_or("");
                    let mut hit = head.splitn(3, ',');
                    let sel_raw: i64 = hit.next().and_then(|s| s.parse().ok()).unwrap_or(-1);
                    let props_h: f64 = hit.next().and_then(|s| s.parse().ok()).unwrap_or(160.0);
                    let open_u8: u8 = hit.next().and_then(|s| s.parse().ok()).unwrap_or(0);
                    let expanded: Vec<bool> = bits.chars()
                        .map(|c| c == '1').collect();
                    let selected = if sel_raw < 0 { None } else { Some(sel_raw as usize) };
                    inspector = Some(InspectorPersist { expanded, selected, props_h, open: open_u8 != 0 });
                }
                k if k.starts_with('d') => {
                    let i: usize = k[1..].parse().ok()?;
                    let ws = parse_window_state(val)?;
                    if i < demos.len() { demos[i] = Some(ws); }
                }
                k if k.starts_with('t') => {
                    let i: usize = k[1..].parse().ok()?;
                    let ws = parse_window_state(val)?;
                    if i < tests.len() { tests[i] = Some(ws); }
                }
                _ => {}
            }
        }

        let demos_count = demos_count?;
        let tests_count = tests_count?;
        if demos.len() != demos_count || tests.len() != tests_count { return None; }

        Some(SavedState {
            demos: demos.into_iter().collect::<Option<Vec<_>>>()?,
            tests: tests.into_iter().collect::<Option<Vec<_>>>()?,
            about: about?,
            backend_open,
            window_w,
            window_h,
            window_fullscreen,
            window_maximized,
            inspector,
        })
    }
}

fn parse_window_state(s: &str) -> Option<WindowState> {
    let mut it = s.splitn(5, ',');
    let open: u8 = it.next()?.parse().ok()?;
    let x: f64   = it.next()?.parse().ok()?;
    let y: f64   = it.next()?.parse().ok()?;
    let w: f64   = it.next()?.parse().ok()?;
    let h: f64   = it.next()?.parse().ok()?;
    Some(WindowState { open: open != 0, x, y, w, h })
}

// ── StateAccessor ─────────────────────────────────────────────────────────────

/// Reads the current state of all demo windows from shared cells.
///
/// Created by `build_demo_ui` and passed to the platform harness, which calls
/// `current_state()` when it needs to persist the layout.
pub struct StateAccessor {
    pub demo_open: Vec<Rc<Cell<bool>>>,
    pub demo_pos:  Vec<Rc<Cell<Rect>>>,
    pub test_open: Vec<Rc<Cell<bool>>>,
    pub test_pos:  Vec<Rc<Cell<Rect>>>,
    pub about_open: Rc<Cell<bool>>,
    pub about_pos:  Rc<Cell<Rect>>,
    pub backend_open: Rc<Cell<bool>>,
    /// Latest OS-window size, updated by the platform harness on Resized.
    pub window_size: Rc<Cell<(u32, u32)>>,
    /// Whether the OS window is currently borderless-fullscreen.
    pub window_fullscreen: Rc<Cell<bool>>,
    /// Whether the OS window is currently maximized.
    pub window_maximized:  Rc<Cell<bool>>,
    /// Pulled each tick by `current_state` to snapshot the Inspector panel's
    /// expand/select/split state.  Returns `None` when the inspector has
    /// never been laid out yet.
    pub inspector_snapshot: Rc<dyn Fn() -> Option<InspectorPersist>>,
}

impl StateAccessor {
    pub fn current_state(&self) -> SavedState {
        let demos = self.demo_open.iter().zip(&self.demo_pos)
            .map(|(o, p)| { let r = p.get(); WindowState { open: o.get(), x: r.x, y: r.y, w: r.width, h: r.height } })
            .collect();
        let tests = self.test_open.iter().zip(&self.test_pos)
            .map(|(o, p)| { let r = p.get(); WindowState { open: o.get(), x: r.x, y: r.y, w: r.width, h: r.height } })
            .collect();
        let r = self.about_pos.get();
        let about = WindowState { open: self.about_open.get(), x: r.x, y: r.y, w: r.width, h: r.height };
        let (ww, wh) = self.window_size.get();
        SavedState {
            demos,
            tests,
            about,
            backend_open: self.backend_open.get(),
            window_w: if ww > 0 { Some(ww) } else { None },
            window_h: if wh > 0 { Some(wh) } else { None },
            window_fullscreen: self.window_fullscreen.get(),
            window_maximized:  self.window_maximized.get(),
            inspector:         (self.inspector_snapshot)(),
        }
    }
}
