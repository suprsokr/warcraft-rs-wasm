//! Web (wasm-bindgen) bindings for the `wow-cdbc` DBC database library.
//!
//! DBC files are WoW's client-side database tables (spells, items, etc.).
//! Everything happens in memory: bytes in (`Uint8Array`), plain data out.
//!
//! ```js
//! import init, { DbcFile } from "wow-cdbc-web";
//! await init();
//!
//! const dbc = new DbcFile(new Uint8Array(await file.arrayBuffer()));
//! console.log(dbc.summary());
//!
//! // Parse raw records (no schema — all fields are UInt32)
//! const records = dbc.records();
//!
//! // Or with a schema for typed fields and named columns
//! const schema = [
//!   { name: "ID", type: "uint32" },
//!   { name: "Name", type: "string" },
//!   { name: "Value", type: "int32" },
//! ];
//! const typed = dbc.recordsWithSchema(schema);
//! ```

use serde::Serialize;
use std::io::Cursor;
use wasm_bindgen::prelude::*;
use wow_cdbc::{DbcParser, FieldType, RecordSet, Schema, SchemaField, Value};
use wow_web_common::{set_property, to_js, to_uint8_array};

fn parse_field_type(s: &str) -> Result<FieldType, JsError> {
    match s.to_ascii_lowercase().as_str() {
        "int32" => Ok(FieldType::Int32),
        "uint32" => Ok(FieldType::UInt32),
        "float32" | "float" => Ok(FieldType::Float32),
        "string" => Ok(FieldType::String),
        "bool" | "boolean" => Ok(FieldType::Bool),
        "uint8" => Ok(FieldType::UInt8),
        "int8" => Ok(FieldType::Int8),
        "uint16" => Ok(FieldType::UInt16),
        "int16" => Ok(FieldType::Int16),
        _ => Err(JsError::new(&format!(
            "unknown field type \"{s}\" (use int32, uint32, float32, string, bool, uint8, int8, uint16, int16)"
        ))),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DbcSummary {
    magic: String,
    record_count: u32,
    field_count: u32,
    record_size: u32,
    string_block_size: u32,
}

fn value_to_json(value: &Value, record_set: &RecordSet) -> serde_json::Value {
    match value {
        Value::Int32(v) => (*v).into(),
        Value::UInt32(v) => (*v).into(),
        Value::Float32(v) => serde_json::Number::from_f64(*v as f64)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        Value::StringRef(r) => record_set
            .get_string(*r)
            .map(|s| s.into())
            .unwrap_or(serde_json::Value::Null),
        Value::Bool(v) => (*v).into(),
        Value::UInt8(v) => (*v).into(),
        Value::Int8(v) => (*v).into(),
        Value::UInt16(v) => (*v).into(),
        Value::Int16(v) => (*v).into(),
        Value::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(|v| value_to_json(v, record_set)).collect())
        }
    }
}

/// A DBC database file parsed in memory.
///
/// Records can be read raw (all fields as UInt32) or with a schema for
/// typed, named columns. String references are resolved automatically.
#[wasm_bindgen(js_name = DbcFile)]
pub struct DbcFile {
    data: Vec<u8>,
    record_set: Option<RecordSet>,
}

#[wasm_bindgen(js_class = DbcFile)]
impl DbcFile {
    /// Parse a DBC file from raw bytes. Auto-detects WDBC/WDB2/WDB5/etc.
    #[wasm_bindgen(constructor)]
    pub fn open(data: &[u8]) -> Result<DbcFile, JsError> {
        // Validate by parsing once
        let _ = DbcParser::parse_bytes(data)
            .map_err(|e| JsError::new(&format!("failed to parse DBC: {e}")))?;
        Ok(DbcFile {
            data: data.to_vec(),
            record_set: None,
        })
    }

    /// Header summary: magic, record count, field count, record size,
    /// string block size.
    #[wasm_bindgen(js_name = summary)]
    pub fn summary(&self) -> Result<JsValue, JsError> {
        let parser = DbcParser::parse_bytes(&self.data)
            .map_err(|e| JsError::new(&format!("failed to parse DBC: {e}")))?;
        let h = parser.header();
        to_js(&DbcSummary {
            magic: String::from_utf8_lossy(&h.magic).to_string(),
            record_count: h.record_count,
            field_count: h.field_count,
            record_size: h.record_size,
            string_block_size: h.string_block_size,
        })
    }

    /// Parse all records without a schema. Each record is an array of
    /// UInt32 values (the raw field data). Returns
    /// `[[field0, field1, ...], ...]`.
    #[wasm_bindgen(js_name = records)]
    pub fn records(&mut self) -> Result<JsValue, JsError> {
        let parser = DbcParser::parse_bytes(&self.data)
            .map_err(|e| JsError::new(&format!("failed to parse DBC: {e}")))?;
        let record_set = parser
            .parse_records()
            .map_err(|e| JsError::new(&format!("failed to parse records: {e}")))?;
        let records: Vec<Vec<serde_json::Value>> = record_set
            .records()
            .iter()
            .map(|r| {
                r.values()
                    .iter()
                    .map(|v| value_to_json(v, &record_set))
                    .collect()
            })
            .collect();
        self.record_set = Some(record_set);
        to_js(&records)
    }

    /// Parse records with a schema. `schema` is an array of objects:
    /// `{ name: string, type: string }` where type is one of "int32",
    /// "uint32", "float32", "string", "bool", "uint8", "int8",
    /// "uint16", "int16".
    ///
    /// Returns `[{ name: value, ... }, ...]` — each record is an object
    /// with field names as keys. String references are resolved to their
    /// actual string values.
    #[wasm_bindgen(js_name = recordsWithSchema)]
    pub fn records_with_schema(&mut self, schema_js: JsValue) -> Result<JsValue, JsError> {
        let schema_fields: Vec<SchemaFieldJs> = serde_wasm_bindgen::from_value(schema_js)
            .map_err(|e| JsError::new(&format!("invalid schema: {e}")))?;

        let mut schema = Schema::new("web");
        for field in &schema_fields {
            schema.add_field(SchemaField::new(
                &field.name,
                parse_field_type(&field.type_)?,
            ));
        }

        let parser = DbcParser::parse_bytes(&self.data)
            .map_err(|e| JsError::new(&format!("failed to parse DBC: {e}")))?
            .with_schema(schema)
            .map_err(|e| JsError::new(&format!("schema validation failed: {e}")))?;

        let record_set = parser
            .parse_records()
            .map_err(|e| JsError::new(&format!("failed to parse records: {e}")))?;

        // Build JS objects manually to ensure plain object output (not Map)
        let arr = js_sys::Array::new();
        for r in record_set.records() {
            let obj = js_sys::Object::new();
            if let Some(s) = r.schema() {
                for (i, field) in s.fields.iter().enumerate() {
                    if let Some(v) = r.get_value(i) {
                        let js_val = serde_wasm_bindgen::to_value(&value_to_json(v, &record_set))
                            .map_err(|e| {
                            JsError::new(&format!("failed to serialize value: {e}"))
                        })?;
                        set_property(&obj, &field.name, js_val)?;
                    }
                }
            }
            arr.push(&obj);
        }
        self.record_set = Some(record_set);
        Ok(arr.into())
    }

    /// Get all strings from the string block as an array. Records must
    /// have been parsed first (call `records()` or `recordsWithSchema()`).
    #[wasm_bindgen(js_name = strings)]
    pub fn strings(&self) -> Result<JsValue, JsError> {
        let record_set = self.record_set.as_ref().ok_or_else(|| {
            JsError::new("parse records first (call records() or recordsWithSchema())")
        })?;

        let mut strings = Vec::new();
        let block = record_set.string_block();
        for record in record_set.records() {
            for value in record.values() {
                if let Value::StringRef(r) = value {
                    if let Ok(s) = block.get_string(*r) {
                        let owned = s.to_string();
                        if !strings.contains(&owned) {
                            strings.push(owned);
                        }
                    }
                }
            }
        }
        to_js(&strings)
    }

    /// Serialize the parsed records back to DBC bytes (round-trip).
    /// Requires records to have been parsed with a schema.
    #[wasm_bindgen(js_name = export)]
    pub fn export(&self) -> Result<js_sys::Uint8Array, JsError> {
        let record_set = self.record_set.as_ref().ok_or_else(|| {
            JsError::new("parse records first (call records() or recordsWithSchema())")
        })?;

        let mut out = Cursor::new(Vec::new());
        let mut writer = wow_cdbc::DbcWriter::new(&mut out);
        if let Some(schema) = record_set.schema() {
            writer = writer.with_schema(schema.clone());
        }
        writer
            .write_records(record_set)
            .map_err(|e| JsError::new(&format!("failed to write DBC: {e}")))?;
        Ok(to_uint8_array(&out.into_inner()))
    }
}

/// Schema field definition accepted by `recordsWithSchema`.
#[derive(serde::Deserialize)]
struct SchemaFieldJs {
    name: String,
    #[serde(rename = "type")]
    type_: String,
}
