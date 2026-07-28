//! Sliders demo — a live configurator for the `agg_gui::Slider` widget.
//!
//! Ported from egui's `egui_demo_lib/src/demo/sliders.rs`: a single demo slider
//! is reconfigured through every `Slider` option via a panel of controls
//! (logarithmic, clamping, smart aim, step, integer/f64, orientation, trailing
//! fill, handle shape, and an editable min/max range).
//!
//! Because agg-gui trees are built once (unlike egui's immediate mode), the
//! demo slider — and the two range sliders whose ranges depend on the
//! integer/logarithmic toggles — are wrapped in [`Rebuilder`]s keyed on version
//! counters that the controls bump when the configuration changes. The demo
//! slider's value survives rebuilds through a shared `Rc<Cell<f64>>` value cell.
//!
//! Re-exported from the `windows` module as `sliders` so `content.rs` and the
//! `windows::sliders` call site are unchanged.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{
    Button, Checkbox, Conditional, DragValue, FlexColumn, Font, HandleShape, Label, RadioGroup,
    Rebuilder, ScrollView, Separator, SizedBox, Slider, SliderClamping, SliderOrientation, Widget,
};

/// Shared, cloneable configuration cells driving the demo. Every control writes
/// into these and bumps the version counters; the [`Rebuilder`]s read them.
#[derive(Clone)]
struct SliderConfig {
    value: Rc<Cell<f64>>,
    min: Rc<Cell<f64>>,
    max: Rc<Cell<f64>>,
    logarithmic: Rc<Cell<bool>>,
    /// 0 = Never, 1 = Edits, 2 = Always.
    clamping: Rc<Cell<usize>>,
    smart_aim: Rc<Cell<bool>>,
    step: Rc<Cell<f64>>,
    use_steps: Rc<Cell<bool>>,
    integer: Rc<Cell<bool>>,
    vertical: Rc<Cell<bool>>,
    trailing_fill: Rc<Cell<bool>>,
    /// 0 = Circle, 1 = Rectangle.
    handle_shape: Rc<Cell<usize>>,
    /// Bumped by any config change *and* by min/max edits — drives the demo slider.
    demo_ver: Rc<Cell<u64>>,
    /// Bumped by config changes only (not min/max) — drives the range sliders,
    /// so dragging a range slider doesn't rebuild it mid-drag.
    range_ver: Rc<Cell<u64>>,
}

impl SliderConfig {
    /// egui defaults (see `Sliders::default`).
    fn new() -> Self {
        Self {
            value: Rc::new(Cell::new(10.0)),
            min: Rc::new(Cell::new(0.0)),
            max: Rc::new(Cell::new(10000.0)),
            logarithmic: Rc::new(Cell::new(true)),
            clamping: Rc::new(Cell::new(2)),
            smart_aim: Rc::new(Cell::new(true)),
            step: Rc::new(Cell::new(10.0)),
            use_steps: Rc::new(Cell::new(false)),
            integer: Rc::new(Cell::new(false)),
            vertical: Rc::new(Cell::new(false)),
            trailing_fill: Rc::new(Cell::new(false)),
            handle_shape: Rc::new(Cell::new(0)),
            demo_ver: Rc::new(Cell::new(0)),
            range_ver: Rc::new(Cell::new(0)),
        }
    }

    /// A config toggle changed: rebuild both the demo slider and the range sliders.
    fn bump_config(&self) {
        self.demo_ver.set(self.demo_ver.get() + 1);
        self.range_ver.set(self.range_ver.get() + 1);
    }

    /// A min/max range edit: rebuild only the demo slider.
    fn bump_range(&self) {
        self.demo_ver.set(self.demo_ver.get() + 1);
    }

    fn clamping(&self) -> SliderClamping {
        match self.clamping.get() {
            0 => SliderClamping::Never,
            1 => SliderClamping::Edits,
            _ => SliderClamping::Always,
        }
    }

    fn shape(&self) -> HandleShape {
        if self.handle_shape.get() == 1 {
            HandleShape::Rect { aspect_ratio: 0.5 }
        } else {
            HandleShape::Circle
        }
    }

    /// The value type's extreme range, mirroring egui's `(type_min, type_max)`.
    fn type_range(&self) -> (f64, f64) {
        if self.integer.get() {
            (i32::MIN as f64, i32::MAX as f64)
        } else if self.logarithmic.get() {
            (-f64::INFINITY, f64::INFINITY)
        } else {
            (-1e5, 1e5) // linear sliders make little sense with huge numbers
        }
    }
}

fn label(text: &str, font: &Arc<Font>) -> Box<dyn Widget> {
    Box::new(Label::new(text, Arc::clone(font)).with_font_size(12.0))
}

/// Build the single live demo slider from the current configuration.
fn build_demo_slider(cfg: &SliderConfig, font: &Arc<Font>) -> Box<dyn Widget> {
    let (type_min, type_max) = cfg.type_range();
    let mn = cfg.min.get().clamp(type_min, type_max);
    let mx = cfg.max.get().clamp(type_min, type_max);
    let istep = if cfg.use_steps.get() {
        cfg.step.get()
    } else {
        0.0
    };
    let orientation = if cfg.vertical.get() {
        SliderOrientation::Vertical
    } else {
        SliderOrientation::Horizontal
    };

    let mut slider = Slider::new(cfg.value.get(), mn, mx, Arc::clone(font))
        .with_value_cell(Rc::clone(&cfg.value))
        .with_logarithmic(cfg.logarithmic.get())
        .with_clamping(cfg.clamping())
        .with_smart_aim(cfg.smart_aim.get())
        .with_orientation(orientation)
        .with_trailing_fill(cfg.trailing_fill.get())
        .with_handle_shape(cfg.shape())
        .with_step(istep);
    if cfg.integer.get() {
        slider = slider.with_integer(true);
    }

    let type_text = if cfg.integer.get() {
        "i32 demo slider"
    } else {
        "f64 demo slider"
    };

    let mut col = FlexColumn::new().with_gap(6.0);
    col.push(label(type_text, font), 0.0);
    col.push(Box::new(slider), 0.0);

    if !cfg.integer.get() {
        // deviation: egui adds "You can always see the full precision value by
        // hovering the value." — our value label has no hover tooltip (Tooltip
        // text is static, and the slider only rebuilds on config changes, so a
        // tooltip would go stale mid-drag), so that sentence is dropped.
        col.push(
            label(
                "Sliders will intelligently pick how many decimals to show.",
                font,
            ),
            0.0,
        );
        let value = Rc::clone(&cfg.value);
        col.push(
            Box::new(
                Button::new("Assign PI", Arc::clone(font))
                    .with_font_size(12.0)
                    .on_click(move || value.set(std::f64::consts::PI)),
            ),
            0.0,
        );
    }
    Box::new(col)
}

/// Build the two range sliders that edit `min` and `max`, matching egui's
/// pair of logarithmic "left"/"right" sliders.
fn build_range_sliders(cfg: &SliderConfig, font: &Arc<Font>) -> Box<dyn Widget> {
    let (type_min, type_max) = cfg.type_range();
    cfg.min.set(cfg.min.get().clamp(type_min, type_max));
    cfg.max.set(cfg.max.get().clamp(type_min, type_max));

    let make = |cell: Rc<Cell<f64>>| -> Box<dyn Widget> {
        let bump = cfg.clone();
        let write = Rc::clone(&cell);
        Box::new(
            Slider::new(cell.get(), type_min, type_max, Arc::clone(font))
                .with_value_cell(Rc::clone(&cell))
                .with_logarithmic(true)
                .with_smart_aim(cfg.smart_aim.get())
                .with_trailing_fill(cfg.trailing_fill.get())
                .with_handle_shape(cfg.shape())
                .on_change(move |v| {
                    write.set(v);
                    bump.bump_range();
                }),
        )
    };

    let mut col = FlexColumn::new().with_gap(6.0);
    col.push(label("Slider range:", font), 0.0);
    col.push(make(Rc::clone(&cfg.min)), 0.0);
    col.push(make(Rc::clone(&cfg.max)), 0.0);
    Box::new(col)
}

/// Build the Sliders demo — a live configurator matching egui's Sliders demo.
pub fn sliders(font: Arc<Font>) -> Box<dyn Widget> {
    let cfg = SliderConfig::new();

    let mut col = FlexColumn::new()
        .with_gap(8.0)
        .with_padding(16.0)
        .with_panel_bg();

    // deviation: egui says "You can click a slider value to edit it with the
    // keyboard." — our value strip is a plain Label (no inline editing), so
    // describe the keyboard interaction that actually works here.
    col.push(
        label(
            "Focus a slider and use the arrow keys to nudge its value.",
            &font,
        ),
        0.0,
    );

    // The live demo slider (rebuilds on any config or range change).
    {
        let cfg2 = cfg.clone();
        let font2 = Arc::clone(&font);
        let ver = Rc::clone(&cfg.demo_ver);
        col.push(
            Box::new(Rebuilder::new(
                move || ver.get(),
                move || build_demo_slider(&cfg2, &font2),
            )),
            0.0,
        );
    }

    col.push(Box::new(Separator::horizontal()), 0.0);

    // Editable range (rebuilds only when the value type / range extent changes).
    {
        let cfg2 = cfg.clone();
        let font2 = Arc::clone(&font);
        let ver = Rc::clone(&cfg.range_ver);
        col.push(
            Box::new(Rebuilder::new(
                move || ver.get(),
                move || build_range_sliders(&cfg2, &font2),
            )),
            0.0,
        );
    }

    push_controls(&mut col, &cfg, &font);

    Box::new(ScrollView::new(Box::new(col)))
}

/// Append every configuration control, in egui's order and wording.
fn push_controls(col: &mut FlexColumn, cfg: &SliderConfig, font: &Arc<Font>) {
    col.push(Box::new(Separator::horizontal()), 0.0);

    // Trailing fill.
    {
        let c = cfg.clone();
        col.push(
            Box::new(
                Checkbox::new(
                    "Toggle trailing color",
                    Arc::clone(font),
                    cfg.trailing_fill.get(),
                )
                .with_font_size(12.0)
                .on_change(move |b| {
                    c.trailing_fill.set(b);
                    c.bump_config();
                }),
            ),
            0.0,
        );
        col.push(
            label(
                "When enabled, trailing color will be painted up until the handle.",
                font,
            ),
            0.0,
        );
    }

    col.push(Box::new(Separator::horizontal()), 0.0);

    // Handle shape.
    {
        let c = cfg.clone();
        col.push(label("Handle shape:", font), 0.0);
        col.push(
            Box::new(
                RadioGroup::new(
                    vec!["Circle", "Rectangle"],
                    cfg.handle_shape.get(),
                    Arc::clone(font),
                )
                .with_font_size(12.0)
                .on_change(move |idx| {
                    c.handle_shape.set(idx);
                    c.bump_config();
                }),
            ),
            0.0,
        );
    }

    col.push(Box::new(Separator::horizontal()), 0.0);

    // Use steps + step editor.
    {
        let c = cfg.clone();
        col.push(
            Box::new(
                Checkbox::new("Use steps", Arc::clone(font), cfg.use_steps.get())
                    .with_font_size(12.0)
                    .on_change(move |b| {
                        c.use_steps.set(b);
                        c.bump_config();
                    }),
            ),
            0.0,
        );
        col.push(
            label(
                "When enabled, the minimal value change would be restricted to a given step.",
                font,
            ),
            0.0,
        );
        let c = cfg.clone();
        let step_editor = Box::new(
            DragValue::new(cfg.step.get(), 0.0, 1_000_000.0, Arc::clone(font))
                .with_font_size(12.0)
                .with_speed(1.0)
                .with_decimals(2)
                .on_change(move |v| {
                    c.step.set(v);
                    c.bump_config();
                }),
        );
        col.push(
            Box::new(Conditional::new(Rc::clone(&cfg.use_steps), step_editor)),
            0.0,
        );
    }

    col.push(Box::new(Separator::horizontal()), 0.0);

    // Slider type (i32 / f64).
    {
        let c = cfg.clone();
        col.push(label("Slider type:", font), 0.0);
        // egui order: i32 (index 0), f64 (index 1).
        let initial = if cfg.integer.get() { 0 } else { 1 };
        col.push(
            Box::new(
                RadioGroup::new(vec!["i32", "f64"], initial, Arc::clone(font))
                    .with_font_size(12.0)
                    .on_change(move |idx| {
                        c.integer.set(idx == 0);
                        c.bump_config();
                    }),
            ),
            0.0,
        );
    }

    // Orientation.
    {
        let c = cfg.clone();
        col.push(label("Slider orientation:", font), 0.0);
        let initial = if cfg.vertical.get() { 1 } else { 0 };
        col.push(
            Box::new(
                RadioGroup::new(vec!["Horizontal", "Vertical"], initial, Arc::clone(font))
                    .with_font_size(12.0)
                    .on_change(move |idx| {
                        c.vertical.set(idx == 1);
                        c.bump_config();
                    }),
            ),
            0.0,
        );
    }

    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);

    // Logarithmic.
    {
        let c = cfg.clone();
        col.push(
            Box::new(
                Checkbox::new("Logarithmic", Arc::clone(font), cfg.logarithmic.get())
                    .with_font_size(12.0)
                    .on_change(move |b| {
                        c.logarithmic.set(b);
                        c.bump_config();
                    }),
            ),
            0.0,
        );
        col.push(
            label(
                "Logarithmic sliders are great for when you want to span a huge range, \
                 i.e. from zero to a million.",
                font,
            ),
            0.0,
        );
        col.push(
            label("Logarithmic sliders can include infinity and zero.", font),
            0.0,
        );
    }

    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);

    // Clamping.
    {
        let c = cfg.clone();
        col.push(label("Clamping:", font), 0.0);
        col.push(
            Box::new(
                RadioGroup::new(
                    vec!["Never", "Edits", "Always"],
                    cfg.clamping.get(),
                    Arc::clone(font),
                )
                .with_font_size(12.0)
                .on_change(move |idx| {
                    c.clamping.set(idx);
                    c.bump_config();
                }),
            ),
            0.0,
        );
        col.push(
            label(
                "If true, the slider will clamp incoming and outgoing values to the given range.",
                font,
            ),
            0.0,
        );
        col.push(
            label(
                "If false, the slider can show values outside its range, and you cannot enter \
                 new values outside the range.",
                font,
            ),
            0.0,
        );
    }

    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);

    // Smart aim.
    {
        let c = cfg.clone();
        col.push(
            Box::new(
                Checkbox::new("Smart Aim", Arc::clone(font), cfg.smart_aim.get())
                    .with_font_size(12.0)
                    .on_change(move |b| {
                        c.smart_aim.set(b);
                        c.bump_config();
                    }),
            ),
            0.0,
        );
        col.push(
            label(
                "Smart Aim will guide you towards round values when you drag the slider so \
                 you you are more likely to hit 250 than 247.23",
                font,
            ),
            0.0,
        );
    }

    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);

    // Reset.
    {
        let c = cfg.clone();
        col.push(
            Box::new(
                Button::new("Reset", Arc::clone(font))
                    .with_font_size(12.0)
                    .on_click(move || {
                        let d = SliderConfig::new();
                        c.value.set(d.value.get());
                        c.min.set(d.min.get());
                        c.max.set(d.max.get());
                        c.logarithmic.set(d.logarithmic.get());
                        c.clamping.set(d.clamping.get());
                        c.smart_aim.set(d.smart_aim.get());
                        c.step.set(d.step.get());
                        c.use_steps.set(d.use_steps.get());
                        c.integer.set(d.integer.get());
                        c.vertical.set(d.vertical.get());
                        c.trailing_fill.set(d.trailing_fill.get());
                        c.handle_shape.set(d.handle_shape.get());
                        c.bump_config();
                    }),
            ),
            0.0,
        );
    }
}
