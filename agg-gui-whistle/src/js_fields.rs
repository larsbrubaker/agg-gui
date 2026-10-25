//! Protocol messages (`protocol.rs`) as JavaScript objects, for
//! `postMessage` between the provider (`web/`) and the worker (`worker/`).
//!
//! A message is a flat object: `{ type: "push", session: 3, rate: 16000,
//! samples: Float32Array }`. Audio travels as a `Float32Array` whose buffer
//! is transferred, not copied, to the receiving side. wasm32 only.

use js_sys::{Array, Float32Array, Object, Reflect};
use wasm_bindgen::{JsCast, JsValue};

use crate::protocol::{Field, ProtocolError};

/// A message's fields as an object, and the buffers to transfer with it.
pub(crate) fn to_object(fields: Vec<(&'static str, Field)>) -> (Object, Array) {
    let object = Object::new();
    let transfer = Array::new();
    for (key, field) in fields {
        let value = match field {
            Field::Str(s) => JsValue::from_str(&s),
            Field::Num(n) => JsValue::from_f64(n),
            Field::Bool(b) => JsValue::from_bool(b),
            Field::Samples(samples) => {
                let array = Float32Array::from(samples.as_slice());
                transfer.push(&array.buffer());
                array.into()
            }
        };
        // Setting a property on a fresh plain object cannot fail.
        let _ = Reflect::set(&object, &JsValue::from_str(key), &value);
    }
    (object, transfer)
}

/// One field of `object`, if it has a readable one under `key`.
fn field(object: &JsValue, key: &str) -> Option<Field> {
    let value = Reflect::get(object, &JsValue::from_str(key)).ok()?;
    if let Some(s) = value.as_string() {
        Some(Field::Str(s))
    } else if let Some(n) = value.as_f64() {
        Some(Field::Num(n))
    } else if let Some(b) = value.as_bool() {
        Some(Field::Bool(b))
    } else {
        value
            .dyn_ref::<Float32Array>()
            .map(|a| Field::Samples(a.to_vec()))
    }
}

/// A protocol reader: `ToWorker::from_fields` or `FromWorker::from_fields`.
type Decode<T> = fn(&dyn Fn(&str) -> Option<Field>) -> Result<T, ProtocolError>;

/// Decode `object` with a protocol reader (`ToWorker::from_fields` or
/// `FromWorker::from_fields`).
pub(crate) fn from_object<T>(object: &JsValue, read: Decode<T>) -> Result<T, ProtocolError> {
    if !object.is_object() {
        return Err(ProtocolError("message is not an object".into()));
    }
    read(&|key| field(object, key))
}
