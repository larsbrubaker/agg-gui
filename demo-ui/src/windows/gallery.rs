//! Widget Gallery demo window.
//!
//! Displays one representative instance of every interactive widget in the
//! agg-gui library: buttons (primary / secondary / danger), checkboxes, a
//! radio group, a combo box, slider, progress bar, toggle switches, drag
//! values, a hyperlink, and a text input field.
//!
//! Section headers use `Label` without an explicit color so they follow the
//! active theme (`ctx.visuals().text_color`) and remain readable in both dark
//! and light mode.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::widget::{paint_subtree, CompositingLayer};
use agg_gui::{
    Button, Checkbox, CollapsingHeader, Color, ColorPicker, ColorWheelPicker, ComboBox,
    Conditional, DragValue, DrawCtx, Event, EventResult, FlexColumn, FlexRow, Font, Hyperlink,
    ImageView, Label, ProgressBar, RadioGroup, Rect, ScrollView, Separator, Size, SizedBox,
    Slider, TextField, ToggleSwitch, Tooltip, Widget,
};

const AGG_GUI_DOCS_URL: &str = "https://docs.rs/agg-gui/";
const AGG_GUI_REPO_URL: &str = "https://github.com/larsbrubaker/agg-gui";

/// ProgressBar wrapper that follows a shared scalar, matching egui's gallery
/// where the slider and progress bar reflect the same value.
struct ScalarProgress {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    scalar: Rc<Cell<f64>>,
    bar: ProgressBar,
}

impl ScalarProgress {
    fn new(scalar: Rc<Cell<f64>>, font: Arc<Font>) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            scalar,
            bar: ProgressBar::new(0.0, font),
        }
    }
}

impl Widget for ScalarProgress {
    fn type_name(&self) -> &'static str {
        "ScalarProgress"
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
        self.bar.set_value(self.scalar.get() / 360.0);
        let size = self.bar.layout(available);
        self.bounds = Rect::new(0.0, 0.0, size.width, size.height);
        self.bar.set_bounds(self.bounds);
        size
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        paint_subtree(&mut self.bar, ctx);
    }

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// Scope wrapper around the gallery grid that reproduces egui's
/// `UiBuilder::disabled()/invisible()` + `Ui::multiply_opacity` block.
///
/// * **Visible** — [`Widget::is_visible`] returns the `visible` flag, which
///   suppresses painting of the whole subtree while layout still reserves its
///   space (so the window does not jump).
/// * **Interactive** — when unchecked, [`Widget::claims_pointer_exclusively`]
///   makes the scope the hit target for every pointer position inside it, so
///   `hit_test_subtree` stops before reaching any child widget and events are
///   swallowed here. The subtree is additionally dimmed as a visual cue.
/// * **Opacity** — realised through a compositing layer
///   ([`Widget::compositing_layer`]); the framework paints the subtree into a
///   transparent buffer and blends it back at the requested alpha, which is the
///   only path that fades backbuffered text/buttons uniformly. See the note in
///   [`GalleryScope::compositing_layer`] about the device-scale limitation.
///
/// It also paints egui's alternating row striping behind the grid rows.
struct GalleryScope {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    visible: Rc<Cell<bool>>,
    interactive: Rc<Cell<bool>>,
    opacity: Rc<Cell<f64>>,
}

impl GalleryScope {
    fn new(
        grid: Box<dyn Widget>,
        visible: Rc<Cell<bool>>,
        interactive: Rc<Cell<bool>>,
        opacity: Rc<Cell<f64>>,
    ) -> Self {
        Self {
            bounds: Rect::default(),
            children: vec![grid],
            visible,
            interactive,
            opacity,
        }
    }

    /// Effective whole-subtree alpha: the user opacity, further dimmed when the
    /// grid is non-interactive to approximate egui's grayed-out disabled look
    /// (per-widget disabled styling is not plumbed through the library).
    fn effective_alpha(&self) -> f64 {
        let base = self.opacity.get().clamp(0.0, 1.0);
        if self.interactive.get() {
            base
        } else {
            base * 0.4
        }
    }
}

impl Widget for GalleryScope {
    fn type_name(&self) -> &'static str {
        "GalleryScope"
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
        let size = if let Some(grid) = self.children.first_mut() {
            let s = grid.layout(available);
            grid.set_bounds(Rect::new(0.0, 0.0, s.width, s.height));
            s
        } else {
            Size::new(0.0, 0.0)
        };
        self.bounds = Rect::new(0.0, 0.0, size.width, size.height);
        size
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        // Alternating row striping (egui's `Grid::striped(true)`).  Painted
        // before the grid child so the rows draw on top.  The stripe tint is
        // derived from the theme text colour at low alpha so it adapts to both
        // dark and light mode.
        let v = ctx.visuals();
        let stripe = Color::rgba(v.text_color.r, v.text_color.g, v.text_color.b, 0.05);
        let width = self.bounds.width;
        if let Some(grid) = self.children.first() {
            for (i, row) in grid.children().iter().enumerate() {
                if i % 2 == 1 {
                    let rb = row.bounds();
                    ctx.set_fill_color(stripe);
                    ctx.begin_path();
                    ctx.rounded_rect(0.0, rb.y - 2.0, width, rb.height + 4.0, 2.0);
                    ctx.fill();
                }
            }
        }
    }

    fn is_visible(&self) -> bool {
        self.visible.get()
    }

    /// When the grid is non-interactive, disable all pointer *and* keyboard
    /// interaction for the subtree: `hit_test_subtree` stops here (clicks are
    /// swallowed) and `collect_focusable` skips the subtree (Tab cannot reach
    /// the grid's TextField).  Mirrors egui's `UiBuilder::disabled()`.
    fn blocks_child_interaction(&self) -> bool {
        !self.interactive.get()
    }

    fn compositing_layer(&mut self) -> Option<CompositingLayer> {
        let alpha = self.effective_alpha();
        // Only request a layer when there is something to fade.  The library's
        // `push_layer` resets the CTM to identity, so on HiDPI (device scale
        // != 1) the faded subtree would render at logical size inside a
        // physical-pixel target — a mis-scale.  We therefore restrict the fade
        // to device scale 1.0 and leave it a no-op elsewhere (flagged for a
        // library fix: either honour `global_alpha` in the backbuffer blit or
        // preserve device scale across `push_layer`).
        let ds = agg_gui::device_scale();
        if alpha < 0.999 && self.visible.get() && (ds - 1.0).abs() < 0.01 {
            Some(CompositingLayer::new(0.0, 0.0, 0.0, 0.0, alpha))
        } else {
            None
        }
    }

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// Build the Widget Gallery demo — a scrollable showcase of all interactive
/// widgets with section headers.
pub fn widget_gallery(font: Arc<Font>) -> Box<dyn Widget> {
    let boolean = Rc::new(Cell::new(false));
    let radio_sel = Rc::new(Cell::new(0_usize));
    let scalar = Rc::new(Cell::new(42.0_f64));
    let color = Rc::new(Cell::new(Color::rgba(0.35, 0.55, 0.90, 0.50)));
    let custom_toggle = Rc::new(Cell::new(false));
    let visible = Rc::new(Cell::new(true));
    let interactive = Rc::new(Cell::new(true));
    let opacity = Rc::new(Cell::new(1.0_f64));

    /// Left-column doc link label, following egui's gallery structure.
    fn doc_link(title: &str, search_term: &str, font: &Arc<Font>) -> Box<dyn Widget> {
        let url = format!("https://docs.rs/agg-gui?search={search_term}");
        Box::new(
            Hyperlink::new(title, Arc::clone(font))
                .with_font_size(13.0)
                .on_click(move || crate::url::open_url(&url)),
        )
    }

    fn grid_row(left: Box<dyn Widget>, right: Box<dyn Widget>) -> Box<dyn Widget> {
        Box::new(
            FlexRow::new()
                .with_gap(40.0)
                .add(Box::new(SizedBox::new().with_width(132.0).with_child(left)))
                .add_flex(right, 1.0),
        )
    }

    fn selectable_button(
        label: &'static str,
        value: usize,
        selected: Rc<Cell<usize>>,
        font: Arc<Font>,
    ) -> Box<dyn Widget> {
        Box::new(
            Button::new(label, font)
                .with_font_size(12.0)
                .on_click(move || selected.set(value)),
        )
    }

    // The grid itself carries no panel background: the striping painted by
    // `GalleryScope` must sit on top of the panel fill, so the panel background
    // lives on the outer column instead.
    let mut col = FlexColumn::new().with_gap(8.0).with_padding(16.0);

    col.push(
        grid_row(
            doc_link("Label", "label", &font),
            Box::new(Label::new(
                "Welcome to the widget gallery!",
                Arc::clone(&font),
            )),
        ),
        0.0,
    );

    col.push(
        grid_row(
            doc_link("Hyperlink", "Hyperlink", &font),
            Box::new(
                Hyperlink::new("agg-gui on GitHub", Arc::clone(&font))
                    .with_font_size(13.0)
                    .on_click(|| crate::url::open_url(AGG_GUI_REPO_URL)),
            ),
        ),
        0.0,
    );

    col.push(
        grid_row(
            doc_link("TextEdit", "TextEdit", &font),
            Box::new(
                SizedBox::new().with_height(30.0).with_child(Box::new(
                    TextField::new(Arc::clone(&font))
                        .with_font_size(13.0)
                        .with_placeholder("Write something here"),
                )),
            ),
        ),
        0.0,
    );

    {
        let b = Rc::clone(&boolean);
        col.push(
            grid_row(
                doc_link("Button", "button", &font),
                Box::new(
                    SizedBox::new().with_height(28.0).with_child(Box::new(
                        Button::new("Click me!", Arc::clone(&font))
                            .with_font_size(13.0)
                            .on_click(move || b.set(!b.get())),
                    )),
                ),
            ),
            0.0,
        );
    }

    {
        let b = Rc::clone(&boolean);
        col.push(
            grid_row(
                doc_link("Link", "link", &font),
                Box::new(
                    Hyperlink::new("Click me!", Arc::clone(&font))
                        .with_font_size(13.0)
                        .on_click(move || b.set(!b.get())),
                ),
            ),
            0.0,
        );
    }

    col.push(
        grid_row(
            doc_link("Checkbox", "checkbox", &font),
            Box::new(
                Checkbox::new("Checkbox", Arc::clone(&font), boolean.get())
                    .with_font_size(13.0)
                    .with_state_cell(Rc::clone(&boolean)),
            ),
        ),
        0.0,
    );

    col.push(
        grid_row(
            doc_link("RadioButton", "radio", &font),
            Box::new(
                RadioGroup::new(
                    vec!["First", "Second", "Third"],
                    radio_sel.get(),
                    Arc::clone(&font),
                )
                .with_font_size(13.0)
                .with_selected_cell(Rc::clone(&radio_sel)),
            ),
        ),
        0.0,
    );

    col.push(
        grid_row(
            doc_link("SelectableLabel", "SelectableLabel", &font),
            Box::new(
                FlexRow::new()
                    .with_gap(6.0)
                    .add(selectable_button(
                        "First",
                        0,
                        Rc::clone(&radio_sel),
                        Arc::clone(&font),
                    ))
                    .add(selectable_button(
                        "Second",
                        1,
                        Rc::clone(&radio_sel),
                        Arc::clone(&font),
                    ))
                    .add(selectable_button(
                        "Third",
                        2,
                        Rc::clone(&radio_sel),
                        Arc::clone(&font),
                    )),
            ),
        ),
        0.0,
    );

    col.push(
        grid_row(
            doc_link("ComboBox", "ComboBox", &font),
            Box::new(
                FlexRow::new()
                    .with_gap(8.0)
                    .add(Box::new(
                        Label::new("Take your pick", Arc::clone(&font)).with_font_size(13.0),
                    ))
                    .add_flex(
                        Box::new(
                            ComboBox::new(
                                vec!["First", "Second", "Third"],
                                radio_sel.get(),
                                Arc::clone(&font),
                            )
                            .with_selected_cell(Rc::clone(&radio_sel)),
                        ),
                        1.0,
                    ),
            ),
        ),
        0.0,
    );

    col.push(
        grid_row(
            doc_link("Slider", "Slider", &font),
            Box::new(
                Slider::new(scalar.get(), 0.0, 360.0, Arc::clone(&font))
                    .with_step(1.0)
                    .with_decimals(0)
                    .with_value_cell(Rc::clone(&scalar)),
            ),
        ),
        0.0,
    );

    col.push(
        grid_row(
            doc_link("DragValue", "DragValue", &font),
            Box::new(
                SizedBox::new()
                    .with_width(120.0)
                    .with_height(28.0)
                    .with_child(Box::new(
                        DragValue::new(scalar.get(), 0.0, 360.0, Arc::clone(&font))
                            .with_decimals(0)
                            .on_change({
                                let scalar = Rc::clone(&scalar);
                                move |x| scalar.set(x)
                            }),
                    )),
            ),
        ),
        0.0,
    );

    col.push(
        grid_row(
            doc_link("ProgressBar", "ProgressBar", &font),
            Box::new(Tooltip::new(
                Box::new(ScalarProgress::new(Rc::clone(&scalar), Arc::clone(&font))),
                "The progress bar can be animated!",
                Arc::clone(&font),
            )),
        ),
        0.0,
    );

    col.push(
        grid_row(
            doc_link("Color picker", "color_edit", &font),
            Box::new(ColorPicker::new(Rc::clone(&color), Arc::clone(&font))),
        ),
        0.0,
    );

    {
        // Inline `ColorWheelPicker` — circular hue ring + SV triangle,
        // each surface hardware back-buffered.  Pushes its committed
        // colour into the shared cell on Select so the standard
        // `ColorPicker` row above reflects the change.
        let wheel_cell = Rc::clone(&color);
        let initial = wheel_cell.get();
        let wheel = ColorWheelPicker::new(initial, Arc::clone(&font))
            .with_allow_none(true)
            .with_show_alpha(true)
            .on_change({
                let c = Rc::clone(&wheel_cell);
                move |opt| {
                    if let Some(col) = opt {
                        c.set(col);
                    }
                }
            })
            .on_select({
                let c = Rc::clone(&wheel_cell);
                move |opt| match opt {
                    Some(col) => c.set(col),
                    None => c.set(Color::transparent()),
                }
            });
        col.push(
            grid_row(
                doc_link("Color wheel", "color_wheel", &font),
                Box::new(wheel),
            ),
            0.0,
        );
    }

    col.push(
        grid_row(
            doc_link("Image", "Image", &font),
            Box::new(
                SizedBox::new().with_height(72.0).with_child(Box::new(
                    ImageView::new(Arc::clone(&font), Rc::new(RefCell::new(None)))
                        .with_placeholder("Image widget")
                        .with_min_height(72.0),
                )),
            ),
        ),
        0.0,
    );

    col.push(
        grid_row(
            doc_link("Button with image", "Button::image_and_text", &font),
            Box::new(
                SizedBox::new().with_height(28.0).with_child(Box::new(
                    Button::new("Image + text", Arc::clone(&font))
                        .with_font_size(13.0)
                        .on_click({
                            let boolean = Rc::clone(&boolean);
                            move || boolean.set(!boolean.get())
                        }),
                )),
            ),
        ),
        0.0,
    );

    col.push(
        grid_row(
            doc_link("Separator", "separator", &font),
            Box::new(Separator::horizontal()),
        ),
        0.0,
    );

    col.push(
        grid_row(
            doc_link("CollapsingHeader", "collapsing", &font),
            Box::new(
                CollapsingHeader::new("Click to see what is hidden!", Arc::clone(&font))
                    .default_open(false)
                    .with_content(Box::new(
                        Label::new("It's a custom toggle switch:", Arc::clone(&font))
                            .with_font_size(13.0),
                    )),
            ),
        ),
        0.0,
    );

    {
        let row = FlexRow::new().with_gap(8.0).add(Box::new(
            ToggleSwitch::new(custom_toggle.get()).with_state_cell(Rc::clone(&custom_toggle)),
        ));
        col.push(
            grid_row(
                Box::new(
                    Hyperlink::new("Custom widget", Arc::clone(&font))
                        .with_font_size(13.0)
                        .on_click(|| crate::url::open_url(AGG_GUI_REPO_URL)),
                ),
                Box::new(row),
            ),
            0.0,
        );
    }

    // Wrap the grid in the scope that applies Visible / Interactive / Opacity
    // and paints the row striping.
    let scope = GalleryScope::new(
        Box::new(col),
        Rc::clone(&visible),
        Rc::clone(&interactive),
        Rc::clone(&opacity),
    );

    // Outer column carries the panel background and holds the scope plus the
    // egui-style control bar and documentation footer, which stay interactive
    // and fully opaque regardless of the scope's state.
    let mut outer = FlexColumn::new().with_gap(8.0).with_panel_bg();
    outer.push(Box::new(scope), 0.0);
    outer.push(Box::new(Separator::horizontal()), 0.0);
    outer.push(gallery_controls(&font, &visible, &interactive, &opacity), 0.0);
    outer.push(Box::new(Separator::horizontal()), 0.0);
    outer.push(
        Box::new(
            FlexColumn::new()
                .with_gap(4.0)
                .with_padding(16.0)
                .add(Box::new(
                    Hyperlink::new(AGG_GUI_DOCS_URL, Arc::clone(&font))
                        .with_font_size(12.0)
                        .on_click(|| crate::url::open_url(AGG_GUI_DOCS_URL)),
                ))
                .add(Box::new(
                    Label::new(
                        "Click widget names to search agg-gui docs.",
                        Arc::clone(&font),
                    )
                    .with_font_size(11.0)
                    .with_wrap(true),
                )),
        ),
        0.0,
    );

    outer.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);

    Box::new(ScrollView::new(Box::new(outer)))
}

/// Build egui's bottom control bar: a "Visible" checkbox that hides the whole
/// grid, and — only while visible — an "Interactive" checkbox plus an Opacity
/// drag value, each with the hover tooltips from the egui gallery.
fn gallery_controls(
    font: &Arc<Font>,
    visible: &Rc<Cell<bool>>,
    interactive: &Rc<Cell<bool>>,
    opacity: &Rc<Cell<f64>>,
) -> Box<dyn Widget> {
    let visible_checkbox = Tooltip::new(
        Box::new(
            Checkbox::new("Visible", Arc::clone(font), visible.get())
                .with_font_size(13.0)
                .with_state_cell(Rc::clone(visible)),
        ),
        "Uncheck to hide all the widgets.",
        Arc::clone(font),
    );

    let interactive_checkbox = Tooltip::new(
        Box::new(
            Checkbox::new("Interactive", Arc::clone(font), interactive.get())
                .with_font_size(13.0)
                .with_state_cell(Rc::clone(interactive)),
        ),
        "Uncheck to inspect how the widgets look when disabled.",
        Arc::clone(font),
    );

    let opacity_control = Tooltip::new(
        Box::new(
            FlexRow::new()
                .with_gap(6.0)
                .add(Box::new(
                    SizedBox::new().with_width(56.0).with_height(28.0).with_child(
                        Box::new(
                            DragValue::new(opacity.get(), 0.0, 1.0, Arc::clone(font))
                                .with_speed(0.01)
                                .with_decimals(2)
                                .on_change({
                                    let opacity = Rc::clone(opacity);
                                    move |v| opacity.set(v)
                                }),
                        ),
                    ),
                ))
                .add(Box::new(
                    Label::new("Opacity", Arc::clone(font)).with_font_size(13.0),
                )),
        ),
        "Reduce this value to make widgets semi-transparent.",
        Arc::clone(font),
    );

    // The Interactive checkbox and Opacity control only appear while Visible is
    // checked, matching egui's `if self.visible { ... }` guard.
    let conditional_controls = Conditional::new(
        Rc::clone(visible),
        Box::new(
            FlexRow::new()
                .with_gap(12.0)
                .add(Box::new(interactive_checkbox))
                .add(Box::new(opacity_control)),
        ),
    );

    Box::new(
        FlexRow::new()
            .with_gap(12.0)
            .with_padding(16.0)
            .add(Box::new(visible_checkbox))
            .add(Box::new(conditional_controls)),
    )
}
