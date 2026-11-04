//! Widget-tree export for page automation: `widget_nodes(type_name)` lists
//! every widget of one type with its on-screen rect and its
//! `Widget::properties`, read from the same inspector walk the demo's tree
//! inspector shows (`App::collect_inspector_nodes`). Playwright tests
//! (`demo/tests/file_browser.test.ts`) use it to find a widget to click and to
//! read the state a real gesture left behind, instead of guessing from
//! pixels. Read-only: it borrows `DEMO_APP` shared and changes nothing.

use wasm_bindgen::prelude::*;

/// Every widget whose type name is `type_name`, in paint order, as
/// `{ x, y, w, h, visible, props: { name: value, .. } }`. The rect is the
/// widget's screen bounds in logical units, Y up from the canvas bottom.
/// Empty while the app is not built yet or its cell is borrowed.
#[wasm_bindgen]
pub fn widget_nodes(type_name: &str) -> Result<js_sys::Array, JsValue> {
    let out = js_sys::Array::new();
    let nodes = crate::DEMO_APP.with(|cell| {
        cell.try_borrow()
            .ok()
            .and_then(|app| app.as_ref().map(|a| a.collect_inspector_nodes()))
            .unwrap_or_default()
    });
    for node in nodes.iter().filter(|n| n.type_name == type_name) {
        let obj = js_sys::Object::new();
        let b = node.screen_bounds;
        set(&obj, "x", JsValue::from_f64(b.x))?;
        set(&obj, "y", JsValue::from_f64(b.y))?;
        set(&obj, "w", JsValue::from_f64(b.width))?;
        set(&obj, "h", JsValue::from_f64(b.height))?;
        set(&obj, "visible", JsValue::from_bool(node.visible))?;
        let props = js_sys::Object::new();
        for (key, value) in &node.properties {
            set(&props, key, JsValue::from_str(value))?;
        }
        set(&obj, "props", props.into())?;
        out.push(&obj);
    }
    Ok(out)
}

fn set(obj: &js_sys::Object, key: &str, value: JsValue) -> Result<(), JsValue> {
    js_sys::Reflect::set(obj, &JsValue::from_str(key), &value).map(|_| ())
}
