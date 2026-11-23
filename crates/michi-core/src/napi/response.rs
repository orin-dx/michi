//! `AgentResponse`: the builder class, its cumulative byte accounting, and `toCallToolResult`.

use napi::bindgen_prelude::Unknown;
use napi_derive::napi;

use super::cost;
use super::input::{self, Budget, List, MAX_FIELDS, MAX_HINTS, MAX_NAME_BYTES, MAX_ROWS, MAX_STEPS, MAX_STRING_BYTES};
use super::num::JsCount;
use super::outcome::{JsCompletedOp, JsFailedOp, JsPipelineStep, JsSkippedOp};
use super::toon::JsStatusItem;
use super::value::{cell_cost, JsKvItem, JsToonValue};

/// MCP content-block annotations (JavaScript-friendly). Currently carries
/// only `audience` — michi has no concept of MCP's optional `priority`.
#[napi(object, js_name = "Annotations")]
#[derive(Debug, Clone)]
pub struct JsAnnotations {
    /// `["assistant"]` or `["user"]` — which surface(s) this block targets.
    pub audience: Vec<String>,
}

/// One MCP content block (JavaScript-friendly). Wire-conformant with MCP's
/// text content shape: `{type: "text", text, annotations: {audience: [...]}}`.
#[napi(object, js_name = "ContentBlock")]
#[derive(Debug, Clone)]
pub struct JsContentBlock {
    /// Always `"text"` — michi only ever produces text content blocks.
    #[napi(js_name = "type")]
    pub content_type: String,
    /// The block's text content.
    pub text: String,
    /// Which surface(s) this block is meant for.
    pub annotations: JsAnnotations,
}

/// The MCP `CallToolResult` shape, returned by `AgentResponse.toCallToolResult`.
#[napi(object, js_name = "CallToolResult")]
#[derive(Debug, Clone)]
pub struct JsCallToolResult {
    /// Text content blocks.
    pub content: Vec<JsContentBlock>,
    /// Whether this is a tool execution error.
    #[napi(js_name = "isError")]
    pub is_error: bool,
    /// The JSON companion to `content[0]` (the `renderJson()` object), as a
    /// real parsed JSON value — not a string the caller has to `JSON.parse()`
    /// themselves.
    #[napi(js_name = "structuredContent")]
    pub structured_content: serde_json::Value,
}

/// Bytes charged to a response's total for each section beyond its strings: roughly the size of the `Section` itself.
const SECTION_OVERHEAD: usize = 64;

/// Bytes charged for an error beyond its message and causes: the code, the struct, and the verify-first hint a
/// timeout carries.
const ERROR_OVERHEAD: usize = 256;

/// String bytes one [`JsAgentResponse`] holds, per slot. A slot that a call replaces (`truncationHint`,
/// `humanContent`, `error`) gives its old bytes back; `items`, `kvItems`, `hint`, `recoveryHint` and the outcome
/// sections (`partialSuccess`, `pipeline`, `status`, `alreadyDone`) each add a block, so they only add.
#[derive(Debug, Default)]
struct HeldBytes {
    type_name: usize,
    items: usize,
    kv_items: usize,
    hints: usize,
    recovery: usize,
    truncation_hint: usize,
    human_content: usize,
    error: usize,
    outcomes: usize,
}

impl HeldBytes {
    fn total(&self) -> usize {
        [
            self.type_name,
            self.items,
            self.kv_items,
            self.hints,
            self.recovery,
            self.truncation_hint,
            self.human_content,
            self.error,
            self.outcomes,
        ]
        .iter()
        .fold(0, |acc, n| acc.saturating_add(*n))
    }

    /// An error if replacing a slot holding `old` bytes with `new` would take the total past
    /// [`input::MAX_INPUT_BYTES`]. Changes nothing: the caller stores the new slot size once its call succeeds.
    fn check_replace(&self, old: usize, new: usize) -> napi::Result<()> {
        let total = self.total().saturating_sub(old).saturating_add(new);
        if total > input::MAX_INPUT_BYTES {
            return Err(napi::Error::from_reason(format!(
                "cumulative byte total {total} exceeds maximum of {}",
                input::MAX_INPUT_BYTES
            )));
        }
        Ok(())
    }
}

/// What one row costs beyond its cells: the row vector. `Array(100000).fill([])` has no cells and still keeps
/// 100,000 of these.
const ROW_OVERHEAD: usize = std::mem::size_of::<Vec<crate::toon::Value>>();

/// What one field name costs: its string and the `String` that holds it.
fn field_cost(f: &str) -> usize {
    std::mem::size_of::<String>().saturating_add(f.len())
}

/// What one row of a four-column outcome table (`partialSuccess`, `pipeline`, `status`) keeps beyond its strings.
const OUTCOME_ROW: usize = ROW_OVERHEAD + 4 * std::mem::size_of::<crate::toon::Value>();

/// What a named outcome section costs before its rows: its name, which is written in its header and again in the
/// `totalCount` table, and the section itself.
fn outcome_section_cost(name: &str) -> usize {
    field_cost(name).saturating_add(name.len()).saturating_add(SECTION_OVERHEAD)
}

/// The sum of `costs`, saturating.
fn sum(costs: impl IntoIterator<Item = usize>) -> usize {
    costs.into_iter().fold(0, usize::saturating_add)
}

/// NAPI wrapper around [`crate::response::AgentResponse`].
///
/// Besides each call's own limits, one object holds at most 16 MiB across all its calls: caller strings (type name,
/// fields, cells, keys, hints, recovery hints, truncation hint and human content) plus the storage rows and cells
/// keep, so a table of empty rows, numbers or nulls is charged too. A call that would pass that total
/// is rejected and leaves the builder unchanged; replacing `truncationHint` or `humanContent` gives the old value's
/// bytes back first. Every `items` and `kvItems` call adds a section, so their bytes add up; each section is also
/// charged the type name's bytes plus 64, so empty calls are not free.
///
/// The total bounds the output too: a render is at most 19 times the bytes held, plus 4 KiB, on every surface (text,
/// JSON, `toCallToolResult`), under V8's string limit; see [`input::MAX_INPUT_BYTES`]. Control characters, which
/// escape to six bytes in the text and again in the JSON, are the worst case for a string. A name can be written
/// more often than a string, so each is charged for every place it can be written: a field name, and the type name
/// or an outcome section's name, once for every cell of the table that is longer than 200 chars (a `truncated` row
/// repeats them for every cut cell), a section's name twice (its header and the `totalCount` table), and each hint
/// and recovery hint a few bytes for the row or object that holds it.
///
/// Both methods name their section for the type name, and a section is a root key in the rendered document. Two
/// `items` calls, or a `kvItems` key equal to the type name, therefore render the `toon_validation_failed` error
/// (`isError` true) instead of invalid TOON. The JS surface renders one list per response.
#[napi(js_name = "AgentResponse")]
#[derive(Debug)]
pub struct JsAgentResponse {
    inner: Option<crate::response::AgentResponse>,
    /// The type name, which names the sections `items` and `kvItems` append.
    name: String,
    /// Cumulative count of [`Self::hint`] calls, capped at [`MAX_HINTS`].
    hint_count: usize,
    /// Cumulative count of [`Self::recovery_hint`] calls, capped at [`MAX_HINTS`].
    recovery_count: usize,
    /// String bytes held, capped at [`input::MAX_INPUT_BYTES`] in all.
    held: HeldBytes,
}

impl JsAgentResponse {
    /// What every section costs before its own strings: a copy of the type name, which names it, plus a fixed
    /// allowance for the section itself. Without it an empty `items` or `kvItems` call would be free, and a loop of
    /// them would grow the response past the cap.
    ///
    /// A list writes its name in its header and again in the `totalCount` table, a KV section only in the table, and
    /// each also writes it once in the JSON `sections`. That is two writes of the name per section, and the name is
    /// charged twice: here, and once when the response is built (`HeldBytes::type_name`). Only one list can carry the
    /// type name, since a second is a root-key collision that replaces the body with a short error, and every KV
    /// section writes it at most once in the body.
    fn section_cost(&self) -> usize {
        self.name.len().saturating_add(SECTION_OVERHEAD)
    }

    fn take(&mut self) -> napi::Result<crate::response::AgentResponse> {
        self.inner.take().ok_or_else(consumed)
    }

    fn built(&self) -> napi::Result<&crate::response::AgentResponse> {
        self.inner.as_ref().ok_or_else(consumed)
    }

    /// Append a TOON list section. The caller has bounded `rows`, `fields` and every row's length (see
    /// [`Self::items`]); this charges the bytes they keep, then converts, so the builder is unchanged on error.
    fn add_items(
        &mut self,
        rows: Vec<Vec<JsToonValue>>,
        fields: Vec<String>,
        total_count: Option<JsCount>,
    ) -> napi::Result<()> {
        let bytes = fields
            .iter()
            .map(|f| field_cost(f.as_str()))
            .chain(rows.iter().map(|row| row.iter().map(cell_cost).fold(ROW_OVERHEAD, usize::saturating_add)))
            .chain([cost::table_repeats(&rows, &fields, self.name.len())])
            .fold(self.section_cost(), usize::saturating_add);
        self.held.check_replace(0, bytes)?;
        let converted: Vec<Vec<crate::toon::Value>> = rows
            .into_iter()
            .map(|row| row.into_iter().map(|v| Ok(v.into_cell()?.into())).collect::<napi::Result<Vec<_>>>())
            .collect::<napi::Result<Vec<_>>>()?;
        let b = self.take()?;
        let section = crate::section::Section::toon(self.name.clone(), fields, converted)
            .total_count(total_count.map(JsCount::get));
        self.inner = Some(b.section(section));
        self.held.items = self.held.items.saturating_add(bytes);
        Ok(())
    }

    /// Append a KV section: the same accounting as [`Self::add_items`].
    fn add_kv_items(&mut self, items: Vec<JsKvItem>, total_count: Option<JsCount>) -> napi::Result<()> {
        let bytes = items
            .iter()
            .map(|i| std::mem::size_of::<String>().saturating_add(i.key.len()).saturating_add(cell_cost(&i.value)))
            .fold(self.section_cost(), usize::saturating_add);
        self.held.check_replace(0, bytes)?;
        let converted = items
            .into_iter()
            .map(|i| Ok(crate::kv::KvItem::new(i.key, i.value.into_cell()?.into())))
            .collect::<napi::Result<Vec<_>>>()?;
        let b = self.take()?;
        let section =
            crate::section::Section::kv(self.name.clone(), converted).total_count(total_count.map(JsCount::get));
        self.inner = Some(b.section(section));
        self.held.kv_items = self.held.kv_items.saturating_add(bytes);
        Ok(())
    }

    /// Charge `bytes` to the outcome slot, then apply `add` to the builder. The builder is unchanged on error.
    fn add_outcome(
        &mut self,
        bytes: usize,
        add: impl FnOnce(crate::response::AgentResponse) -> crate::response::AgentResponse,
    ) -> napi::Result<()> {
        self.held.check_replace(0, bytes)?;
        let b = self.take()?;
        self.inner = Some(add(b));
        self.held.outcomes = self.held.outcomes.saturating_add(bytes);
        Ok(())
    }

    /// Set the error block; see [`Self::error`].
    fn set_error(
        &mut self,
        code: &str,
        message: String,
        causes: Vec<String>,
        retryable: Option<bool>,
    ) -> napi::Result<()> {
        // A cause is written like a hint (its text plus a quote or separator per surface), so it is charged like one.
        // Charging the `String` that holds it as well cut the capacity of one error report below what the per-call
        // string cap allows.
        let bytes = sum(causes
            .iter()
            .map(|c| c.len().saturating_add(cost::HINT_OVERHEAD))
            .chain([message.len(), ERROR_OVERHEAD]));
        self.held.check_replace(self.held.error, bytes)?;
        let mut err = super::outcome::build_domain_error(code, message, causes)?;
        if let Some(retryable) = retryable {
            err = err.retryable(retryable);
        }
        let b = self.take()?;
        self.inner = Some(b.error(err));
        self.held.error = bytes;
        Ok(())
    }

    /// Append the per-operation outcomes table; see [`Self::partial_success`].
    fn add_partial_success(
        &mut self,
        name: String,
        completed: Vec<JsCompletedOp>,
        failed: Vec<JsFailedOp>,
        skipped: Vec<JsSkippedOp>,
    ) -> napi::Result<()> {
        let rows = completed.len() + failed.len() + skipped.len();
        let strings = sum(completed
            .iter()
            .map(|o| o.operation.len())
            .chain(failed.iter().map(|o| {
                let recovery = o
                    .recovery
                    .as_ref()
                    .map_or(0, |r| SECTION_OVERHEAD + r.tool.len() + r.reason.as_ref().map_or(0, String::len));
                o.operation.len() + o.reason.len() + recovery
            }))
            .chain(skipped.iter().map(|o| o.operation.len() + o.reason.as_ref().map_or(0, String::len))));
        let cut = cost::cut_count(
            completed
                .iter()
                .map(|o| o.operation.as_str())
                .chain(failed.iter().flat_map(|o| [o.operation.as_str(), o.reason.as_str()]))
                .chain(skipped.iter().flat_map(|o| [o.operation.as_str(), o.reason.as_deref().unwrap_or("")])),
        );
        let repeats = cost::fixed_table_repeats(cut, name.len());
        let bytes = sum([outcome_section_cost(&name), strings, rows.saturating_mul(OUTCOME_ROW), repeats]);
        let outcome = super::outcome::build_partial_success(completed, failed, skipped)?;
        self.add_outcome(bytes, |b| b.partial_success(name, outcome))
    }

    /// Append the pipeline table; see [`Self::pipeline`].
    fn add_pipeline(&mut self, name: String, steps: Vec<JsPipelineStep>) -> napi::Result<()> {
        let strings = sum(steps.iter().map(|s| s.id.len() + s.name.len()));
        let cut = cost::cut_count(steps.iter().flat_map(|s| [s.id.as_str(), s.name.as_str()]));
        let repeats = cost::fixed_table_repeats(cut, name.len());
        let bytes = sum([outcome_section_cost(&name), strings, steps.len().saturating_mul(OUTCOME_ROW), repeats]);
        let pipeline = super::outcome::build_pipeline(steps)?;
        self.add_outcome(bytes, |b| b.pipeline(name, pipeline))
    }

    /// Append the status sections; see [`Self::status`].
    fn add_status(&mut self, tool_name: String, description: String, items: Vec<JsStatusItem>) -> napi::Result<()> {
        let strings =
            sum(items.iter().map(|i| i.key.len() + cell_cost(&i.value) + i.health.as_ref().map_or(0, String::len)));
        let head = sum([tool_name.len(), description.len(), 2 * SECTION_OVERHEAD]);
        let cut = cost::cut_count(items.iter().flat_map(|i| {
            [i.key.as_str(), i.value.str_val.as_deref().unwrap_or(""), i.health.as_deref().unwrap_or("")]
        }));
        let repeats = cost::fixed_table_repeats(cut, "items".len());
        let bytes = sum([head, strings, items.len().saturating_mul(OUTCOME_ROW), repeats]);
        let status = super::toon::build_status(tool_name, description, items)?;
        self.add_outcome(bytes, |b| b.status(status))
    }

    /// Append the already-done section; see [`Self::already_done`].
    fn add_already_done(&mut self, operation: String, summary: String) -> napi::Result<()> {
        let bytes = sum([operation.len(), summary.len(), outcome_section_cost("already_done"), 3 * OUTCOME_ROW]);
        self.add_outcome(bytes, |b| b.already_done(operation, summary))
    }
}

fn consumed() -> napi::Error {
    napi::Error::from_reason("AgentResponse already consumed")
}

#[napi]
impl JsAgentResponse {
    /// Create a new response builder for the given type name, `new AgentResponse(typeName)`.
    ///
    /// # Errors
    ///
    /// Rejects a `typeName` over 4 KiB.
    #[napi(constructor, catch_unwind)]
    pub fn new(type_name: String) -> napi::Result<Self> {
        input::check_bytes("typeName", &type_name, MAX_NAME_BYTES)?;
        let held = HeldBytes { type_name: type_name.len(), ..HeldBytes::default() };
        let inner = Some(crate::response::AgentResponse::new(type_name.clone()));
        Ok(Self { inner, name: type_name, hint_count: 0, recovery_count: 0, held })
    }

    /// Append a TOON list section named for the response's type. Every call adds a section, rendered in call order;
    /// a second `items` call repeats the root key and renders the validation error (see the type docs).
    /// `totalCount`, when given, is how many rows existed before any cut; `totalCount: n` renders only when `n` is
    /// larger than the rows passed.
    ///
    /// # Errors
    ///
    /// Rejects, before converting anything, more than 100,000 rows, 1,000 fields or 1,000 values in a row, and
    /// more than 1,000,000 cells in all; while converting, a field name over 4 KiB, a cell over 1 MiB, more than
    /// 16 MiB of strings in the call, or an invalid cell; a `totalCount` that is not a finite, non-negative integer;
    /// then strings that would take the response past its 16 MiB total. The builder is unchanged on error.
    #[napi(
        ts_args_type = "rows: Array<Array<ToonValue>>, fields: Array<string>, totalCount?: number | null",
        catch_unwind
    )]
    pub fn items(&mut self, rows: Unknown<'_>, fields: Unknown<'_>, total_count: Option<JsCount>) -> napi::Result<()> {
        let mut budget = Budget::new();
        let rows = List::new(rows, "rows")?;
        let fields = List::new(fields, "fields")?;
        input::check_len("rows", rows.len(), MAX_ROWS)?;
        input::check_len("fields", fields.len(), MAX_FIELDS)?;
        let fields = budget.strings(&fields, "fields", MAX_FIELDS, "field name", MAX_NAME_BYTES)?;
        let rows = budget.rows(&rows, MAX_ROWS, MAX_FIELDS, "maximum")?;
        self.add_items(rows, fields, total_count)
    }

    /// Append a KV section named for the response's type; the name is not printed. Every call adds a section,
    /// rendered in call order. `totalCount` works as in [`Self::items`].
    ///
    /// # Errors
    ///
    /// Rejects more than 1,000 items before converting any, a key over 4 KiB, a value over 1 MiB, more than 16 MiB
    /// of strings in the call, an invalid value, a `totalCount` that is not a finite, non-negative integer, or
    /// strings that would take the response past its 16 MiB total. The builder is unchanged on error.
    #[napi(js_name = "kvItems", ts_args_type = "items: Array<KvItem>, totalCount?: number | null", catch_unwind)]
    pub fn kv_items(&mut self, items: Unknown<'_>, total_count: Option<JsCount>) -> napi::Result<()> {
        let mut budget = Budget::new();
        let items = List::new(items, "items")?;
        let items = budget.kv_items(&items, MAX_FIELDS)?;
        self.add_kv_items(items, total_count)
    }

    /// Report a classified error: it renders first, as `error: <code>`, `message`, `causes[N]` and `exit_code: 1`, and
    /// the response is an error on every surface. In `renderJson()` and `structuredContent` it is the object
    /// `error: {code, message, causes, retryable}`, beside the response's `hints` and `recovery` arrays (add those
    /// with `hint()` and `recoveryHint()`). `retryable` defaults to the code's (true for `rate_limited` and
    /// `unavailable`); a `timeout` that is not retryable gets michi's verify-first hint. One per response: a second
    /// call replaces it.
    ///
    /// # Errors
    ///
    /// Rejects an unknown `code`, a `code` over 4 KiB, a `message` or cause over 1 MiB, more than 10,000 causes, more
    /// than 16 MiB of strings in the call, or an error that would take the response past its 16 MiB total. The
    /// builder is unchanged on error.
    #[napi(
        catch_unwind,
        ts_args_type = "code: 'invalid_input' | 'not_found' | 'unauthorized' | 'forbidden' | 'conflict' | 'rate_limited' \
                        | 'unavailable' | 'timeout' | 'external_failure', message: string, \
                        causes?: Array<string> | undefined | null, retryable?: boolean | undefined | null"
    )]
    pub fn error(
        &mut self,
        code: Unknown<'_>,
        message: Unknown<'_>,
        causes: Option<Unknown<'_>>,
        retryable: Option<bool>,
    ) -> napi::Result<()> {
        let mut budget = Budget::new();
        let causes = List::optional(causes, "causes")?;
        if let Some(causes) = &causes {
            input::check_len("causes", causes.len(), MAX_HINTS)?;
        }
        let code = budget.string(code, "code", MAX_NAME_BYTES)?;
        let message = budget.string(message, "message", MAX_STRING_BYTES)?;
        let causes = match &causes {
            Some(causes) => budget.strings(causes, "causes", MAX_HINTS, "cause", MAX_STRING_BYTES)?,
            None => Vec::new(),
        };
        self.set_error(&code, message, causes, retryable)
    }

    /// Append a batch's per-operation outcomes as the list section `name`: `name[N]{index,operation,status,reason}:`,
    /// a row per operation sorted by `index`. The failed operations' recovery hints join the response's `recovery`.
    /// A partial batch is not an error: call `error()` or `asError()` when the call failed as a whole.
    ///
    /// # Errors
    ///
    /// Rejects an `index` that appears more than once across the three lists, a completed operation with a `reason`,
    /// more than 10,000 entries in a list, a `name`, `operation`, skipped `reason` or recovery `tool` over 4 KiB, a
    /// failed `reason` over 1 MiB, an `index` that is not an integer in `0..=2^53-1`, more than 16 MiB of strings in
    /// the call, or entries that would take the response past its 16 MiB total. The builder is unchanged on error.
    #[napi(
        catch_unwind,
        ts_args_type = "name: string, completed: Array<CompletedOp>, failed: Array<FailedOp>, skipped: Array<SkippedOp>"
    )]
    pub fn partial_success(
        &mut self,
        name: Unknown<'_>,
        completed: Unknown<'_>,
        failed: Unknown<'_>,
        skipped: Unknown<'_>,
    ) -> napi::Result<()> {
        let mut budget = Budget::new();
        let completed = List::new(completed, "completed")?;
        let failed = List::new(failed, "failed")?;
        let skipped = List::new(skipped, "skipped")?;
        input::check_len("completed", completed.len(), MAX_STEPS)?;
        input::check_len("failed", failed.len(), MAX_STEPS)?;
        input::check_len("skipped", skipped.len(), MAX_STEPS)?;
        let name = budget.string(name, "name", MAX_NAME_BYTES)?;
        let completed = budget.completed_ops(&completed, MAX_STEPS)?;
        let failed = budget.failed_ops(&failed, MAX_STEPS)?;
        let skipped = budget.skipped_ops(&skipped, MAX_STEPS)?;
        self.add_partial_success(name, completed, failed, skipped)
    }

    /// Append pipeline state as the list section `name`: `name[N]{id,name,status,reason}:`, a row per step.
    ///
    /// # Errors
    ///
    /// Rejects an unknown `status`, a missing or unknown `reason` for `outcome_unknown`, `skipped` or `not_run`, a
    /// `reason` on any other status, more than 10,000 steps, a `name`, `id`, `status` or `reason` over 4 KiB, a step
    /// `name` over 1 MiB, more than 16 MiB of strings in the call, or steps that would take the response past its
    /// 16 MiB total. The builder is unchanged on error.
    #[napi(catch_unwind, ts_args_type = "name: string, steps: Array<PipelineStep>")]
    pub fn pipeline(&mut self, name: Unknown<'_>, steps: Unknown<'_>) -> napi::Result<()> {
        let mut budget = Budget::new();
        let steps = List::new(steps, "steps")?;
        input::check_len("steps", steps.len(), MAX_STEPS)?;
        let name = budget.string(name, "name", MAX_NAME_BYTES)?;
        let steps = budget.pipeline_steps(&steps, MAX_STEPS)?;
        self.add_pipeline(name, steps)
    }

    /// Append a status report: `tool` and `description` lines, then `items[N]{key,value,health,reason}:`. An item
    /// without `health` renders `ok`. Add hints with `hint()`.
    ///
    /// # Errors
    ///
    /// Rejects an unknown `health`, more than 1,000 items, a `toolName` or key over 4 KiB, a `description`, value or
    /// `health` over 1 MiB, more than 16 MiB of strings in the call, or items that would take the response past its
    /// 16 MiB total. The builder is unchanged on error.
    #[napi(catch_unwind, ts_args_type = "toolName: string, description: string, items: Array<StatusItem>")]
    pub fn status(&mut self, tool_name: Unknown<'_>, description: Unknown<'_>, items: Unknown<'_>) -> napi::Result<()> {
        let mut budget = Budget::new();
        let items = List::new(items, "items")?;
        input::check_len("items", items.len(), MAX_FIELDS)?;
        let tool_name = budget.string(tool_name, "toolName", MAX_NAME_BYTES)?;
        let description = budget.string(description, "description", MAX_STRING_BYTES)?;
        let items = budget.status_items(&items, MAX_FIELDS)?;
        self.add_status(tool_name, description, items)
    }

    /// Append an already-done report: `operation`, `status: already_done` and `summary` lines.
    ///
    /// # Errors
    ///
    /// Rejects an `operation` over 4 KiB, a `summary` over 1 MiB, or text that would take the response past its
    /// 16 MiB total. The builder is unchanged on error.
    #[napi(catch_unwind, ts_args_type = "operation: string, summary: string")]
    pub fn already_done(&mut self, operation: Unknown<'_>, summary: Unknown<'_>) -> napi::Result<()> {
        let mut budget = Budget::new();
        let operation = budget.string(operation, "operation", MAX_NAME_BYTES)?;
        let summary = budget.string(summary, "summary", MAX_STRING_BYTES)?;
        self.add_already_done(operation, summary)
    }

    /// Append a contextual hint.
    ///
    /// # Errors
    ///
    /// Rejects a hint over 1 MiB, any hint after the 10,000th, and a hint that would take the response past its
    /// 16 MiB total.
    #[napi(catch_unwind)]
    pub fn hint(&mut self, hint: String) -> napi::Result<()> {
        if self.hint_count >= MAX_HINTS {
            return Err(napi::Error::from_reason(format!("cumulative hint count exceeds maximum of {MAX_HINTS}")));
        }
        input::check_bytes("hint", &hint, MAX_STRING_BYTES)?;
        let bytes = hint.len().saturating_add(cost::HINT_OVERHEAD);
        self.held.check_replace(0, bytes)?;
        let b = self.take()?;
        self.inner = Some(b.hint(hint));
        self.hint_count += 1;
        self.held.hints = self.held.hints.saturating_add(bytes);
        Ok(())
    }

    /// Append a recovery hint naming a tool.
    ///
    /// # Errors
    ///
    /// Rejects a `tool` over 4 KiB, a `reason` over 1 MiB, any recovery hint after the 10,000th, and one that would
    /// take the response past its 16 MiB total.
    #[napi(catch_unwind)]
    pub fn recovery_hint(&mut self, tool: String, reason: Option<String>) -> napi::Result<()> {
        if self.recovery_count >= MAX_HINTS {
            return Err(napi::Error::from_reason(format!(
                "cumulative recovery hint count exceeds maximum of {MAX_HINTS}"
            )));
        }
        input::check_bytes("tool", &tool, MAX_NAME_BYTES)?;
        if let Some(reason) = &reason {
            input::check_bytes("reason", reason, MAX_STRING_BYTES)?;
        }
        let bytes = sum([tool.len(), reason.as_ref().map_or(0, String::len), cost::RECOVERY_OVERHEAD]);
        self.held.check_replace(0, bytes)?;
        let b = self.take()?;
        let mut hint = crate::recovery::RecoveryHint::new(tool);
        if let Some(reason) = reason {
            hint = hint.reason(reason);
        }
        self.inner = Some(b.recovery_hint(hint));
        self.recovery_count += 1;
        self.held.recovery = self.held.recovery.saturating_add(bytes);
        Ok(())
    }

    /// Set the caller's instruction for getting a cut TOON cell in full (e.g. `"full=true"`), delivered in `help[]`
    /// after the response's own hints when a cell was cut. Not called: the `truncated` block still reports each cut.
    /// Tool-author text, never data.
    ///
    /// # Errors
    ///
    /// Rejects a hint over 4 KiB, or one that would take the response past its 16 MiB total. A hint over 256 chars is
    /// accepted here but renders as a `toon_validation_failed` error block, like any other options michi-core's TOON
    /// validation rejects.
    #[napi(catch_unwind)]
    pub fn truncation_hint(&mut self, hint: String) -> napi::Result<()> {
        input::check_bytes("truncationHint", &hint, MAX_NAME_BYTES)?;
        let bytes = hint.len();
        self.held.check_replace(self.held.truncation_hint, bytes)?;
        let b = self.take()?;
        self.inner = Some(b.truncation_hint(hint));
        self.held.truncation_hint = bytes;
        Ok(())
    }

    /// Mark this response as an error state.
    ///
    /// # Errors
    ///
    /// Returns an error if the builder was already consumed (an internal invariant; should not happen in normal use).
    #[napi(catch_unwind)]
    pub fn as_error(&mut self) -> napi::Result<()> {
        let b = self.take()?;
        self.inner = Some(b.as_error());
        Ok(())
    }

    /// Attach a human-facing companion block (`audience: user`) for MCP callers. A second call replaces the first.
    ///
    /// # Errors
    ///
    /// Rejects text over 1 MiB, or text that would take the response past its 16 MiB total. The builder is unchanged
    /// on error.
    #[napi(catch_unwind)]
    pub fn human_content(&mut self, text: String) -> napi::Result<()> {
        input::check_bytes("humanContent", &text, MAX_STRING_BYTES)?;
        let bytes = text.len();
        self.held.check_replace(self.held.human_content, bytes)?;
        let b = self.take()?;
        self.inner = Some(b.human_content(text));
        self.held.human_content = bytes;
        Ok(())
    }

    /// Render the response as text: the error block, its sections in call order, then `help[]` and `recovery[]`. The
    /// one text render; see [`crate::response::AgentResponse::render`].
    ///
    /// # Errors
    ///
    /// Returns an error only if an internal invariant is violated (should not happen in normal use).
    #[napi(catch_unwind)]
    pub fn render_text(&self) -> napi::Result<String> {
        Ok(self.built()?.render(crate::response::OutputFormat::Text))
    }

    /// Render as a compact JSON string: `{"body":...,"error":{...},"sections":[...],"hints":[...],"recovery":[...],
    /// "isError":bool}`, with `error` and `sections` only when set.
    ///
    /// # Errors
    ///
    /// Returns an error only if an internal invariant is violated (should not happen in normal use).
    #[napi(catch_unwind)]
    pub fn render_json(&self) -> napi::Result<String> {
        Ok(self.built()?.render(crate::response::OutputFormat::Json))
    }

    /// Render just the `help[N]:` block — the three-surface seam for MCP
    /// frameworks assembling their own display body separately.
    ///
    /// # Errors
    ///
    /// Returns an error only if an internal invariant is violated (should not happen in normal use).
    #[napi(catch_unwind)]
    pub fn render_hints_only(&self) -> napi::Result<String> {
        Ok(self.built()?.render_hints_only())
    }

    /// Render for the given audience — `"assistant"` or `"user"`. `"user"`
    /// returns the `humanContent()` block if one was set, falling back to
    /// the same agent-oriented rendering `"assistant"` would produce
    /// otherwise — never an empty string. See [`Self::has_human_content`]
    /// to check for the fallback case ahead of time.
    ///
    /// # Errors
    ///
    /// Returns an error if `audience` is anything other than `"assistant"`
    /// or `"user"`, or if an internal invariant is violated (should not
    /// happen in normal use).
    #[napi(catch_unwind)]
    #[allow(clippy::needless_pass_by_value)] // napi-derive requires owned String for JS string params
    pub fn render_for(&self, audience: String) -> napi::Result<String> {
        let audience = match audience.as_str() {
            "assistant" => crate::audience::Audience::Assistant,
            "user" => crate::audience::Audience::User,
            other => {
                return Err(napi::Error::from_reason(format!(
                    "unknown audience {other:?}: expected \"assistant\" or \"user\""
                )))
            }
        };
        Ok(self.built()?.render_for(audience))
    }

    /// Whether `.humanContent()` was set on this builder.
    ///
    /// # Errors
    ///
    /// Returns an error only if an internal invariant is violated (should not happen in normal use).
    #[napi(catch_unwind)]
    pub fn has_human_content(&self) -> napi::Result<bool> {
        Ok(self.built()?.has_human_content())
    }

    /// Build the MCP `CallToolResult` for this response. Returns a real
    /// object, not a JSON string — `structuredContent` is a parsed
    /// `serde_json::Value`, so TypeScript callers don't need to
    /// `JSON.parse()` it themselves.
    ///
    /// # Errors
    ///
    /// Returns an error only if an internal invariant is violated (should not happen in normal use).
    #[napi(catch_unwind)]
    pub fn to_call_tool_result(&self) -> napi::Result<JsCallToolResult> {
        let result = self.built()?.to_call_tool_result();
        let structured_content =
            serde_json::from_str(result.structured_content.as_ref().map_or("null", |j| j.as_str()))
                .map_err(|e| napi::Error::from_reason(format!("structured_content was not valid JSON: {e}")))?;
        Ok(JsCallToolResult {
            content: result
                .content
                .into_iter()
                .map(|c| JsContentBlock {
                    content_type: "text".to_string(),
                    text: c.text,
                    annotations: JsAnnotations {
                        audience: c
                            .audience
                            .into_iter()
                            .map(|a| match a {
                                crate::audience::Audience::Assistant => "assistant".to_string(),
                                crate::audience::Audience::User => "user".to_string(),
                            })
                            .collect(),
                    },
                })
                .collect(),
            is_error: result.is_error,
            structured_content,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::num::JsInt;
    use super::*;

    fn new_response(name: impl Into<String>) -> JsAgentResponse {
        JsAgentResponse::new(name.into()).expect("a valid type name")
    }

    fn value(t: &str) -> JsToonValue {
        JsToonValue { r#type: t.to_string(), ..JsToonValue::default() }
    }

    #[test]
    fn js_agent_response_truncation_hint_reaches_toon_body() {
        let mut r = new_response("t".to_string());
        r.truncation_hint("full=true".to_string()).expect("fresh builder accepts a hint");
        r.add_items(
            vec![vec![JsToonValue { str_val: Some("a".repeat(500)), ..value("str") }]],
            vec!["a".to_string()],
            None,
        )
        .expect("valid rows");
        let out = r.render_text().expect("renders");
        assert_eq!(
            out,
            format!(
                "t[1]{{a}}:\n  {}\u{2026}\ntruncated[1]{{row,field,removed}}:\n  0,a,301\n\
                 help[1]: Cut cells are listed in the truncated block; use full=true for the full value",
                "a".repeat(199)
            )
        );
    }

    #[test]
    fn js_agent_response_items_then_render_text() {
        let mut r = new_response("issues".to_string());
        r.add_items(
            vec![vec![JsToonValue { int_val: Some(JsInt::try_from(1.0).expect("1 is a valid int")), ..value("int") }]],
            vec!["id".to_string()],
            None,
        )
        .unwrap();
        let out = r.render_text().unwrap();
        assert!(out.starts_with("issues[1]{id}:"), "got: {out}");
    }

    #[test]
    fn js_agent_response_items_rejects_invalid_value_and_stays_usable() {
        let mut r = new_response("issue".to_string());
        // `value("int")` has no `int_val` set, so conversion must fail.
        let err = r
            .add_items(vec![vec![value("int")]], vec!["a".to_string()], None)
            .expect_err("int value missing intVal must be rejected before consuming the builder");
        assert!(err.reason.contains("intVal is missing"), "got: {}", err.reason);

        // The rejected call must not have consumed the builder via `take()`:
        // a subsequent valid call should still succeed.
        r.add_items(vec![vec![value("null")]], vec!["a".to_string()], None)
            .expect("builder still usable after rejection");
    }

    #[test]
    fn js_agent_response_kv_items_rejects_invalid_value_and_stays_usable() {
        let mut r = new_response("issue".to_string());
        // `value("int")` has no `int_val` set, so conversion must fail.
        let err = r
            .add_kv_items(vec![JsKvItem { key: "id".to_string(), value: value("int") }], None)
            .expect_err("int value missing intVal must be rejected before consuming the builder");
        assert!(err.reason.contains("intVal is missing"), "got: {}", err.reason);

        // The rejected call must not have consumed the builder via `take()`:
        // a subsequent valid call should still succeed.
        r.add_kv_items(vec![JsKvItem { key: "id".to_string(), value: value("null") }], None)
            .expect("builder still usable after rejection");
    }

    #[test]
    fn js_agent_response_kv_items_then_render_text() {
        let mut r = new_response("issue".to_string());
        r.add_kv_items(vec![JsKvItem { key: "id".to_string(), value: value("null") }], None).unwrap();
        let out = r.render_text().unwrap();
        assert!(out.contains("id:"), "got: {out}");
    }

    #[test]
    fn js_agent_response_items_and_kv_items_render_both_in_call_order() {
        let int = |n: f64| JsToonValue { int_val: Some(JsInt::try_from(n).expect("a valid int")), ..value("int") };
        let kv_item = |n: f64| JsKvItem { key: "id".to_string(), value: int(n) };
        let mut r = new_response("issues".to_string());
        r.add_items(vec![vec![int(1.0)]], vec!["id".to_string()], None).unwrap();
        r.add_kv_items(vec![kv_item(99.0)], None).unwrap();
        assert_eq!(r.render_text().unwrap(), "issues[1]{id}:\n  1\nid: 99");

        let mut flipped = new_response("issues".to_string());
        flipped.add_kv_items(vec![kv_item(1.0)], None).unwrap();
        flipped.add_items(vec![vec![int(99.0)]], vec!["id".to_string()], None).unwrap();
        assert_eq!(flipped.render_text().unwrap(), "id: 1\nissues[1]{id}:\n  99");
    }

    #[test]
    fn js_agent_response_total_count_rides_on_the_section_and_shows_only_when_larger() {
        let int = |n: f64| JsToonValue { int_val: Some(JsInt::try_from(n).expect("a valid int")), ..value("int") };
        let count = |n: f64| Some(JsCount::try_from(n).expect("a valid count"));
        let mut r = new_response("t".to_string());
        r.add_items(vec![vec![int(1.0)]], vec!["id".to_string()], count(5.0)).unwrap();
        assert_eq!(r.render_text().unwrap(), "t[1]{id}:\n  1\ntotalCount: 5");
        let mut same = new_response("t".to_string());
        same.add_items(vec![vec![int(1.0)]], vec!["id".to_string()], count(1.0)).unwrap();
        assert_eq!(same.render_text().unwrap(), "t[1]{id}:\n  1");
        let mut kv = new_response("t".to_string());
        kv.add_kv_items(vec![JsKvItem { key: "id".to_string(), value: int(1.0) }], count(3.0)).unwrap();
        assert_eq!(kv.render_text().unwrap(), "id: 1\ntotalCount: 3");
    }

    #[test]
    fn js_agent_response_hint_and_render_hints_only() {
        let mut r = new_response("t".to_string());
        r.add_kv_items(vec![], None).unwrap();
        r.hint("do this".to_string()).unwrap();
        assert_eq!(r.render_hints_only().unwrap(), "help[1]: do this");
    }

    #[test]
    fn render_for_assistant_matches_render_text() {
        let mut r = new_response("issue".to_string());
        r.add_kv_items(
            vec![JsKvItem {
                key: "id".to_string(),
                value: JsToonValue { int_val: Some(JsInt::try_from(1.0).expect("1 is a valid int")), ..value("int") },
            }],
            None,
        )
        .unwrap();
        assert_eq!(r.render_for("assistant".to_string()).unwrap(), r.render_text().unwrap());
    }

    #[test]
    fn render_for_user_returns_human_content_when_set() {
        let mut r = new_response("t".to_string());
        r.add_kv_items(vec![], None).unwrap();
        r.human_content("hi there".to_string()).unwrap();
        assert_eq!(r.render_for("user".to_string()).unwrap(), "hi there");
    }

    #[test]
    fn render_for_user_falls_back_when_unset() {
        let mut r = new_response("t".to_string());
        r.add_kv_items(vec![], None).unwrap();
        assert_eq!(r.render_for("user".to_string()).unwrap(), r.render_text().unwrap());
    }

    // --- AC-017: with items() populated (not kv_items()) and human_content
    // never called, render_for('user') matches render_for('assistant') and
    // is non-empty. Neither clause was previously asserted at the NAPI layer. ---

    #[test]
    fn ac017_render_for_user_with_items_populated_matches_assistant_and_is_nonempty() {
        let mut r = new_response("issue".to_string());
        r.add_items(
            vec![vec![JsToonValue { str_val: Some("a".to_string()), ..value("str") }]],
            vec!["name".to_string()],
            None,
        )
        .unwrap();
        let user = r.render_for("user".to_string()).unwrap();
        let assistant = r.render_for("assistant".to_string()).unwrap();
        assert_eq!(
            user, assistant,
            "with human_content unset, 'user' must fall back to the same output as 'assistant'"
        );
        assert!(!user.is_empty(), "items() populated with at least one row must yield non-empty output");
    }

    #[test]
    fn render_for_rejects_unknown_audience() {
        let mut r = new_response("t".to_string());
        r.add_kv_items(vec![], None).unwrap();
        let err = r.render_for("nonsense".to_string()).expect_err("should reject");
        assert!(err.reason.contains("nonsense"), "got: {}", err.reason);
    }

    #[test]
    fn has_human_content_reflects_whether_it_was_set() {
        let mut r = new_response("t".to_string());
        r.add_kv_items(vec![], None).unwrap();
        assert!(!r.has_human_content().unwrap());
        r.human_content("hi".to_string()).unwrap();
        assert!(r.has_human_content().unwrap());
    }

    #[test]
    fn js_agent_response_render_json_reflects_is_error() {
        let mut r = new_response("t".to_string());
        r.add_kv_items(vec![], None).unwrap();
        r.as_error().unwrap();
        assert!(r.render_json().unwrap().contains("\"isError\":true"));
    }

    #[test]
    fn js_agent_response_hint_rejects_after_cumulative_limit_exceeded() {
        let mut r = new_response("t".to_string());
        r.add_kv_items(vec![], None).unwrap();
        for i in 0..MAX_HINTS {
            r.hint(format!("hint {i}")).expect("under limit should succeed");
        }
        let result = r.hint("one too many".to_string());
        assert!(result.is_err(), "expected error after exceeding MAX_HINTS cumulative hints");
    }

    #[test]
    fn js_agent_response_recovery_hint_rejects_after_cumulative_limit_exceeded() {
        let mut r = new_response("t".to_string());
        r.add_kv_items(vec![], None).unwrap();
        for _ in 0..MAX_HINTS {
            r.recovery_hint("retry".to_string(), None).expect("under limit should succeed");
        }
        let result = r.recovery_hint("retry".to_string(), None);
        assert!(result.is_err(), "expected error after exceeding MAX_HINTS cumulative recovery hints");
    }

    // --- Cumulative byte budget: the per-call byte cap also bounds what one AgentResponse keeps across calls. ---

    fn big(len: usize) -> String {
        "x".repeat(len)
    }

    #[test]
    fn agent_response_hint_bytes_are_capped_across_calls() {
        let mut r = new_response("t".to_string());
        let mut accepted = 0;
        let err = loop {
            match r.hint(big(MAX_STRING_BYTES)) {
                Ok(()) => accepted += 1,
                Err(e) => break e,
            }
            assert!(accepted <= 16, "17 one-MiB hints were accepted; the budget is per call only");
        };
        // "t" takes one byte, so 15 full hints fit and the 16th would pass 16 MiB.
        assert_eq!(accepted, 15);
        assert!(err.reason.starts_with("cumulative byte total "), "{}", err.reason);
        assert!(err.reason.ends_with(&format!("exceeds maximum of {}", input::MAX_INPUT_BYTES)), "{}", err.reason);
        let rendered = r.render_text().expect("still usable after the rejected hint");
        assert!(rendered.len() < input::MAX_INPUT_BYTES + 4096, "rendered {} bytes", rendered.len());
        r.hint("small".to_string()).expect("a hint that still fits is accepted");
    }

    #[test]
    fn agent_response_recovery_bytes_share_the_budget() {
        let mut r = new_response("t".to_string());
        for _ in 0..10 {
            r.hint(big(MAX_STRING_BYTES)).expect("10 MiB of hints fit");
        }
        let mut accepted = 0;
        let err = loop {
            match r.recovery_hint("retry".to_string(), Some(big(MAX_STRING_BYTES))) {
                Ok(()) => accepted += 1,
                Err(e) => break e,
            }
            assert!(accepted <= 16, "recovery hints are not charged to the response's byte total");
        };
        assert_eq!(accepted, 5, "10 MiB of hints + 5 recovery reasons (+ tool names) leave less than 1 MiB");
        assert!(err.reason.starts_with("cumulative byte total "), "{}", err.reason);
    }

    #[test]
    fn agent_response_human_content_is_capped() {
        let mut r = new_response("t".to_string());
        let err = r.human_content(big(MAX_STRING_BYTES + 1)).expect_err("over 1 MiB");
        assert_eq!(
            err.reason,
            format!("humanContent byte length {} exceeds maximum of {MAX_STRING_BYTES}", MAX_STRING_BYTES + 1)
        );
        assert!(!r.has_human_content().expect("usable"), "the builder is unchanged on error");
        r.human_content(big(MAX_STRING_BYTES)).expect("exactly 1 MiB is accepted");
        r.human_content("short".to_string()).expect("replacing returns the old bytes to the budget");
        assert_eq!(r.render_for("user".to_string()).expect("renders"), "short");
    }

    #[test]
    fn agent_response_sections_add_their_bytes_and_replaced_slots_return_theirs() {
        let cell = |len: usize| JsToonValue { str_val: Some(big(len)), ..value("str") };
        let rows = |n: usize| (0..n).map(|_| vec![cell(MAX_STRING_BYTES)]).collect::<Vec<_>>();
        let mut r = new_response("t".to_string());
        r.add_items(rows(10), vec!["v".to_string()], None).expect("10 MiB of cells fit");
        let err = r
            .add_items(rows(10), vec!["v".to_string()], None)
            .expect_err("every items call adds a section, so its bytes add to the first call's");
        assert!(err.reason.starts_with("cumulative byte total "), "{}", err.reason);
        r.add_items(rows(4), vec!["v".to_string()], None)
            .expect("10 + 4 MiB still fit: the rejected call changed nothing");
        let kv = |n: usize| {
            (0..n).map(|i| JsKvItem { key: format!("k{i}"), value: cell(MAX_STRING_BYTES) }).collect::<Vec<_>>()
        };
        let err = r.add_kv_items(kv(3), None).expect_err("14 MiB of rows + 3 MiB of KV values pass 16 MiB");
        assert!(err.reason.starts_with("cumulative byte total "), "{}", err.reason);
        r.add_kv_items(kv(1), None).expect("1 more MiB fits");
        // A replaced slot still gives its old bytes back.
        r.truncation_hint("a".repeat(2000)).expect("fits");
        r.truncation_hint("b".to_string()).expect("replacing returns the old bytes to the budget");
    }

    #[test]
    fn agent_response_empty_sections_are_charged_for_their_name_and_overhead() {
        // Each empty `items` / `kvItems` call still stores a section with a copy of the type name, so it costs bytes.
        for kv in [false, true] {
            let mut r = new_response("t".repeat(MAX_NAME_BYTES));
            let mut accepted = 0usize;
            while accepted <= 1_000_000 {
                let added = if kv { r.add_kv_items(vec![], None) } else { r.add_items(vec![], vec![], None) };
                if added.is_err() {
                    break;
                }
                accepted += 1;
            }
            // 16 MiB / (4 KiB name + overhead) is about 4k sections; uncharged, the loop would run past a million.
            assert!(accepted < 5_000, "{accepted} empty sections accepted (kv: {kv})");
            assert!(accepted > 2_500, "{accepted} empty sections accepted (kv: {kv}): overhead too large");
        }
    }

    #[test]
    fn agent_response_rows_and_non_string_cells_are_charged_for_the_storage_they_keep() {
        // `Array(100000).fill([])`: no strings at all, yet every call keeps 100,000 row vectors. Charging string
        // bytes only let a loop of these retain gigabytes, since each call now appends a section instead of
        // replacing the rows.
        let empty_rows = || vec![Vec::<JsToonValue>::new(); MAX_ROWS];
        let mut r = new_response("t".to_string());
        let mut accepted = 0usize;
        while accepted < 1_000 {
            if r.add_items(empty_rows(), vec![], None).is_err() {
                break;
            }
            accepted += 1;
        }
        // 16 MiB / (100,000 rows * 24 bytes) is about 7 calls. Uncharged, the loop would run to its cap.
        assert!(accepted < 100, "{accepted} calls of {MAX_ROWS} empty rows accepted");
        assert!(accepted > 3, "{accepted} calls accepted: row charge is too large");

        // A cell that is not a string also keeps a `Value`, so a number-only table is not free either.
        let null_cell = || JsToonValue { r#type: "null".to_string(), ..JsToonValue::default() };
        let mut r = new_response("t".to_string());
        assert!(
            r.add_items(vec![vec![null_cell()]], vec!["a".to_string()], None).is_ok(),
            "a null cell is a valid cell"
        );
        let mut accepted = 0usize;
        while accepted < 100_000 {
            if r.add_items(vec![vec![null_cell(); MAX_FIELDS]; 300], vec![], None).is_err() {
                break;
            }
            accepted += 1;
        }
        // 300,000 cells * 24 bytes = 7.2 MB a call, so two fit in 16 MiB; uncharged, the loop would run to its cap.
        assert!((1..10).contains(&accepted), "{accepted} calls of 300,000 non-string cells accepted");
    }

    // --- AC-026: hint() and recovery_hint() are independently capped;
    // exhausting hint_count must not affect recovery_count. ---

    #[test]
    fn ac026_hint_and_recovery_hint_are_independently_capped() {
        let mut r = new_response("t".to_string());
        r.add_kv_items(vec![], None).unwrap();
        for i in 0..MAX_HINTS {
            r.hint(format!("h{i}")).expect("hint calls under MAX_HINTS must all succeed");
        }
        // hint_count is now exhausted; recovery_count must be a separate counter,
        // still at 0, so this call must succeed rather than failing on account
        // of hint()'s exhausted cap.
        r.recovery_hint("retry".to_string(), None)
            .expect("recovery_hint must succeed after hint()'s cap is exhausted — the two counters are independent");
    }

    #[test]
    fn js_agent_response_to_call_tool_result_basic() {
        let mut r = new_response("issue".to_string());
        r.add_kv_items(
            vec![JsKvItem {
                key: "id".to_string(),
                value: JsToonValue { int_val: Some(JsInt::try_from(1.0).expect("1 is a valid int")), ..value("int") },
            }],
            None,
        )
        .unwrap();
        let result = r.to_call_tool_result().unwrap();
        assert_eq!(result.content.len(), 1);
        assert_eq!(result.content[0].content_type, "text");
        assert_eq!(result.content[0].annotations.audience, vec!["assistant".to_string()]);
        assert!(!result.is_error);
    }

    #[test]
    fn js_agent_response_to_call_tool_result_reflects_is_error() {
        let mut r = new_response("t".to_string());
        r.add_kv_items(vec![], None).unwrap();
        r.as_error().unwrap();
        let result = r.to_call_tool_result().unwrap();
        assert!(result.is_error);
    }

    #[test]
    fn js_agent_response_to_call_tool_result_structured_content_is_parsed_json() {
        let mut r = new_response("t".to_string());
        r.add_kv_items(vec![], None).unwrap();
        let result = r.to_call_tool_result().unwrap();
        assert!(result.structured_content.get("isError").is_some(), "got: {:?}", result.structured_content);
    }

    #[test]
    fn js_agent_response_to_call_tool_result_includes_user_block_with_correct_annotations() {
        let mut r = new_response("t".to_string());
        r.add_kv_items(vec![], None).unwrap();
        r.human_content("friendly summary".to_string()).unwrap();
        let result = r.to_call_tool_result().unwrap();
        assert_eq!(result.content.len(), 2);
        assert_eq!(result.content[1].content_type, "text");
        assert_eq!(result.content[1].annotations.audience, vec!["user".to_string()]);
    }

    #[test]
    fn js_agent_response_human_content_adds_user_audience_block() {
        let mut r = new_response("t".to_string());
        r.add_kv_items(vec![], None).unwrap();
        r.human_content("friendly summary".to_string()).unwrap();
        let result = r.to_call_tool_result().unwrap();
        assert_eq!(result.content.len(), 2);
        assert_eq!(result.content[1].text, "friendly summary");
        assert_eq!(result.content[1].annotations.audience, vec!["user".to_string()]);
    }

    // --- AC-044: render_json() on the spec's example builder state produces
    // real, parseable, distinguishable JSON. ---

    #[test]
    fn ac044_render_json_is_valid_json_distinct_from_render_text() {
        let mut r = new_response("issue".to_string());
        r.add_items(
            vec![vec![JsToonValue { str_val: Some("a".to_string()), ..value("str") }]],
            vec!["name".to_string()],
            None,
        )
        .unwrap();
        r.hint("h".to_string()).unwrap();

        let json = r.render_json().expect("render_json must succeed");
        assert!(json.starts_with('{'), "got: {json}");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("render_json output must parse as JSON");
        assert!(parsed.is_object(), "expected a JSON object, got: {parsed:?}");

        let toon = r.render_text().expect("render_toon must succeed");
        assert_ne!(json, toon, "render_json output must not be byte-identical to render_toon output");
    }

    // --- AC-050: a recovery_hint added via the builder appears in
    // render_text() output, and presence vs. absence of a reason changes the
    // rendered output. ---

    #[test]
    fn ac050_builder_recovery_hint_appears_in_render_toon_output() {
        let mut r_no_reason = new_response("issue".to_string());
        r_no_reason
            .add_items(
                vec![vec![JsToonValue { str_val: Some("a".to_string()), ..value("str") }]],
                vec!["name".to_string()],
                None,
            )
            .unwrap();
        r_no_reason.recovery_hint("retry".to_string(), None).unwrap();
        let out_no_reason = r_no_reason.render_text().unwrap();
        assert!(out_no_reason.contains("retry"), "got: {out_no_reason}");

        let mut r_with_reason = new_response("issue".to_string());
        r_with_reason
            .add_items(
                vec![vec![JsToonValue { str_val: Some("a".to_string()), ..value("str") }]],
                vec!["name".to_string()],
                None,
            )
            .unwrap();
        r_with_reason.recovery_hint("retry".to_string(), Some("r".to_string())).unwrap();
        let out_with_reason = r_with_reason.render_text().unwrap();
        assert!(out_with_reason.contains("retry"), "got: {out_with_reason}");
        assert!(out_with_reason.contains('r'), "got: {out_with_reason}");

        assert_ne!(
            out_no_reason, out_with_reason,
            "presence of a reason must produce different render_text() output than its absence"
        );
    }

    // --- AC-040: to_call_tool_result() succeeds for the fresh-builder and
    // items()-populated states (kv_items-populated and as_error states are
    // already covered by existing tests above). ---

    #[test]
    fn ac040_to_call_tool_result_ok_for_fresh_and_items_populated_states() {
        let r_fresh = new_response("issue".to_string());
        let result_fresh = r_fresh.to_call_tool_result().expect("fresh builder must produce Ok");
        assert!(result_fresh.structured_content.is_object(), "got: {:?}", result_fresh.structured_content);

        let mut r_items = new_response("issue".to_string());
        r_items
            .add_items(
                vec![vec![JsToonValue { str_val: Some("a".to_string()), ..value("str") }]],
                vec!["name".to_string()],
                None,
            )
            .unwrap();
        let result_items = r_items.to_call_tool_result().expect("items-populated builder must produce Ok");
        assert!(result_items.structured_content.is_object(), "got: {:?}", result_items.structured_content);
    }

    // --- Outcomes compose into the builder ---

    fn count(n: u32) -> JsCount {
        JsCount::try_from(f64::from(n)).expect("a valid count")
    }

    #[test]
    fn every_outcome_composes_into_one_response() {
        let mut r = new_response("batch");
        r.set_error("timeout", "POST /charge timed out".into(), vec!["socket closed".into()], Some(true))
            .expect("valid");
        let completed = vec![JsCompletedOp { index: count(0), operation: "symbols".into() }];
        let failed = vec![JsFailedOp {
            index: count(1),
            operation: "refs".into(),
            reason: "not indexed".into(),
            recovery: Some(super::super::hints::JsRecoveryHint { tool: "index".into(), reason: None }),
        }];
        r.add_partial_success("ops".into(), completed, failed, vec![]).expect("valid");
        let step = JsPipelineStep { id: "a".into(), name: "A".into(), status: "completed".into(), reason: None };
        r.add_pipeline("step".into(), vec![step]).expect("valid");
        r.add_status("tool-x".into(), "does x".into(), vec![]).expect("valid");
        r.add_already_done("delete".into(), "done".into()).expect("valid");
        assert_eq!(
            r.render_text().expect("renders"),
            "error: timeout\nmessage: POST /charge timed out\ncauses[1]: socket closed\nexit_code: 1\n\
             ops[2]{index,operation,status,reason}:\n  0,symbols,completed,null\n  1,refs,failed,not indexed\n\
             step[1]{id,name,status,reason}:\n  a,A,completed,null\n\
             tool: tool-x\ndescription: does x\nitems: []\ntotalCount: 0\n\
             operation: delete\nstatus: already_done\nsummary: done\n\
             recovery[1]{tool,params,reason}:\n  index,\"{}\",null"
        );
        let json: serde_json::Value = serde_json::from_str(&r.render_json().expect("renders")).expect("JSON");
        assert_eq!(
            json["error"],
            serde_json::json!({
                "code": "timeout",
                "message": "POST /charge timed out",
                "causes": ["socket closed"],
                "retryable": true
            })
        );
        let tool = r.to_call_tool_result().expect("builds");
        assert!(tool.is_error);
        assert_eq!(tool.structured_content, json);
    }

    #[test]
    fn outcome_inputs_are_checked_and_leave_the_builder_unchanged() {
        let mut r = new_response("t");
        let before = r.render_text().expect("renders");
        let err = r.set_error("bogus", "m".into(), vec![], None).expect_err("unknown code");
        assert!(err.reason.starts_with("unknown error code \"bogus\""), "{}", err.reason);
        let twice = vec![JsCompletedOp { index: count(0), operation: "a".into() }];
        let failed = vec![JsFailedOp { index: count(0), operation: "a".into(), reason: "x".into(), recovery: None }];
        let err = r.add_partial_success("ops".into(), twice, failed, vec![]).expect_err("index 0 twice");
        assert_eq!(err.reason, "operation index 0 has more than one outcome");
        let bad = JsPipelineStep { id: "a".into(), name: "A".into(), status: "skipped".into(), reason: None };
        r.add_pipeline("step".into(), vec![bad]).expect_err("skipped needs a reason");
        assert_eq!(r.render_text().expect("renders"), before);
        assert_eq!(r.held.total(), "t".len(), "nothing was charged");
    }

    #[test]
    fn a_second_error_gives_back_the_first_errors_bytes() {
        let mut r = new_response("t");
        r.set_error("not_found", "x".repeat(1000), vec![], None).expect("fits");
        let first = r.held.total();
        r.set_error("not_found", "x".repeat(1000), vec![], None).expect("fits");
        assert_eq!(r.held.total(), first, "replacing the error does not add up");
        r.add_already_done("op".into(), "s".into()).expect("fits");
        assert!(r.held.total() > first, "an outcome section adds");
    }

    // The TypeScript type of `error`'s `code` lists every ErrorCode label (napi-derive cannot read it from the enum).
    #[test]
    fn error_code_union_names_every_error_code() {
        let src = include_str!("response.rs");
        let start = src.find("pub fn error(").expect("the method is declared");
        let declaration = &src[src[..start].rfind("#[napi(").expect("attribute")..start];
        for code in crate::ErrorCode::ALL {
            assert!(declaration.contains(&format!("'{}'", code.label())), "missing {} in {declaration}", code.label());
        }
    }

    // --- Output bound: a render is at most MAX_OUTPUT_FACTOR times the string bytes held, for every ASCII byte. ---

    /// The three outputs a response can produce, as the byte counts JS receives: the text, the JSON string, and
    /// `toCallToolResult` (its text plus its parsed JSON, re-serialized).
    fn output_sizes(r: &JsAgentResponse) -> [usize; 3] {
        let tool = r.to_call_tool_result().expect("builds");
        [
            r.render_text().expect("renders").len(),
            r.render_json().expect("renders").len(),
            tool.content.iter().map(|c| c.text.len()).sum::<usize>() + tool.structured_content.to_string().len(),
        ]
    }

    #[test]
    fn rendered_output_is_at_most_the_documented_factor_of_the_bytes_held() {
        let mut worst_tenths = 0;
        for byte in 0u8..0x80 {
            let text = char::from(byte).to_string().repeat(1024);
            let kv = |name: &str| {
                let mut r = new_response(name);
                let value = JsToonValue { str_val: Some(text.clone()), ..value("str") };
                r.add_kv_items(vec![JsKvItem { key: "k".into(), value }], None).expect("fits");
                r
            };
            let table = |name: &str| {
                let mut r = new_response(name);
                let cell = JsToonValue { str_val: Some(text.clone()), ..value("str") };
                r.add_items(vec![vec![cell]], vec!["f".into()], None).expect("fits");
                r
            };
            let outcomes = || {
                let mut r = new_response("t");
                r.set_error("not_found", text.clone(), vec![text.clone()], None).expect("fits");
                let failed =
                    JsFailedOp { index: count(0), operation: text.clone(), reason: text.clone(), recovery: None };
                r.add_partial_success("ops".into(), vec![], vec![failed], vec![]).expect("fits");
                let step =
                    JsPipelineStep { id: text.clone(), name: text.clone(), status: "completed".into(), reason: None };
                r.add_pipeline("step".into(), vec![step]).expect("fits");
                let item = JsStatusItem {
                    key: "k".into(),
                    value: JsToonValue { str_val: Some(text.clone()), ..value("str") },
                    health: None,
                };
                r.add_status(text.clone(), text.clone(), vec![item]).expect("fits");
                r.add_already_done(text.clone(), text.clone()).expect("fits");
                r
            };
            for r in [kv("t"), table("t"), outcomes()] {
                let held = r.held.total();
                for out in output_sizes(&r) {
                    assert!(
                        out <= input::MAX_OUTPUT_FACTOR * held,
                        "byte {byte:#04x}: {out} output bytes for {held} held (limit {}x)",
                        input::MAX_OUTPUT_FACTOR
                    );
                    worst_tenths = worst_tenths.max(out * 10 / held);
                }
            }
        }
        // The factor is tight for control characters, not just an upper bound nobody reaches.
        assert!(
            worst_tenths > 150,
            "worst ratio {worst_tenths}/10: the documented factor of {} is loose",
            input::MAX_OUTPUT_FACTOR
        );
    }
}

#[cfg(test)]
mod output_bound;
