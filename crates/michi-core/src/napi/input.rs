//! Bounded, incremental conversion of caller-supplied JS values.
//!
//! napi-derive converts a `Vec<Vec<JsToonValue>>` or `String` parameter in full before the export's body runs, so a
//! size check in the body comes too late: a rejected 100,001 × 100 `rows` array cost 3.4 s and 2.6 GiB, and 10k × 10
//! cells sharing one 100 KB JS string were copied out once per cell (~10 GB). The exports therefore take untyped napi
//! values and convert them here: array lengths are checked before any element is read, and every string's UTF-8
//! length is read and charged to a per-call [`Budget`] before the string is copied.

use napi::bindgen_prelude::{FromNapiValue, JsObjectValue as _, Object, Unknown};
use napi::{JsString, JsValue as _, ValueType};

use super::{JsFailedOp, JsKvItem, JsPipelineStep, JsRecoveryHint, JsStatusItem, JsToonValue};

/// Most cells (row values) one call may pass.
pub(crate) const MAX_CELLS: usize = 1_000_000;
/// Most UTF-8 bytes of string data one call may pass, summed over every string argument and nested string.
pub(crate) const MAX_INPUT_BYTES: usize = 64 * 1024 * 1024;
/// Longest single cell value, hint, reason, description or message, in UTF-8 bytes.
pub(crate) const MAX_STRING_BYTES: usize = 1024 * 1024;
/// Longest type name, field name, key, tool name, error code, truncation hint or other identifier, in UTF-8 bytes.
pub(crate) const MAX_NAME_BYTES: usize = 4 * 1024;

/// `"{what} length {len} exceeds maximum of {max}"` when `len > max`.
pub(crate) fn check_len(what: &str, len: usize, max: usize) -> napi::Result<()> {
    if len > max {
        return Err(napi::Error::from_reason(format!("{what} length {len} exceeds maximum of {max}")));
    }
    Ok(())
}

/// A JS array whose length has been read but none of whose elements have.
pub(crate) struct List<'env> {
    obj: Object<'env>,
    len: u32,
}

impl<'env> List<'env> {
    /// `value` as an array, or an error saying `what` must be one.
    pub(crate) fn new(value: Unknown<'env>, what: &str) -> napi::Result<Self> {
        if !value.is_array()? {
            return Err(napi::Error::from_reason(format!("{what} must be an array")));
        }
        // Coercing an object to an object returns it unchanged.
        let obj = value.coerce_to_object()?;
        let len = obj.get_array_length_unchecked()?;
        Ok(Self { obj, len })
    }

    /// `value` as an array, or `None` when it is absent, `undefined` or `null`.
    pub(crate) fn optional(value: Option<Unknown<'env>>, what: &str) -> napi::Result<Option<Self>> {
        match value {
            Some(v) if !is_nullish(v)? => Self::new(v, what).map(Some),
            _ => Ok(None),
        }
    }

    /// The element count.
    pub(crate) fn len(&self) -> usize {
        usize::try_from(self.len).unwrap_or(usize::MAX)
    }

    fn get(&self, index: u32) -> napi::Result<Unknown<'env>> {
        self.obj.get_element::<Unknown<'env>>(index)
    }
}

pub(crate) fn is_nullish(value: Unknown<'_>) -> napi::Result<bool> {
    Ok(matches!(value.get_type()?, ValueType::Undefined | ValueType::Null))
}

fn missing(name: &str) -> napi::Error {
    napi::Error::from_reason(format!("Missing field `{name}`"))
}

/// `value` as an object, or an error saying `what` must be one.
pub(crate) fn object<'env>(value: Unknown<'env>, what: &str) -> napi::Result<Object<'env>> {
    if value.get_type()? != ValueType::Object {
        return Err(napi::Error::from_reason(format!("{what} must be an object")));
    }
    value.coerce_to_object()
}

/// Property `name`, or `None` when it is absent or `undefined`. As with napi-derive's generated object conversion,
/// `null` is a value (and so rejected wherever a string, number or object is expected), not an absent field.
fn prop<'env>(obj: &Object<'env>, name: &str) -> napi::Result<Option<Unknown<'env>>> {
    obj.get::<Unknown<'env>>(name)
}

/// A scalar property converted by its type's own `FromNapiValue` (the numeric newtypes reject out-of-domain numbers
/// with their usual messages); `None` when absent or `undefined`.
pub(crate) fn typed_prop<T: FromNapiValue>(obj: &Object<'_>, name: &str) -> napi::Result<Option<T>> {
    obj.get::<T>(name)
}

/// A required array property.
pub(crate) fn list_prop<'env>(obj: &Object<'env>, name: &str) -> napi::Result<List<'env>> {
    List::new(prop(obj, name)?.ok_or_else(|| missing(name))?, name)
}

/// Per-call accounting of cells and string bytes.
pub(crate) struct Budget {
    cells: usize,
    bytes: usize,
}

impl Budget {
    /// A fresh budget for one export call.
    pub(crate) fn new() -> Self {
        Self { cells: 0, bytes: 0 }
    }

    /// Charge `n` cells, failing once the call's total passes [`MAX_CELLS`].
    fn charge_cells(&mut self, n: usize) -> napi::Result<()> {
        self.cells = self.cells.saturating_add(n);
        if self.cells > MAX_CELLS {
            return Err(napi::Error::from_reason(format!("cell count {} exceeds maximum of {MAX_CELLS}", self.cells)));
        }
        Ok(())
    }

    /// Copy the JS string `value` out after checking it is a string of at most `max` UTF-8 bytes and charging its
    /// length to the call's byte budget. The UTF-16 length rejects most oversize strings without scanning them,
    /// since a string never has fewer UTF-8 bytes than UTF-16 units.
    pub(crate) fn string(&mut self, value: Unknown<'_>, what: &str, max: usize) -> napi::Result<String> {
        if value.get_type()? != ValueType::String {
            return Err(napi::Error::from_reason(format!("{what} must be a string")));
        }
        let s: JsString<'_> = value.coerce_to_string()?;
        let units = s.utf16_len()?;
        if units > max {
            return Err(napi::Error::from_reason(format!(
                "{what} byte length (at least {units}) exceeds maximum of {max}"
            )));
        }
        let len = s.utf8_len()?;
        if len > max {
            return Err(napi::Error::from_reason(format!("{what} byte length {len} exceeds maximum of {max}")));
        }
        self.bytes = self.bytes.saturating_add(len);
        if self.bytes > MAX_INPUT_BYTES {
            return Err(napi::Error::from_reason(format!(
                "input byte total {} exceeds maximum of {MAX_INPUT_BYTES}",
                self.bytes
            )));
        }
        s.into_utf8()?.into_owned()
    }

    /// An optional string property: `None` when absent or `undefined`.
    pub(crate) fn opt_string_prop(
        &mut self,
        obj: &Object<'_>,
        name: &str,
        what: &str,
        max: usize,
    ) -> napi::Result<Option<String>> {
        match prop(obj, name)? {
            Some(value) => self.string(value, what, max).map(Some),
            None => Ok(None),
        }
    }

    /// An optional string property where `null` also means absent, for fields whose TypeScript type admits `null`.
    fn nullable_string_prop(&mut self, obj: &Object<'_>, name: &str, max: usize) -> napi::Result<Option<String>> {
        match prop(obj, name)? {
            Some(value) if !is_nullish(value)? => self.string(value, name, max).map(Some),
            _ => Ok(None),
        }
    }

    /// A required string property.
    pub(crate) fn string_prop(&mut self, obj: &Object<'_>, name: &str, what: &str, max: usize) -> napi::Result<String> {
        self.opt_string_prop(obj, name, what, max)?.ok_or_else(|| missing(name))
    }

    /// An array of strings: its length checked against `max_len` first, then each string against `max_item`.
    pub(crate) fn strings(
        &mut self,
        list: &List<'_>,
        what_len: &str,
        max_len: usize,
        what_item: &str,
        max_item: usize,
    ) -> napi::Result<Vec<String>> {
        check_len(what_len, list.len(), max_len)?;
        let mut out = Vec::with_capacity(list.len());
        for i in 0..list.len {
            out.push(self.string(list.get(i)?, what_item, max_item)?);
        }
        Ok(out)
    }

    /// One `JsToonValue` object. Only the payload field its `type` names (plus `decimalsVal` for floats) is read, so
    /// an unused payload costs nothing.
    fn toon_value(&mut self, value: Unknown<'_>) -> napi::Result<JsToonValue> {
        let obj = object(value, "JsToonValue")?;
        let r#type = self.string_prop(&obj, "type", "JsToonValue type", MAX_NAME_BYTES)?;
        let mut out = JsToonValue { r#type, ..JsToonValue::default() };
        match out.r#type.as_str() {
            "str" => out.str_val = self.opt_string_prop(&obj, "strVal", "strVal", MAX_STRING_BYTES)?,
            "int" => out.int_val = typed_prop(&obj, "intVal")?,
            "float" => {
                out.float_val = typed_prop(&obj, "floatVal")?;
                out.decimals_val = typed_prop(&obj, "decimalsVal")?;
            }
            "bool" => out.bool_val = typed_prop(&obj, "boolVal")?,
            _ => {}
        }
        Ok(out)
    }

    /// `rows`: the row count and every row's length are checked, and the cells charged, before any cell is read.
    pub(crate) fn rows(
        &mut self,
        rows: &List<'_>,
        max_rows: usize,
        max_row_len: usize,
        row_len_limit: &str,
    ) -> napi::Result<Vec<Vec<JsToonValue>>> {
        check_len("rows", rows.len(), max_rows)?;
        let mut lists = Vec::with_capacity(rows.len());
        for i in 0..rows.len {
            let row = List::new(rows.get(i)?, "each row")?;
            if row.len() > max_row_len {
                return Err(napi::Error::from_reason(format!(
                    "row length {} exceeds {row_len_limit} of {max_row_len}",
                    row.len()
                )));
            }
            self.charge_cells(row.len())?;
            lists.push(row);
        }
        let mut out = Vec::with_capacity(lists.len());
        for row in &lists {
            let mut cells = Vec::with_capacity(row.len());
            for j in 0..row.len {
                cells.push(self.toon_value(row.get(j)?)?);
            }
            out.push(cells);
        }
        Ok(out)
    }

    /// `JsKvItem[]`, at most `max` items.
    pub(crate) fn kv_items(&mut self, items: &List<'_>, max: usize) -> napi::Result<Vec<JsKvItem>> {
        check_len("items", items.len(), max)?;
        let mut out = Vec::with_capacity(items.len());
        for i in 0..items.len {
            let obj = object(items.get(i)?, "JsKvItem")?;
            let key = self.string_prop(&obj, "key", "key", MAX_NAME_BYTES)?;
            let value = self.toon_value(prop(&obj, "value")?.ok_or_else(|| missing("value"))?)?;
            out.push(JsKvItem { key, value });
        }
        Ok(out)
    }

    /// `JsStatusItem[]`, at most `max` items.
    pub(crate) fn status_items(&mut self, items: &List<'_>, max: usize) -> napi::Result<Vec<JsStatusItem>> {
        check_len("items", items.len(), max)?;
        let mut out = Vec::with_capacity(items.len());
        for i in 0..items.len {
            let obj = object(items.get(i)?, "JsStatusItem")?;
            let key = self.string_prop(&obj, "key", "key", MAX_NAME_BYTES)?;
            let value = self.toon_value(prop(&obj, "value")?.ok_or_else(|| missing("value"))?)?;
            let health = self.opt_string_prop(&obj, "health", "health", MAX_STRING_BYTES)?;
            out.push(JsStatusItem { key, value, health });
        }
        Ok(out)
    }

    /// One `JsRecoveryHint` object.
    fn recovery_hint(&mut self, value: Unknown<'_>) -> napi::Result<JsRecoveryHint> {
        let obj = object(value, "JsRecoveryHint")?;
        let tool = self.string_prop(&obj, "tool", "tool", MAX_NAME_BYTES)?;
        let reason = self.opt_string_prop(&obj, "reason", "reason", MAX_STRING_BYTES)?;
        Ok(JsRecoveryHint { tool, reason })
    }

    /// `JsFailedOp[]`, at most `max` items. `recovery` may be absent, `undefined` or `null`.
    pub(crate) fn failed_ops(&mut self, ops: &List<'_>, max: usize) -> napi::Result<Vec<JsFailedOp>> {
        check_len("failed", ops.len(), max)?;
        let mut out = Vec::with_capacity(ops.len());
        for i in 0..ops.len {
            let obj = object(ops.get(i)?, "JsFailedOp")?;
            let operation = self.string_prop(&obj, "operation", "operation", MAX_NAME_BYTES)?;
            let reason = self.string_prop(&obj, "reason", "reason", MAX_STRING_BYTES)?;
            let recovery = match prop(&obj, "recovery")? {
                Some(value) if !is_nullish(value)? => Some(self.recovery_hint(value)?),
                _ => None,
            };
            out.push(JsFailedOp { operation, reason, recovery });
        }
        Ok(out)
    }

    /// `JsPipelineStep[]`, at most `max` items. `reason` may be absent, `undefined` or `null`.
    pub(crate) fn pipeline_steps(&mut self, steps: &List<'_>, max: usize) -> napi::Result<Vec<JsPipelineStep>> {
        check_len("steps", steps.len(), max)?;
        let mut out = Vec::with_capacity(steps.len());
        for i in 0..steps.len {
            let obj = object(steps.get(i)?, "JsPipelineStep")?;
            let id = self.string_prop(&obj, "id", "id", MAX_NAME_BYTES)?;
            let name = self.string_prop(&obj, "name", "name", MAX_STRING_BYTES)?;
            let status = self.string_prop(&obj, "status", "status", MAX_NAME_BYTES)?;
            let reason = self.nullable_string_prop(&obj, "reason", MAX_NAME_BYTES)?;
            out.push(JsPipelineStep { id, name, status, reason });
        }
        Ok(out)
    }

    /// `JsRecoveryHint[]`, at most `max` items.
    pub(crate) fn recovery_hints(&mut self, hints: &List<'_>, max: usize) -> napi::Result<Vec<JsRecoveryHint>> {
        check_len("hints", hints.len(), max)?;
        let mut out = Vec::with_capacity(hints.len());
        for i in 0..hints.len {
            out.push(self.recovery_hint(hints.get(i)?)?);
        }
        Ok(out)
    }
}
