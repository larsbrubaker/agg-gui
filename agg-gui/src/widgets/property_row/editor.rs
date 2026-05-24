//! Editor metadata for property rows — what kind of inline / panel
//! widget the property panel should mount for each field.
//!
//! Lives at the agg-gui layer because the schema is shared widget
//! vocabulary: any host wanting MatterCAD-style reflection-driven
//! property panels feeds its reflected fields into these types.
//! Host-side `PropertyInfo` analogues (e.g. atomartist's `PropDef`)
//! compose these types but keep their own value-typed defaults.
//!
//! ## Mapped from MatterCAD attributes
//!
//! | MatterCAD attribute             | agg-gui shape                       |
//! | ------------------------------- | ----------------------------------- |
//! | `[Slider(min, max, easing, …)]` | `EditorKind::Slider(NumberAttrs)`   |
//! | `[MaxDecimalPlaces(n)]`         | `NumberAttrs::max_decimal_places`   |
//! | `[Description("…")]`            | `NodeFieldAttrs::description`       |
//! | `[ReadOnly(true)]` on string    | `EditorKind::StringReadOnly`        |
//! | `[MultiLineEdit]`               | `EditorKind::StringMultiLine`       |
//! | `[EnumDisplay(Mode = Tabs)]`    | `EditorKind::EnumTabs { variants }` |
//! | `[EnumDisplay(Mode = Buttons)]` | `EditorKind::EnumButtons { … }`     |
//! | `[EnumDisplay(Mode = IconRow)]` | `EditorKind::EnumDropdown { … }`    |
//! | `[HideFromEditor]`              | `NodeFieldAttrs::hidden`            |

use std::sync::Arc;

/// Editor hint for a property — how the property panel should render
/// an editor for the current value.
///
/// The variants describe *intent*, not pixels — the row factory at
/// `widgets/property_row` picks the concrete widget. Keeping the hint
/// in the schema lets headless callers (tests, serialization, future
/// inspector ports) reason about the editor shape without depending
/// on the rendered widget tree.
#[derive(Clone, Debug, PartialEq)]
pub enum EditorKind {
    /// Click-and-drag horizontally to edit a number. Default for
    /// `Number` properties.
    NumberDrag(NumberAttrs),
    /// Horizontal slider between `min` and `max`. NodeDesigner's
    /// "slider" widget and MatterCAD's `[Slider(...)]` map here.
    Slider(NumberAttrs),
    /// Boolean checkbox toggle.
    Toggle,
    /// Color swatch + picker.
    ColorPicker,
    /// 4×4 matrix — typically rendered as a compact button that opens
    /// a translation/rotation/scale sub-panel.
    Matrix,
    /// Read-only text display. Used when a property's value isn't
    /// directly editable on the node row (e.g. a derived value).
    Display,
    /// Editable single-line string. MatterCAD's default `string`
    /// editor.
    StringSingleLine,
    /// Editable multi-line string. MatterCAD's `[MultiLineEdit]`.
    StringMultiLine,
    /// Word-wrapped read-only text. MatterCAD's `[ReadOnly(true)]`.
    StringReadOnly,
    /// Enum rendered as a dropdown combo box. The `variants` list is
    /// the ordered set of allowed values (also the canonical display
    /// labels). Current value is matched against one of these entries.
    EnumDropdown { variants: Vec<Arc<str>> },
    /// Enum rendered as a row of mutually-exclusive buttons.
    EnumButtons { variants: Vec<Arc<str>> },
    /// Enum rendered as a full-width tab strip — MatterCAD's
    /// `EnumDisplay.Tabs`. Best for 2–5 variants the user switches
    /// between at the top of a panel.
    EnumTabs { variants: Vec<Arc<str>> },
    /// Image picker / preview. MatterCAD's `ImageBufferPropertyEditor`.
    Image,
}

impl EditorKind {
    /// `NumberDrag` editor with `[min, max]` range.
    pub fn drag_range(min: f64, max: f64) -> Self {
        EditorKind::NumberDrag(NumberAttrs {
            min: Some(min),
            max: Some(max),
            ..Default::default()
        })
    }

    /// `Slider` editor with `[min, max]` range.
    pub fn slider_range(min: f64, max: f64) -> Self {
        EditorKind::Slider(NumberAttrs {
            min: Some(min),
            max: Some(max),
            ..Default::default()
        })
    }

    /// Inclusive numeric range when this editor is numeric, else `None`.
    pub fn numeric_range(&self) -> (Option<f64>, Option<f64>) {
        match self {
            EditorKind::NumberDrag(a) | EditorKind::Slider(a) => (a.min, a.max),
            _ => (None, None),
        }
    }

    /// Numeric editor attributes when this editor is numeric.
    pub fn number_attrs(&self) -> Option<&NumberAttrs> {
        match self {
            EditorKind::NumberDrag(a) | EditorKind::Slider(a) => Some(a),
            _ => None,
        }
    }
}

impl Default for EditorKind {
    fn default() -> Self {
        EditorKind::Display
    }
}

/// Numeric editor attributes — used by [`EditorKind::NumberDrag`] and
/// [`EditorKind::Slider`]. Mirrors NodeDesigner's `addWidget("slider", ...)`
/// option bag and MatterCAD's `[Slider]` / `[MaxDecimalPlaces]` attributes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NumberAttrs {
    /// Inclusive minimum.
    pub min: Option<f64>,
    /// Inclusive maximum.
    pub max: Option<f64>,
    /// Drag step (smallest delta per pixel of motion). `None` lets the
    /// editor pick a sensible default for the range.
    pub step: Option<f64>,
    /// Display + clamp as an integer.
    pub integer: bool,
    /// Power-of-N easing applied to slider drag deltas. NodeDesigner's
    /// `easeIn: 2` maps here.
    pub ease_in: Option<f64>,
    /// Snap drag deltas to a screen-space grid. NodeDesigner's
    /// `useSnapGrid` maps here.
    pub snap_grid: bool,
    /// Limit display precision. MatterCAD's `[MaxDecimalPlaces(n)]` —
    /// the stored value keeps full precision; this only affects
    /// rendering.
    pub max_decimal_places: Option<u8>,
}

impl NumberAttrs {
    pub fn with_range(min: f64, max: f64) -> Self {
        Self {
            min: Some(min),
            max: Some(max),
            ..Default::default()
        }
    }
    pub fn integer(mut self) -> Self {
        self.integer = true;
        self
    }
    pub fn with_step(mut self, step: f64) -> Self {
        self.step = Some(step);
        self
    }
    pub fn with_ease_in(mut self, e: f64) -> Self {
        self.ease_in = Some(e);
        self
    }
    pub fn with_snap_grid(mut self) -> Self {
        self.snap_grid = true;
        self
    }
    pub fn with_decimal_places(mut self, n: u8) -> Self {
        self.max_decimal_places = Some(n);
        self
    }
}

/// Conditional visibility for a field — the data-driven analogue of
/// MatterCAD's `IPropertyGridModifier.UpdateControls(change)` hook.
///
/// The host knows which sibling boolean property gates the
/// `AdvancedOn` / `AdvancedOff` rows (typically a `bool` named
/// `advanced`); the UI layer filters rows before rendering based on
/// the live value of that toggle.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VisibleWhen {
    /// Always render (default).
    #[default]
    Always,
    /// Render only when the node's `advanced` toggle is on. MatterCAD's
    /// `IPropertyGridModifier.UpdateControls` pattern for advanced
    /// rows.
    AdvancedOn,
    /// Render only when the node's `advanced` toggle is off. Used by
    /// the easy-mode hint message that nudges the user toward
    /// Advanced.
    AdvancedOff,
    /// Never render. MatterCAD's `[HideFromEditor]`.
    Never,
}

/// Field-level metadata — declared once per reflected struct field and
/// consumed both by the host's `PropDef`-style binding type (which
/// folds these into the property store) and by the property panel
/// when it renders the field's editor + label.
#[derive(Clone, Debug, Default)]
pub struct NodeFieldAttrs {
    pub label: Option<Arc<str>>,
    pub editor: EditorKind,
    /// When `Some(socket_name)`, the field is paired with the input
    /// socket of that name (host-side concept): the canvas draws the
    /// field's inline editor on the socket's row, and the editor is
    /// hidden when the socket is connected. Hosts without an input-
    /// socket concept ignore this.
    pub bound_input: Option<Arc<str>>,
    /// Free-text description shown in tooltips / the property-panel
    /// detail view. MatterCAD's `[Description("…")]`.
    pub description: Option<Arc<str>>,
    /// Visibility gate — Always, AdvancedOn, AdvancedOff, or Never.
    /// The UI filters rows by combining this with the live "advanced"
    /// toggle value.
    pub visible_when: VisibleWhen,
}

impl NodeFieldAttrs {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn with_label(mut self, label: impl Into<Arc<str>>) -> Self {
        self.label = Some(label.into());
        self
    }
    pub fn with_editor(mut self, editor: EditorKind) -> Self {
        self.editor = editor;
        self
    }
    pub fn bound_to(mut self, socket: impl Into<Arc<str>>) -> Self {
        self.bound_input = Some(socket.into());
        self
    }
    pub fn with_description(mut self, text: impl Into<Arc<str>>) -> Self {
        self.description = Some(text.into());
        self
    }
    /// Set the conditional visibility gate. Same trio as MatterCAD's
    /// `[HideFromEditor]` + `IPropertyGridModifier.UpdateControls`.
    pub fn visible_when(mut self, when: VisibleWhen) -> Self {
        self.visible_when = when;
        self
    }
    /// Shorthand for `visible_when(VisibleWhen::AdvancedOn)` — the
    /// common case of "this row is only relevant after the user opens
    /// Advanced".
    pub fn advanced(mut self) -> Self {
        self.visible_when = VisibleWhen::AdvancedOn;
        self
    }
    /// Shorthand for `visible_when(VisibleWhen::AdvancedOff)` — used
    /// by easy-mode hint messages that disappear once Advanced is on.
    pub fn easy_only(mut self) -> Self {
        self.visible_when = VisibleWhen::AdvancedOff;
        self
    }
    /// Shorthand for `visible_when(VisibleWhen::Never)`.
    pub fn hidden(mut self) -> Self {
        self.visible_when = VisibleWhen::Never;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slider_range_helper_sets_min_max() {
        let k = EditorKind::slider_range(1.0, 400.0);
        match k {
            EditorKind::Slider(a) => {
                assert_eq!(a.min, Some(1.0));
                assert_eq!(a.max, Some(400.0));
            }
            other => panic!("expected Slider, got {:?}", other),
        }
    }

    #[test]
    fn default_editor_is_display() {
        assert!(matches!(EditorKind::default(), EditorKind::Display));
    }

    #[test]
    fn number_attrs_builder_chains() {
        let a = NumberAttrs::with_range(0.0, 360.0)
            .integer()
            .with_step(1.0)
            .with_decimal_places(0)
            .with_ease_in(2.0)
            .with_snap_grid();
        assert_eq!(a.min, Some(0.0));
        assert_eq!(a.max, Some(360.0));
        assert!(a.integer);
        assert_eq!(a.step, Some(1.0));
        assert_eq!(a.max_decimal_places, Some(0));
        assert_eq!(a.ease_in, Some(2.0));
        assert!(a.snap_grid);
    }

    #[test]
    fn node_field_attrs_builder_chains() {
        let a = NodeFieldAttrs::new()
            .with_label("Diameter")
            .with_editor(EditorKind::slider_range(1.0, 400.0))
            .with_description("Width across.")
            .advanced();
        assert_eq!(a.label.as_deref().map(|x| x.as_ref()), Some("Diameter"));
        assert!(matches!(a.editor, EditorKind::Slider(_)));
        assert!(a.description.as_deref().map(|x| x.contains("Width")).unwrap_or(false));
        assert_eq!(a.visible_when, VisibleWhen::AdvancedOn);
    }

    #[test]
    fn visible_when_shorthands_set_expected_variant() {
        assert_eq!(NodeFieldAttrs::new().visible_when, VisibleWhen::Always);
        assert_eq!(NodeFieldAttrs::new().advanced().visible_when, VisibleWhen::AdvancedOn);
        assert_eq!(NodeFieldAttrs::new().easy_only().visible_when, VisibleWhen::AdvancedOff);
        assert_eq!(NodeFieldAttrs::new().hidden().visible_when, VisibleWhen::Never);
    }

    #[test]
    fn numeric_range_is_none_for_non_numeric() {
        assert_eq!(EditorKind::Toggle.numeric_range(), (None, None));
        assert_eq!(EditorKind::ColorPicker.numeric_range(), (None, None));
        assert_eq!(EditorKind::StringSingleLine.numeric_range(), (None, None));
    }

    #[test]
    fn enum_variants_round_trip() {
        let k = EditorKind::EnumTabs {
            variants: vec!["Easy".into(), "Advanced".into()],
        };
        if let EditorKind::EnumTabs { variants } = k {
            assert_eq!(variants.len(), 2);
            assert_eq!(variants[0].as_ref(), "Easy");
        } else {
            panic!("expected EnumTabs");
        }
    }
}
