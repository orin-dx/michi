//! The `ToonValue` cell shared by every export that takes a value, and its one conversion into michi-core values.

use napi_derive::napi;

use super::num::{JsDecimals, JsFloat, JsInt};

/// Value type for a TOON row cell (JavaScript-friendly).
///
/// Use the `type` field to discriminate: `"str"`, `"int"`, `"float"`, `"bool"`, `"null"`.
#[napi(object, js_name = "ToonValue")]
#[derive(Debug, Clone, Default)]
pub struct JsToonValue {
    /// Discriminant: `"str"`, `"int"`, `"float"`, `"bool"`, or `"null"`.
    #[napi(js_name = "type")]
    pub r#type: String,
    /// The value when `type` is `"str"`.
    #[napi(js_name = "strVal")]
    pub str_val: Option<String>,
    /// The value when `type` is `"int"`.
    #[napi(js_name = "intVal", ts_type = "number")]
    pub int_val: Option<JsInt>,
    /// The value when `type` is `"float"`.
    #[napi(js_name = "floatVal", ts_type = "number")]
    pub float_val: Option<JsFloat>,
    /// The value when `type` is `"bool"`.
    #[napi(js_name = "boolVal")]
    pub bool_val: Option<bool>,
    /// Decimal places when `type` is `"float"` (KV and status render only). Rejected if outside [0, 20]. Omitted, the
    /// value keeps every digit it needs, so `1e-7` renders `1e-7` and `5e-324` renders `5e-324`. Given, it rounds as
    /// asked: `1e-7` with `decimalsVal: 2` renders `0`.
    #[napi(js_name = "decimalsVal", ts_type = "number")]
    pub decimals_val: Option<JsDecimals>,
}

/// A key-value item for `AgentResponse.kvItems` and `renderKv` (JavaScript-friendly).
#[napi(object, js_name = "KvItem")]
#[derive(Debug, Clone)]
pub struct JsKvItem {
    /// The field name.
    pub key: String,
    /// The field value.
    pub value: JsToonValue,
}

/// A [`JsToonValue`] checked against its `type` tag: the payload that tag names is present, and no other is read.
/// Both document families (TOON tables and KV lines) build their value from this, so a tag is validated in one place.
pub(crate) enum Cell {
    Str(String),
    Int(JsInt),
    Float(JsFloat, Option<JsDecimals>),
    Bool(bool),
    Null,
}

impl JsToonValue {
    /// The checked cell. A payload without its tag, an unknown tag, or a tag without its payload is an error, so a
    /// misspelled `type` cannot silently drop a value.
    pub(crate) fn into_cell(self) -> napi::Result<Cell> {
        let missing = |tag: &str, field: &str| {
            napi::Error::from_reason(format!("ToonValue: type is \"{tag}\" but {field} is missing"))
        };
        match self.r#type.as_str() {
            "str" => self.str_val.map(Cell::Str).ok_or_else(|| missing("str", "strVal")),
            "int" => self.int_val.map(Cell::Int).ok_or_else(|| missing("int", "intVal")),
            "float" => {
                self.float_val.map(|f| Cell::Float(f, self.decimals_val)).ok_or_else(|| missing("float", "floatVal"))
            }
            "bool" => self.bool_val.map(Cell::Bool).ok_or_else(|| missing("bool", "boolVal")),
            "null" => Ok(Cell::Null),
            other => Err(napi::Error::from_reason(format!(
                "ToonValue: unknown type {other:?}: expected \"str\", \"int\", \"float\", \"bool\", or \"null\""
            ))),
        }
    }
}

impl From<Cell> for crate::toon::Value {
    /// A TOON table cell. `decimalsVal` does not apply to tables.
    fn from(cell: Cell) -> Self {
        match cell {
            Cell::Str(s) => Self::Str(s.into()),
            Cell::Int(n) => Self::Int(n.get()),
            Cell::Float(f, _) => Self::Float(f.get()),
            Cell::Bool(b) => Self::Bool(b),
            Cell::Null => Self::Null,
        }
    }
}

impl From<Cell> for crate::kv::KvValue {
    /// A KV line or status value. Without `decimalsVal` a float keeps the number exact: its shortest decimals, as
    /// `RecoveryHint` deserialization does, or `FULL_PRECISION` (never rounds) for a value below about 1e-254.
    fn from(cell: Cell) -> Self {
        match cell {
            Cell::Str(s) => Self::Text(s),
            Cell::Int(n) => Self::Int(n.get()),
            Cell::Float(f, decimals) => {
                let decimals = decimals.map_or_else(|| crate::kv::exact_decimals(f.get()), JsDecimals::get_u8);
                Self::Float(f.get(), decimals)
            }
            Cell::Bool(b) => Self::Bool(b),
            Cell::Null => Self::Missing,
        }
    }
}

/// UTF-8 bytes of the string in one cell (0 for other types).
pub(crate) fn cell_bytes(v: &JsToonValue) -> usize {
    v.str_val.as_ref().map_or(0, String::len)
}

/// What one cell costs a response that keeps it: the stored value plus its string. Every cell keeps a
/// [`crate::toon::Value`] whatever its type, so a table of numbers or nulls is not free.
pub(crate) fn cell_cost(v: &JsToonValue) -> usize {
    std::mem::size_of::<crate::toon::Value>().saturating_add(cell_bytes(v))
}
