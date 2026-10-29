# Later: more of agg-gui through bevy_reflect

How reflection works today, and how a widget opts in, is in the code: the crate header's
"Reflection" section (`agg-gui/src/lib.rs`), `Widget::as_reflect`
(`agg-gui/src/widget/widget_trait.rs`) and the edit pipeline (`agg-gui/src/widget/tree_inspector/`).
MatterCAD does not use reflection yet; this is what could come next.

## Widgets still to convert

Five widgets have companion props (Slider, Checkbox, ProgressBar, ToggleSwitch, Container).

- **Label** (`widgets/label.rs`): `text`, `font_size`, `color`, `align`, `buffered`, `wrap`,
  `dim`, `strong`, `bold`, `ignore_system_font`, `lcd_pref`, `line_spacing`; `ellipsis` needs
  `EllipsisMode` (and `align` needs `LabelAlign`) to derive `Reflect` first. Three sites set
  `label.buffered = false` directly (`widgets/performance.rs`, `widgets/performance/run_mode.rs`,
  `demo-ui/src/backend_panel/widgets.rs`) and become `label.props.buffered`. Every setter keeps
  its `self.cache.invalidate()`. Converting Label first makes Button and Hyperlink easy.
- **DragValue** (`widgets/drag_value.rs`): `value`, `min`, `max`, `speed`, `step`, `decimals`,
  `suffix`, `font_size`, the same shape as Slider. Drag, edit and hover state stays on the widget.
- **TextField** (`widgets/text_field.rs`): `placeholder`, `font_size`, `padding` (the `f64`),
  `read_only`, `select_all_on_focus`, `password_mode`. The text itself lives in the shared
  `Rc<RefCell<TextEditState>>`, so exposing it needs a decision of its own.
- **FlexColumn** (`widgets/flex.rs`): `gap`, `inner_padding`, `background`, `use_panel_bg`,
  `fit_width`, `top_anchor`. **FlexRow** (`widgets/flex_row.rs`): `gap`, `inner_padding`,
  `background`. Callers use only builders, so the move is mechanical.
- Then, in priority order: Button, Hyperlink, RadioGroup (`selected_index`, options), ComboBox,
  TabView, CollapsingHeader, Window (keep persisted values apart from run-time state);
  ScrollView, Splitter, Resize and Tooltip have little to tune; ColorPicker (its colour, once
  the inspector has a colour editor); MarkdownView, TreeView and MenuBar, whose state is mostly
  not user-tunable.

## Open questions

1. **Numeric editing.** A reflected number steps by 5% per click on either half of its row,
   and only `f64` fields take the edit. Better: drag to scrub (a small `DragValue` per row),
   double-click to type a value; and edits for `f32` and integer fields.
2. **Colour editing.** `Color` fields show as `rgba(...)` text. Clicking one could open the
   `ColorPicker` widget in a popup.
3. **Enum editing.** Reflected enums (`ThemePreference`, `AccentColor`, `SpinnerSize`) show as
   `Debug` text and cannot be edited; `bevy_reflect` lists their variants (`ReflectRef::Enum`),
   so a dropdown is cheap. `HAnchor`/`VAnchor` are opaque bit flags: the inspector's own anchor
   rows cycle their presets, but an anchor inside a widget's props would need the same special
   case.
4. **Serialization.** Saved state still has its own code (`OsWindowState::serialize` and
   `deserialize`). One `TypeRegistry` built at startup, with every persisted type registered,
   and bevy_reflect's serde bridge (RON or JSON) would replace it with one path.
5. **Upgrades.** bevy_reflect breaks its API every few months; each upgrade is planned work.
6. **Field paths.** `InspectorEdit::field_path` is a `String`, fine for one-shot edits.
   Animating fields by path every frame should use `ParsedPath`, which validates the path once.
7. **Bound widgets.** A Checkbox or ToggleSwitch bound to a shared cell reads the cell, so an
   inspector edit of its props does not show. Whether an edit should write the cell too is open.
