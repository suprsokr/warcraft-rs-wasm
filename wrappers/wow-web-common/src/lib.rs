//! Shared utilities for the `wow-*-web` wasm-bindgen wrappers.
//!
//! This crate is internal to the workspace and is not distributed as a
//! standalone web package. It collects tiny helpers that every wrapper
//! repeats (serializing to `JsValue`, wrapping bytes as `Uint8Array`,
//! mapping errors to `JsError`) so they stay consistent and easier to
//! maintain.
//!
//! It is only compiled for `wasm32-unknown-unknown` (and native tests);
//! it must be excluded from `wasm32-wasip1` builds in CI because
//! `js-sys`/`wasm-bindgen` do not support WASI.

use wasm_bindgen::prelude::*;

/// Serialize a Rust value to a JSON-friendly JS object.
pub fn to_js<T: serde::Serialize>(value: &T) -> Result<JsValue, JsError> {
    serde_wasm_bindgen::to_value(value)
        .map_err(|e| JsError::new(&format!("failed to serialize result: {e}")))
}

/// Wrap a byte slice as a JS `Uint8Array`.
pub fn to_uint8_array(bytes: &[u8]) -> js_sys::Uint8Array {
    js_sys::Uint8Array::from(bytes)
}

/// Copy the contents of a JS `Uint8Array` value into a `Vec<u8>`.
///
/// Returns `None` for `null`/`undefined` or for values that cannot be
/// interpreted as a `Uint8Array`.
pub fn to_vec(array: &JsValue) -> Option<Vec<u8>> {
    if array.is_null() || array.is_undefined() {
        return None;
    }
    js_sys::Uint8Array::new(array).to_vec().into()
}

/// Set a property on a JS object, returning a `JsError` on failure.
pub fn set_property(obj: &js_sys::Object, key: &str, value: JsValue) -> Result<(), JsError> {
    js_sys::Reflect::set(obj, &JsValue::from_str(key), &value)
        .map_err(|e| JsError::new(&format!("failed to set property '{key}': {e:?}")))?;
    Ok(())
}
