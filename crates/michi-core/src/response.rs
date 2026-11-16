use crate::budget::BudgetSummary;
use crate::hints::Hint;
use crate::json;
use crate::kv::KvItem;
use crate::provenance::ProvenanceBlock;
use crate::recovery::RecoveryHint;
use crate::section::{Section, SectionBody};
use crate::toon::Value;

/// The serialisation format for `AgentResponse::render`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum OutputFormat {
    /// Plain-text TOON / kv format. Default.
    #[default]
    Text,
    /// Compact JSON object.
    Json,
}

/// Default per-cell truncation limit used by [`AgentResponse::new`].
pub const DEFAULT_TRUNCATE_CELLS: usize = 200;

/// Builder for an agent-facing response: an ordered list of [`Section`]s, an optional budget report and provenance
/// block, hints and recovery hints.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct AgentResponse {
    /// The type name given to [`new`](Self::new): it names the sections that [`items`](Self::items) and
    /// [`kv_items`](Self::kv_items) append, and the empty list an untouched response renders.
    name: String,
    /// Per-cell cap for every `Toon` section, set with [`truncate_cells_at`](Self::truncate_cells_at).
    max_cell_len: usize,
    /// The caller's instruction for getting a cut cell in full, set with [`truncation_hint`](Self::truncation_hint).
    truncation_hint: Option<String>,
    /// Every block, in call order. The one place a response keeps its body.
    sections: Vec<Section>,
    /// Response-level budget report, set with `budget()`.
    budget: Option<BudgetSummary>,
    /// Response-level provenance block, set with `provenance()`.
    provenance: Option<ProvenanceBlock>,
    hints: Vec<Hint>,
    recovery: Vec<RecoveryHint>,
    is_error: bool,
    human_content: Option<String>,
}

impl AgentResponse {
    /// Create a new, empty response for the given type name. The name labels the sections that
    /// [`items`](Self::items) and [`kv_items`](Self::kv_items) add. A response that never gets a section, budget or
    /// provenance renders `name: []`, the definitive empty list.
    pub fn new(type_name: impl Into<String>) -> Self {
        Self {
            name: type_name.into(),
            max_cell_len: DEFAULT_TRUNCATE_CELLS,
            truncation_hint: None,
            sections: Vec::new(),
            budget: None,
            provenance: None,
            hints: Vec::new(),
            recovery: Vec::new(),
            is_error: false,
            human_content: None,
        }
    }

    /// Append a TOON list section named for the response's type: shorthand for
    /// `section(Section::toon(name, fields, rows))`. Every call adds a section, rendered in call order.
    ///
    /// Sections are root keys, and this shorthand gives each the same name, so for one list per response only: a
    /// second `items()` call repeats the key `name`, and the response renders the `toon_validation_failed` error
    /// naming it (and `is_error()` is true) instead of invalid TOON. Mixing with [`kv_items`](Self::kv_items) works
    /// while no KV key equals the type name. For several lists, give each its own name with
    /// `.section(Section::toon(distinct_name, ..))`. To report a total, build the section yourself and set
    /// [`Section::total_count`].
    #[must_use]
    pub fn items(self, rows: Vec<Vec<Value>>, fields: &[&str]) -> Self {
        let name = self.name.clone();
        self.section(Section::toon(name, fields.iter().map(|s| (*s).to_string()).collect(), rows))
    }

    /// Append a TOON list section for a cut list, in one call. The rows are `outcome.kept`, each turned into a row by
    /// `row`; the section's total is `outcome.total_before`; and, only when something was cut,
    /// [`TruncationOutcome::hint`](crate::truncation::TruncationOutcome::hint) (`showing {kept} of {total}; {guidance}`)
    /// is appended to the hints. michi builds the rows from the outcome itself, so the list's `[N]`, its `totalCount`
    /// and the `help[]` line all describe the same cut.
    ///
    /// `totalCount` renders only when `total_before` exceeds the rows kept.
    ///
    /// ```
    /// use michi_core::toon::Value;
    /// use michi_core::{truncate_by_count, AgentResponse, OutputFormat};
    ///
    /// let outcome = truncate_by_count(vec![1, 2, 3, 4], 2);
    /// let r = AgentResponse::new("issues").truncated_section(
    ///     "issues",
    ///     vec!["n".into()],
    ///     &outcome,
    ///     |n| vec![Value::Int(*n)],
    ///     "pass --limit 4",
    /// );
    /// assert_eq!(
    ///     r.render(OutputFormat::Text),
    ///     "issues[2]{n}:\n  1\n  2\ntotalCount: 4\nhelp[1]: showing 2 of 4; pass --limit 4"
    /// );
    /// ```
    #[must_use]
    pub fn truncated_section<T>(
        mut self,
        name: impl Into<String>,
        fields: Vec<String>,
        outcome: &crate::truncation::TruncationOutcome<T>,
        row: impl FnMut(&T) -> Vec<Value>,
        guidance: impl Into<String>,
    ) -> Self {
        let rows = outcome.kept.iter().map(row).collect();
        if let Some(hint) = outcome.hint(guidance) {
            self.hints.push(hint);
        }
        self.section(Section::toon(name, fields, rows).total_count(Some(outcome.total_before)))
    }

    /// Append a KV section named for the response's type: shorthand for `section(Section::kv(name, items))`. The name
    /// is not printed for a KV body. Every call adds a section, but the item keys are root keys: two calls sharing a
    /// key render the `toon_validation_failed` error naming it. See [`items`](Self::items); for several KV sections
    /// use `.section(Section::kv(name, ..))` and keep the keys distinct.
    #[must_use]
    pub fn kv_items(self, items: Vec<KvItem>) -> Self {
        let name = self.name.clone();
        self.section(Section::kv(name, items))
    }

    /// Append a section. Sections render in call order, the one place a response keeps its body: the caller owns
    /// ordering.
    ///
    /// The response's cell cap ([`truncate_cells_at`](Self::truncate_cells_at)) and truncation hint apply to every
    /// `Toon` body. A section's `totalCount` comes from [`Section::total_count`]. Each section is a root key of the
    /// document, so names must be distinct (see [`Section`]).
    #[must_use]
    pub fn section(mut self, section: Section) -> Self {
        self.sections.push(section);
        self
    }

    /// Attach the response-level budget report; one per response, a second call replaces the first. It renders as a
    /// `budget:` block after the last section (see [`BudgetSummary`] for what it carries), and the per-section
    /// roll-up appears only when the response has two or more sections. michi renders the report; it never computes
    /// it.
    ///
    /// A response with a budget or provenance but no section has no body of its own: the empty `type: []` line that
    /// an untouched response renders is left out.
    #[must_use]
    pub fn budget(mut self, budget: BudgetSummary) -> Self {
        self.budget = Some(budget);
        self
    }

    /// Attach the response-level provenance block; one per response, a second call replaces the first. It renders as
    /// a `provenance:` block after `budget:` (see [`ProvenanceBlock`]).
    #[must_use]
    pub fn provenance(mut self, provenance: ProvenanceBlock) -> Self {
        self.provenance = Some(provenance);
        self
    }

    /// Append a contextual hint.
    #[must_use]
    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hints.push(Hint::new(hint));
        self
    }

    /// Replace all contextual hints.
    #[must_use]
    pub fn hints(mut self, hints: Vec<Hint>) -> Self {
        self.hints = hints;
        self
    }

    /// Append a recovery hint.
    #[must_use]
    pub fn recovery_hint(mut self, r: RecoveryHint) -> Self {
        self.recovery.push(r);
        self
    }

    /// Set the max string cell length, in chars, before truncation on the TOON path. Default:
    /// [`DEFAULT_TRUNCATE_CELLS`].
    ///
    /// The limit counts characters before TOON escaping and quoting, so a cell that needs quotes or escapes is
    /// written longer than `limit` (see [`crate::toon::ToonOptions::max_cell_len`]). The KV path is never truncated.
    ///
    /// The cap shapes the text render only. The JSON `sections` (and so `structuredContent`) carry every cell in full:
    /// they are the canonical data, and the text is the token-saving view of it.
    #[must_use]
    pub fn truncate_cells_at(mut self, limit: usize) -> Self {
        self.max_cell_len = limit;
        self
    }

    /// Set the caller's instruction for getting a cut TOON cell in full. When a cell was cut, michi adds one line to
    /// `help[]` (after your own hints) that names the `truncated` block and this instruction (see
    /// [`crate::toon::ToonOptions::truncation_hint`]). Unset by default: michi never invents a parameter the tool
    /// may not have, and the `truncated` block still reports each cut. Pass tool-author text, never data.
    #[must_use]
    pub fn truncation_hint(mut self, hint: impl Into<String>) -> Self {
        self.truncation_hint = Some(hint.into());
        self
    }

    /// Mark this response as an error state.
    #[must_use]
    pub fn as_error(mut self) -> Self {
        self.is_error = true;
        self
    }

    /// Attach a human-facing companion block (`audience: user`).
    #[must_use]
    pub fn human_content(mut self, text: impl Into<String>) -> Self {
        self.human_content = Some(text.into());
        self
    }

    /// The caller's hints, then michi's own truncation line when there is one: a single `help` block.
    fn hints_with<'a>(&'a self, extra: &[Hint]) -> std::borrow::Cow<'a, [Hint]> {
        if extra.is_empty() {
            std::borrow::Cow::Borrowed(&self.hints)
        } else {
            let mut all = Vec::with_capacity(self.hints.len() + extra.len());
            all.extend(self.hints.iter().cloned());
            all.extend(extra.iter().cloned());
            std::borrow::Cow::Owned(all)
        }
    }

    /// The options a `Toon` section renders by: the response's cell cap and truncation hint, the section's own
    /// `totalCount` rule. Copies the rows: `ToonOptions` owns them.
    fn section_options(
        &self,
        name: &str,
        fields: &[String],
        rows: &[Vec<Value>],
        shown_total: Option<usize>,
    ) -> crate::toon::ToonOptions {
        let mut opts = crate::toon::ToonOptions::new(name, fields.to_vec(), rows.to_vec());
        opts.total_count = shown_total;
        opts.max_cell_len = self.max_cell_len;
        opts.truncation_hint.clone_from(&self.truncation_hint);
        opts
    }

    /// The blocks to render: the sections, or, for a response with nothing at all, the one empty list that names
    /// its type. A budget or provenance block stands alone, with no empty list before it.
    fn blocks(&self) -> std::borrow::Cow<'_, [Section]> {
        if self.sections.is_empty() && self.budget.is_none() && self.provenance.is_none() {
            std::borrow::Cow::Owned(vec![Section::toon(self.name.as_str(), Vec::new(), Vec::new())])
        } else {
            std::borrow::Cow::Borrowed(&self.sections)
        }
    }

    /// One section as an envelope part.
    fn section_part<'a>(&self, s: &'a Section) -> (Part<'a>, bool, Option<Hint>) {
        let total = s.shown_total();
        let name = s.name();
        match s.body() {
            SectionBody::Toon { fields, rows } => match self.section_options(name, fields, rows, total).validate() {
                Ok(doc) => {
                    let list = doc.render_list();
                    let cuts = list
                        .cuts
                        .iter()
                        .map(|&(row, field, removed)| crate::toon::table::Cut {
                            section: name,
                            row,
                            field: fields.get(field).map_or("", String::as_str),
                            removed,
                        })
                        .collect();
                    (Part { name, text: list.text, cuts, total }, false, list.extra_hint.map(Hint::new))
                }
                Err(e) => (Part::failed(name, "toon_validation_failed", &e.to_string()), true, None),
            },
            SectionBody::Kv(items) => kv_part(name, items, total),
            // One string value under the section's name, escaped like any other value: prose cannot start a line
            // of its own, so it cannot forge a `help[]` line or break the document.
            SectionBody::Text { text, .. } => {
                let line = format!("{}: {}", crate::toon::escape_key(name), crate::toon::escape_value(text));
                (Part { name, text: line, cuts: Vec::new(), total }, false, None)
            }
        }
    }

    /// Whether each block fails validation, without rendering it.
    fn failed_flags(&self) -> Vec<bool> {
        self.blocks()
            .iter()
            .map(|s| match s.body() {
                SectionBody::Toon { fields, rows } => {
                    self.section_options(s.name(), fields, rows, s.shown_total()).validate().is_err()
                }
                SectionBody::Kv(items) => crate::kv::validate_keys(items).is_err(),
                SectionBody::Text { .. } => false,
            })
            .collect()
    }

    /// The first key the body cannot render, or `None`. Sections render as root keys (a TOON list or text section
    /// under its name, a KV section as its item keys), beside the keys michi writes itself
    /// ([`crate::toon::RESERVED_ROOT_KEYS`]: the envelope blocks, the keys that follow a body, and the error
    /// document's keys). The reserved names are claimed whether or not michi writes that block here, so validity
    /// never depends on the caller's data, and a success response cannot carry `error:` or `message:` lines.
    /// `failed[i]` says whether block `i` failed validation: its `error` and `message` lines are michi's own, so a
    /// second failed block is a collision on `error`. Provenance labels sit one level down (see
    /// [`crate::provenance::label_collision`]).
    fn root_collision(&self, failed: &[bool]) -> Option<String> {
        let blocks = self.blocks();
        let mut seen: std::collections::HashSet<&str> = crate::toon::RESERVED_ROOT_KEYS.into_iter().collect();
        let mut error_block = false;
        for (s, failed) in blocks.iter().zip(failed) {
            if *failed {
                if error_block {
                    return Some("error".to_string());
                }
                error_block = true;
                continue;
            }
            let keys: Vec<&str> = match s.body() {
                SectionBody::Toon { .. } | SectionBody::Text { .. } => vec![s.name()],
                SectionBody::Kv(items) => items.iter().map(|i| i.key.as_str()).collect(),
            };
            for key in keys {
                if !seen.insert(key) {
                    return Some(key.to_string());
                }
            }
        }
        let p = self.provenance.as_ref()?;
        crate::provenance::label_collision(p).map(str::to_string)
    }

    /// The body: sections, `budget:`, `provenance:`.
    ///
    /// Blocks are joined with one LF and empty ones are skipped. `truncated` and `totalCount` are root keys, so they
    /// may appear once: when one block has them they stay inline after its list; when two or more do, each becomes
    /// one table with a leading `section` column, after the last block.
    ///
    /// The flag is whether this response is an error: set by `as_error()` or by a block that failed validation.
    /// Every output surface reads its error flag from here so text, JSON and MCP never disagree.
    fn body(&self) -> (String, bool, Vec<Hint>) {
        let blocks = self.blocks();
        let mut parts: Vec<Part<'_>> = Vec::with_capacity(blocks.len());
        let mut failed = false;
        let mut flags: Vec<bool> = Vec::with_capacity(blocks.len());
        let mut extras: Vec<Hint> = Vec::new();
        for s in blocks.iter() {
            let (part, part_failed, extra) = self.section_part(s);
            failed |= part_failed;
            flags.push(part_failed);
            // One line per cut response is enough: every section's line says the same thing.
            if let Some(h) = extra {
                if !extras.contains(&h) {
                    extras.push(h);
                }
            }
            parts.push(part);
        }
        // Two blocks writing one root key would be invalid TOON. Like any other bad input the whole body becomes the
        // visible validation error, so the caller sees it instead of a document a decoder rejects.
        if let Some(key) = self.root_collision(&flags) {
            return (collision_body(&key), true, Vec::new());
        }
        let cutters = parts.iter().filter(|p| !p.cuts.is_empty()).count();
        let totals = parts.iter().filter(|p| p.total.is_some()).count();
        let mut out = String::with_capacity(parts.iter().map(|p| p.text.len() + 24).sum::<usize>() + 256);
        for p in &parts {
            join_block(&mut out, &p.text);
            if cutters == 1 {
                crate::toon::table::push_truncated(&mut out, &p.cuts, false);
            }
            if let (1, Some(n)) = (totals, p.total) {
                join_block(&mut out, &format!("totalCount: {n}"));
            }
        }
        if cutters >= 2 {
            let cuts: Vec<crate::toon::table::Cut<'_>> = parts.iter().flat_map(|p| p.cuts.iter().cloned()).collect();
            crate::toon::table::push_truncated(&mut out, &cuts, true);
        }
        if totals >= 2 {
            use crate::toon::table::{self, Cell};
            table::push_header(&mut out, "totalCount", totals, ["section", "total"]);
            for p in &parts {
                if let Some(total) = p.total {
                    table::push_row(&mut out, &[Cell::Str(p.name), Cell::count(total)]);
                }
            }
        }
        if let Some(b) = &self.budget {
            join_block(&mut out, &crate::budget::render_budget_block(b, &self.sections));
        }
        if let Some(p) = &self.provenance {
            join_block(&mut out, &crate::provenance::render_provenance_block(p));
        }
        (out, failed || self.is_error, extras)
    }

    /// Compose the text document: body, help block, recovery block. Each part is a TOON 4.4 document without a
    /// trailing newline and the append helpers join them with one LF, so the result is one root object whose keys
    /// are unique (the body cannot use `totalCount`, `help`, `recovery` or `truncated`). Appends to `body` in place.
    fn finish_text(&self, mut body: String, extra: &[Hint]) -> String {
        let hints = self.hints_with(extra);
        if !hints.is_empty() || !self.recovery.is_empty() {
            body.reserve(1 + hints.len() * 60 + self.recovery.len() * 80);
        }
        crate::hints::append_hints(&mut body, &hints);
        crate::recovery::append_recovery(&mut body, &self.recovery);
        body
    }

    /// Render the response in the requested format.
    #[must_use]
    pub fn render(&self, format: OutputFormat) -> String {
        match format {
            OutputFormat::Text => self.render_text(),
            OutputFormat::Json => self.render_json(),
        }
    }

    fn render_text(&self) -> String {
        let (body, _, extra) = self.body();
        self.finish_text(body, &extra)
    }

    /// Whether this response is an error: `as_error()` was called, or a section's TOON rows or KV items failed
    /// validation and its block is a `toon_validation_failed` or `kv_validation_failed` error block. Validates
    /// without rendering.
    #[must_use]
    pub fn is_error(&self) -> bool {
        if self.is_error {
            return true;
        }
        let failed = self.failed_flags();
        failed.contains(&true) || self.root_collision(&failed).is_some()
    }

    /// Render just the `help[N]:` block for `self.hints`.
    #[must_use]
    pub fn render_hints_only(&self) -> String {
        crate::hints::render_hints(&self.hints)
    }

    /// Render for the given audience.
    #[must_use]
    pub fn render_for(&self, audience: crate::audience::Audience) -> String {
        match audience {
            crate::audience::Audience::Assistant => self.render_text(),
            crate::audience::Audience::User => self.human_content.clone().unwrap_or_else(|| self.render_text()),
        }
    }

    /// Whether `.human_content()` was set on this builder.
    #[must_use]
    pub fn has_human_content(&self) -> bool {
        self.human_content.is_some()
    }

    /// Build the MCP `CallToolResult` for this response.
    #[must_use]
    pub fn to_call_tool_result(&self) -> crate::mcp::CallToolResult {
        let (body, is_error, extra) = self.body();
        let structured_content = self.json_from_body(&body, is_error, &extra);
        let mut content = vec![crate::mcp::ContentBlock {
            text: self.finish_text(body, &extra),
            audience: vec![crate::audience::Audience::Assistant],
        }];
        if let Some(human) = &self.human_content {
            content.push(crate::mcp::ContentBlock {
                text: human.clone(),
                audience: vec![crate::audience::Audience::User],
            });
        }
        crate::mcp::CallToolResult::new(
            content,
            is_error,
            Some(crate::mcp::JsonObject::from_trusted(structured_content)),
        )
    }

    fn render_json(&self) -> String {
        let (body, is_error, extra) = self.body();
        self.json_from_body(&body, is_error, &extra)
    }

    fn json_from_body(&self, body: &str, is_error: bool, extra: &[Hint]) -> String {
        let hints = self.hints_with(extra);
        let capacity =
            body.len() + hints.iter().map(|h| h.as_str().len() + 16).sum::<usize>() + self.recovery.len() * 64 + 64;
        let mut out = String::with_capacity(capacity + 128);
        out.push_str("{\"body\":");
        json::push_str(&mut out, body);
        if !self.sections.is_empty() {
            out.push_str(",\"sections\":");
            json::push_array(&mut out, &self.sections, push_section_json);
        }
        if let Some(b) = &self.budget {
            out.push_str(",\"budget\":");
            crate::budget::push_budget_json(&mut out, b, &self.sections);
        }
        if let Some(p) = &self.provenance {
            out.push_str(",\"provenance\":");
            crate::provenance::push_provenance_json(&mut out, p);
        }
        out.push_str(",\"hints\":");
        json::push_str_array(&mut out, hints.iter().map(Hint::as_str));
        out.push_str(",\"recovery\":");
        json::push_array(&mut out, &self.recovery, json::push_recovery);
        out.push_str(",\"isError\":");
        json::push_bool(&mut out, is_error);
        out.push('}');
        out
    }
}

/// The body that replaces rows or items that cannot form a valid document: `error: <code>` and the quoted message.
fn validation_error_body(code: &str, message: &str) -> String {
    let message = crate::toon::escape_value(message);
    let mut out = String::with_capacity(17 + code.len() + message.len());
    out.push_str("error: ");
    out.push_str(code);
    out.push_str("\nmessage: ");
    out.push_str(&message);
    out
}

/// The body for two blocks that would write the same root key.
fn collision_body(key: &str) -> String {
    validation_error_body(
        "toon_validation_failed",
        &format!("duplicate root key '{key}': give every section and KV key a distinct name"),
    )
}

/// One block of an envelope body before the root-level `truncated` and `totalCount` keys are placed.
struct Part<'a> {
    /// The block's name, the `section` column when several blocks share a root key.
    name: &'a str,
    /// The block's text without its `truncated` block or `totalCount` line.
    text: String,
    /// Cut cells, for the `truncated` block.
    cuts: Vec<crate::toon::table::Cut<'a>>,
    /// The `totalCount` this block reports, when it reports one.
    total: Option<usize>,
}

impl<'a> Part<'a> {
    /// A block that failed validation: the error body replaces its content and it reports no count.
    fn failed(name: &'a str, code: &str, message: &str) -> Self {
        Self { name, text: validation_error_body(code, message), cuts: Vec::new(), total: None }
    }
}

/// A KV block as a part. No items still reports a larger total the caller set.
fn kv_part<'a>(name: &'a str, items: &[KvItem], total: Option<usize>) -> (Part<'a>, bool, Option<Hint>) {
    if items.is_empty() {
        return (Part { name, text: String::new(), cuts: Vec::new(), total }, false, None);
    }
    match crate::kv::render_kv(items, None, &[]) {
        Ok(text) => (Part { name, text, cuts: Vec::new(), total }, false, None),
        Err(e) => (Part::failed(name, "kv_validation_failed", &e.to_string()), true, None),
    }
}

/// Append `block` after a LF unless either side is empty.
fn join_block(out: &mut String, block: &str) {
    if block.is_empty() {
        return;
    }
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str(block);
}

/// One section as the JSON object spec 07 section 5 describes. Rows carry the full values: the per-cell cap shapes
/// the text render, and `structuredContent` is the typed copy of the same data. `returnedCount` and `truncated` are
/// the section's own derived facts, the ones the text render uses.
fn push_section_json(out: &mut String, s: &Section) {
    out.push_str("{\"name\":");
    json::push_str(out, s.name());
    match s.body() {
        SectionBody::Toon { fields, rows } => {
            out.push_str(",\"kind\":\"toon\",\"fields\":");
            json::push_str_array(out, fields.iter().map(String::as_str));
            out.push_str(",\"rows\":");
            json::push_array(out, rows, |out, row| json::push_array(out, row, json::push_value));
        }
        SectionBody::Kv(items) => {
            out.push_str(",\"kind\":\"kv\",\"items\":");
            json::push_array(out, items, |out, item| {
                out.push_str("{\"key\":");
                json::push_str(out, &item.key);
                out.push_str(",\"value\":");
                json::push_kv_value(out, &item.value);
                out.push('}');
            });
        }
        SectionBody::Text { text, .. } => {
            out.push_str(",\"kind\":\"text\",\"text\":");
            json::push_str(out, text);
        }
    }
    out.push_str(",\"returnedCount\":");
    json::push_uint(out, s.count() as u64);
    if let Some(total) = s.total() {
        out.push_str(",\"totalCount\":");
        json::push_uint(out, total as u64);
    }
    out.push_str(",\"truncated\":");
    json::push_bool(out, s.truncated());
    out.push('}');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capacity_violations_are_toon_validation_errors_on_every_surface() {
        let fields: Vec<String> = (0..=crate::toon::MAX_FIELDS).map(|i| format!("f{i}")).collect();
        let field_refs: Vec<&str> = fields.iter().map(String::as_str).collect();
        let too_wide = AgentResponse::new("t").items(vec![], &field_refs);
        let long_hint = AgentResponse::new("t")
            .items(vec![vec![Value::from("x")]], &["a"])
            .truncation_hint("h".repeat(crate::toon::MAX_TRUNCATION_HINT_CHARS + 1));
        for (r, message) in [
            (too_wide, "fields length 1001 exceeds maximum of 1000"),
            (long_hint, "truncation_hint length 257 chars exceeds maximum of 256"),
        ] {
            assert!(r.is_error(), "{message}");
            let text = r.render(OutputFormat::Text);
            assert_eq!(text, format!("error: toon_validation_failed\nmessage: {message}"));
            assert!(r.render(OutputFormat::Json).contains("\"isError\":true"), "{message}");
            let result = r.to_call_tool_result();
            assert!(result.is_error, "{message}");
            assert_eq!(result.content[0].text, text);
            assert!(
                result.structured_content.map(String::from).unwrap_or_default().contains("\"isError\":true"),
                "{message}"
            );
        }
    }

    #[test]
    fn is_error_agrees_with_the_rendered_flag_for_every_target() {
        use crate::kv::{KvItem, KvValue};
        let kv = |key: &str| KvItem { key: key.into(), value: KvValue::Int(1) };
        let cases = [
            AgentResponse::new("t"),
            AgentResponse::new("help"),
            AgentResponse::new("t").items(vec![vec![Value::Int(1)]], &["a"]),
            AgentResponse::new("t").items(vec![vec![Value::Int(1)]], &["a", "b"]),
            AgentResponse::new("t").kv_items(vec![]),
            AgentResponse::new("t").kv_items(vec![kv("a")]),
            AgentResponse::new("t").kv_items(vec![kv("a"), kv("a")]),
            AgentResponse::new("t").kv_items(vec![kv("a")]).as_error(),
        ];
        for r in cases {
            assert_eq!(r.is_error(), r.to_call_tool_result().is_error, "{r:?}");
        }
    }

    #[test]
    fn items_path_renders_toon() {
        let r = AgentResponse::new("issues").items(vec![vec![Value::Int(1), Value::from("open")]], &["id", "state"]);
        let out = r.render(OutputFormat::Text);
        assert!(out.starts_with("issues[1]{id,state}:\n  1,open"), "got: {out}");
    }

    // AC-001: Unset target (no .items()/.kv_items() call) routes to TOON
    // rendering with zero rows/fields, not KV or an empty string.
    #[test]
    fn ac001_unset_target_renders_empty_toon_exactly() {
        let r = AgentResponse::new("widget");
        assert_eq!(r.render(OutputFormat::Text), "widget: []");
    }

    // AC-002: the test above (items_path_renders_toon) only checks
    // starts_with, which would still pass if extra bytes followed the row.
    // Pin the exact full output for a builder with no hints/recovery set.
    #[test]
    fn ac002_items_path_renders_exact_toon_literal() {
        let r = AgentResponse::new("issues").items(vec![vec![Value::Int(1), Value::from("open")]], &["id", "state"]);
        assert_eq!(r.render(OutputFormat::Text), "issues[1]{id,state}:\n  1,open");
    }

    // AC-003: render(Text) via kv_items() matches the direct render_kv() call
    // for the same items, not TOON list structure.
    #[test]
    fn ac003_kv_items_path_matches_direct_render_kv_call() {
        use crate::kv::{KvItem, KvValue};
        let items = vec![KvItem { key: "id".into(), value: KvValue::Int(1) }];
        let r = AgentResponse::new("t").kv_items(items.clone());
        // Without hints or recovery the composed text is exactly the KV document.
        assert_eq!(Ok(r.render(OutputFormat::Text)), crate::kv::render_kv(&items, None, &[]));
    }

    #[test]
    fn kv_section_with_no_items_still_reports_a_larger_total() {
        let r = AgentResponse::new("t").section(Section::kv("k", vec![]).total_count(Some(3)));
        assert_eq!(r.render(OutputFormat::Text), "totalCount: 3");
        let json = r.render(OutputFormat::Json);
        assert!(json.starts_with("{\"body\":\"totalCount: 3\","), "{json}");
        let with_hint =
            AgentResponse::new("t").section(Section::kv("k", vec![]).total_count(Some(3))).hint("try another id");
        assert_eq!(with_hint.render(OutputFormat::Text), "totalCount: 3\nhelp[1]: try another id");
        assert_eq!(AgentResponse::new("t").kv_items(vec![]).render(OutputFormat::Text), "", "no count, no body");
    }

    #[test]
    fn render_json_basic_structure() {
        let r = AgentResponse::new("items").items(vec![vec![Value::from("hello world")]], &["name"]);
        let json = r.render_json();
        assert!(json.contains("\"isError\":false"), "got: {json}");
        assert!(json.contains("\"body\""), "got: {json}");
        assert!(json.contains("\"hints\":[]"), "got: {json}");
    }

    #[test]
    fn render_json_is_error_flag() {
        let r = AgentResponse::new("items").items(vec![vec![Value::from("fail")]], &["name"]).as_error();
        let json = r.render_json();
        assert!(json.contains("\"isError\":true"), "got: {json}");
    }

    // AC-004: render(Json) parses as an object with exactly the 4 named top-level keys plus the envelope keys
    // (`sections`, `budget`, `provenance`) that were set, for every construction path. `items()` and `kv_items()`
    // add a section, so they add `sections`; a response with no body at all keeps exactly the four.
    #[test]
    #[cfg(feature = "serde")]
    fn ac004_render_json_top_level_keys_are_the_four_plus_the_envelope_keys_that_were_set() {
        use crate::{BudgetSummary, ProvenanceBlock, Section};
        let kv_item = || crate::kv::KvItem { key: "k".into(), value: crate::kv::KvValue::Int(1) };
        let section = || Section::text("note", "x");
        let base = ["body", "hints", "isError", "recovery"];
        let cases: Vec<(AgentResponse, Vec<&str>)> = vec![
            (AgentResponse::new("t"), vec![]),
            (AgentResponse::new("t").items(vec![vec![Value::from("x")]], &["a"]), vec!["sections"]),
            (AgentResponse::new("t").kv_items(vec![kv_item()]), vec!["sections"]),
            (AgentResponse::new("t").section(section()), vec!["sections"]),
            (AgentResponse::new("t").budget(BudgetSummary::new()), vec!["budget"]),
            (AgentResponse::new("t").provenance(ProvenanceBlock::new()), vec!["provenance"]),
            (
                AgentResponse::new("t")
                    .kv_items(vec![kv_item()])
                    .section(section())
                    .budget(BudgetSummary::new())
                    .provenance(ProvenanceBlock::new()),
                vec!["budget", "provenance", "sections"],
            ),
        ];
        for (r, extra) in cases {
            let mut want: Vec<&str> = base.iter().copied().chain(extra).collect();
            want.sort_unstable();
            for json in [
                r.render(OutputFormat::Json),
                r.to_call_tool_result().structured_content.map(String::from).unwrap_or_default(),
            ] {
                let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
                let obj = parsed.as_object().expect("top-level object");
                let mut keys: Vec<&str> = obj.keys().map(String::as_str).collect();
                keys.sort_unstable();
                assert_eq!(keys, want, "{json}");
            }
        }
    }

    // AC-006: recovery array entries carry tool/params always, and reason
    // only when Some — key absent (not null) when None.
    #[test]
    #[cfg(feature = "serde")]
    fn ac006_recovery_json_entry_shape_and_reason_presence() {
        use crate::kv::KvValue;
        use crate::recovery::RecoveryHint;
        let with_reason = RecoveryHint::new("retry").param("n", KvValue::Int(1)).reason("why");
        let without_reason = RecoveryHint::new("retry2");
        let r = AgentResponse::new("t").recovery_hint(with_reason).recovery_hint(without_reason);
        let parsed: serde_json::Value = serde_json::from_str(&r.render_json()).expect("valid JSON");
        let recovery = parsed["recovery"].as_array().expect("recovery array");
        assert_eq!(recovery.len(), 2);
        assert_eq!(recovery[0]["tool"], serde_json::json!("retry"));
        assert_eq!(recovery[0]["params"], serde_json::json!({"n": 1}));
        assert_eq!(recovery[0]["reason"], serde_json::json!("why"));
        assert_eq!(recovery[1]["tool"], serde_json::json!("retry2"));
        assert!(recovery[1].as_object().unwrap().get("reason").is_none(), "got: {recovery:?}");
    }

    #[test]
    fn render_json_recovery_int_param_is_numeric_not_string() {
        use crate::kv::KvValue;
        use crate::recovery::RecoveryHint;
        let recovery = RecoveryHint::new("retry").param("limit", KvValue::Int(10));
        let r =
            AgentResponse::new("items").items(vec![vec![Value::from("hit limit")]], &["name"]).recovery_hint(recovery);
        let json = r.render_json();
        assert!(json.contains("\"limit\":10"), "Int param must be unquoted, got: {json}");
        assert!(!json.contains("\"limit\":\"10\""), "Int param must not be quoted string, got: {json}");
    }

    #[test]
    fn render_json_hint_with_quotes_is_escaped() {
        let r = AgentResponse::new("items").hint(r#"Use "get_item" instead"#);
        let json = r.render_json();
        assert!(json.contains("\\\"get_item\\\""), "quotes in hints must be escaped, got: {json}");
    }

    // AC-038: the test above only exercises the quote-escaping half; the
    // embedded-newline half and the body-value escaping half were untested.
    #[test]
    #[cfg(feature = "serde")]
    fn ac038_hint_and_body_with_quote_backslash_newline_produce_valid_json() {
        let r = AgentResponse::new("items").hint("has \"quotes\" and \\backslash\nand newline").kv_items(vec![
            crate::kv::KvItem { key: "k".into(), value: crate::kv::KvValue::Text("body \"quoted\" \\slash".into()) },
        ]);
        let raw = r.render_json();
        let parsed: serde_json::Value = serde_json::from_str(&raw).expect("must be valid JSON");
        assert_eq!(parsed["hints"][0].as_str().unwrap(), "has \"quotes\" and \\backslash\nand newline");
        // The body is TOON text: the value is quoted and escaped per §7.1/§7.2, then JSON-escaped as a whole.
        assert_eq!(parsed["body"].as_str().unwrap(), "k: \"body \\\"quoted\\\" \\\\slash\"");
    }

    // AC-010: render(Text) for a lone kv_items() section is exactly the kv document.
    #[test]
    fn ac010_render_text_for_kv_items_is_the_kv_document() {
        use crate::kv::{KvItem, KvValue};
        let items = vec![KvItem { key: "id".into(), value: KvValue::Int(1) }];
        let r = AgentResponse::new("t").kv_items(items.clone());
        assert_eq!(Ok(r.render(OutputFormat::Text)), crate::kv::render_kv(&items, None, &[]));
    }

    // AC-012: Missing rendered via the AgentResponse Json body is the TOON
    // `null` literal embedded in the body string, not a nested JSON null.
    #[test]
    #[cfg(feature = "serde")]
    fn ac012_missing_in_kv_items_json_body_is_embedded_null_literal() {
        use crate::kv::{KvItem, KvValue};
        let r = AgentResponse::new("t").kv_items(vec![KvItem { key: "k".into(), value: KvValue::Missing }]);
        let parsed: serde_json::Value = serde_json::from_str(&r.render_json()).expect("valid JSON");
        assert_eq!(parsed["body"].as_str().unwrap(), "k: null");
    }

    // AC-014: NaN/Inf/-Inf via the AgentResponse Json body are the TOON
    // `null` literal (§3) embedded in body text, not standalone JSON values.
    #[test]
    #[cfg(feature = "serde")]
    fn ac014_float_special_values_in_kv_items_json_body_are_embedded_null() {
        use crate::kv::{KvItem, KvValue};
        for (value, token) in [
            (KvValue::Float(f64::NAN, 2), "null"),
            (KvValue::Float(f64::INFINITY, 0), "null"),
            (KvValue::Float(f64::NEG_INFINITY, 0), "null"),
        ] {
            let r = AgentResponse::new("t").kv_items(vec![KvItem { key: "k".into(), value }]);
            let parsed: serde_json::Value = serde_json::from_str(&r.render_json()).expect("valid JSON");
            assert_eq!(parsed["body"].as_str().unwrap(), format!("k: {token}"), "token: {token}");
        }
    }

    // AC-016: Duration via the AgentResponse Json body is the literal '1.5s'
    // token embedded in body text.
    #[test]
    #[cfg(feature = "serde")]
    fn ac016_duration_in_kv_items_json_body_is_embedded_token() {
        use crate::kv::{KvItem, KvValue};
        let r = AgentResponse::new("t").kv_items(vec![KvItem {
            key: "k".into(),
            value: KvValue::Duration(std::time::Duration::from_millis(1500)),
        }]);
        let parsed: serde_json::Value = serde_json::from_str(&r.render_json()).expect("valid JSON");
        assert_eq!(parsed["body"].as_str().unwrap(), "k: 1.5s");
    }

    // AC-041 through AC-045: RecoveryHint params through AgentResponse's Json
    // path use json::push_kv_value (bare null for missing and non-finite floats, bare bool), in
    // contrast to kv_items' plain-text body rendering above.
    #[test]
    #[cfg(feature = "serde")]
    fn ac041_to_ac045_recovery_params_use_json_kv_value_typing() {
        use crate::kv::KvValue;
        use crate::recovery::RecoveryHint;
        let cases: Vec<(KvValue, serde_json::Value)> = vec![
            (KvValue::Missing, serde_json::Value::Null),
            (KvValue::Float(f64::NAN, 2), serde_json::Value::Null),
            (KvValue::Float(f64::INFINITY, 0), serde_json::Value::Null),
            (KvValue::Float(f64::NEG_INFINITY, 0), serde_json::Value::Null),
            (KvValue::Duration(std::time::Duration::from_millis(1500)), serde_json::json!("1.5s")),
            (KvValue::Bool(true), serde_json::json!(true)),
        ];
        for (value, expected) in cases {
            let recovery = RecoveryHint::new("x").param("p", value.clone());
            let r = AgentResponse::new("t").recovery_hint(recovery);
            let parsed: serde_json::Value = serde_json::from_str(&r.render_json()).expect("valid JSON");
            assert_eq!(parsed["recovery"][0]["params"]["p"], expected, "value: {value:?}");
        }
    }

    #[test]
    fn to_call_tool_result_composes_correctly() {
        let r = AgentResponse::new("items").items(vec![vec![Value::from("ok")]], &["name"]).as_error();
        let result = r.to_call_tool_result();
        assert!(result.is_error);
        assert!(!result.content.is_empty(), "content must not be empty");
        assert!(
            result.structured_content.map(String::from).unwrap_or_default().starts_with('{'),
            "structured_content must be JSON object"
        );
    }

    // AC-023: the test above only checks !content.is_empty(), which would
    // still pass with 2+ blocks or a wrong audience/text. Pin the exact shape.
    #[test]
    fn ac023_call_tool_result_has_exactly_one_assistant_block_matching_render_text() {
        let r = AgentResponse::new("items").items(vec![vec![Value::from("ok")]], &["name"]);
        let result = r.to_call_tool_result();
        assert_eq!(result.content.len(), 1);
        assert_eq!(result.content[0].audience, vec![crate::audience::Audience::Assistant]);
        assert_eq!(result.content[0].text, r.render(OutputFormat::Text));
    }

    // AC-024: human_content set adds a second, User-tagged block whose text
    // equals the human_content value exactly.
    #[test]
    fn ac024_call_tool_result_adds_user_block_when_human_content_set() {
        let r = AgentResponse::new("items").human_content("For humans");
        let result = r.to_call_tool_result();
        assert_eq!(result.content.len(), 2);
        assert_eq!(result.content[1].audience, vec![crate::audience::Audience::User]);
        assert_eq!(result.content[1].text, "For humans");
    }

    // AC-025: without human_content, to_call_tool_result() does NOT fall
    // back to a text-output User block (unlike render_for, which does).
    #[test]
    fn ac025_call_tool_result_has_no_user_block_without_human_content() {
        let r = AgentResponse::new("items").items(vec![vec![Value::from("x")]], &["name"]);
        let result = r.to_call_tool_result();
        assert_eq!(result.content.len(), 1);
        assert!(!result.content.iter().any(|c| c.audience == vec![crate::audience::Audience::User]));
    }

    // AC-026: structured_content equals render(Json) exactly, and is_error
    // mirrors self.is_error in both directions.
    #[test]
    fn ac026_call_tool_result_structured_content_and_is_error_match_exactly() {
        let r = AgentResponse::new("items").items(vec![vec![Value::from("ok")]], &["name"]);
        let result = r.to_call_tool_result();
        assert_eq!(result.structured_content.map(String::from).unwrap_or_default(), r.render(OutputFormat::Json));
        assert!(!result.is_error);

        let err = r.as_error();
        let err_result = err.to_call_tool_result();
        assert_eq!(err_result.structured_content.map(String::from).unwrap_or_default(), err.render(OutputFormat::Json));
        assert!(err_result.is_error);
    }

    #[test]
    fn toon_body_returns_error_on_invalid_type_name() {
        let r = AgentResponse::new("help").items(vec![vec![Value::from("x")]], &["name"]);
        let out = r.render(OutputFormat::Text);
        assert!(out.contains("error:"), "expected error output for invalid type_name, got: {out}");
    }

    // AC-031: the test above only checks the generic substring "error:",
    // which many unrelated messages could also contain. Pin the exact
    // required prefix.
    #[test]
    fn ac031_invalid_type_name_produces_the_exact_error_prefix() {
        let r = AgentResponse::new("totalCount");
        let out = r.render(OutputFormat::Text);
        assert!(out.starts_with("error: toon_validation_failed\nmessage: "), "got: {out}");
    }

    // AC-039: a row whose length differs from fields.len() surfaces
    // RowLengthMismatch with row index, expected, and actual counts.
    #[test]
    fn ac039_row_length_mismatch_names_indices_and_counts() {
        let r = AgentResponse::new("t").items(vec![vec![Value::Int(1)]], &["a", "b"]);
        let out = r.render(OutputFormat::Text);
        assert_eq!(out, "error: toon_validation_failed\nmessage: row 0 has 1 values but 2 fields declared");
    }

    // AC-040: a repeated field name surfaces InvalidFieldName naming the offending field; the message is quoted
    // because it contains `"` (TOON 4.4 §7.2).
    #[test]
    fn ac040_invalid_field_name_names_the_offending_field() {
        let r = AgentResponse::new("t").items(vec![], &["a", "a"]);
        let out = r.render(OutputFormat::Text);
        assert_eq!(out, "error: toon_validation_failed\nmessage: \"field \\\"a\\\" appears more than once\"");
    }

    #[test]
    fn render_hints_only_contains_help_block() {
        let r = AgentResponse::new("items").hint("Try again with fewer params");
        let out = r.render_hints_only();
        assert!(out.contains("help[1]:"), "expected help block, got: {out}");
        assert!(out.contains("Try again with fewer params"), "got: {out}");
    }

    #[test]
    fn render_hints_only_empty_when_no_hints() {
        let r = AgentResponse::new("items");
        assert!(r.render_hints_only().is_empty(), "no hints should produce empty string");
    }

    #[test]
    fn render_for_assistant_returns_text_output() {
        let r = AgentResponse::new("items").items(vec![vec![Value::from("ok")]], &["name"]);
        let out = r.render_for(crate::audience::Audience::Assistant);
        assert!(!out.is_empty(), "assistant render must not be empty");
        assert!(out.contains("items"), "got: {out}");
    }

    #[test]
    fn render_for_user_returns_human_content_when_set() {
        let r = AgentResponse::new("items").human_content("Human-readable summary.");
        let out = r.render_for(crate::audience::Audience::User);
        assert_eq!(out, "Human-readable summary.");
    }

    #[test]
    fn render_for_user_falls_back_to_text_when_no_human_content() {
        let r = AgentResponse::new("items").items(vec![vec![Value::from("x")]], &["name"]);
        let assistant_out = r.render_for(crate::audience::Audience::Assistant);
        let user_out = r.render_for(crate::audience::Audience::User);
        assert_eq!(assistant_out, user_out, "user render must match assistant render when no human_content set");
    }

    // AC-021: the test above only proves assistant_out == user_out; the
    // criterion also requires both equal render(Text) — untested until now.
    #[test]
    fn ac021_render_for_fallback_matches_render_text_directly() {
        let r = AgentResponse::new("items").items(vec![vec![Value::from("x")]], &["name"]);
        let text_out = r.render(OutputFormat::Text);
        assert_eq!(r.render_for(crate::audience::Audience::Assistant), text_out);
        assert_eq!(r.render_for(crate::audience::Audience::User), text_out);
    }

    // AC-029: a truncated cell's total char count (including any marker)
    // never exceeds the configured limit.
    #[test]
    fn ac029_truncate_cells_at_bounds_total_cell_length() {
        let r = AgentResponse::new("t").truncate_cells_at(5).items(vec![vec![Value::from("abcdefghij")]], &["name"]);
        let out = r.render(OutputFormat::Text);
        let row_line = out.lines().nth(1).expect("row line present");
        // A 5-char cap cannot hold the suffix, so the cell is 4 kept chars and an ellipsis.
        assert_eq!(row_line, "  abcd…");
    }

    #[test]
    fn truncation_hint_passes_through_to_toon_body() {
        let rows = vec![vec![Value::from("a".repeat(500))]];
        let without = AgentResponse::new("t").items(rows.clone(), &["c"]).render(OutputFormat::Text);
        let cut = format!("t[1]{{c}}:\n  {}\u{2026}\ntruncated[1]{{row,field,removed}}:\n  0,c,301", "a".repeat(199));
        assert_eq!(without, cut);
        let with = AgentResponse::new("t").truncation_hint("detail=full").items(rows, &["c"]);
        assert_eq!(
            with.render(OutputFormat::Text),
            format!("{cut}\nhelp[1]: Cut cells are listed in the truncated block; use detail=full for the full value")
        );
        assert!(with.render(OutputFormat::Json).contains("use detail=full"));
    }

    // AC-030: truncate_cells_at has no observable effect on the KV path.
    #[test]
    fn ac030_truncate_cells_at_does_not_affect_kv_path() {
        use crate::kv::{KvItem, KvValue};
        let long_value = "x".repeat(50);
        let r = AgentResponse::new("t")
            .truncate_cells_at(5)
            .kv_items(vec![KvItem { key: "k".into(), value: KvValue::Text(long_value.clone()) }]);
        let out = r.render(OutputFormat::Text);
        assert!(out.contains(&long_value), "value must be untruncated, got: {out}");
    }

    // AC-051/052: a section's total count reaches the text on both the KV and the TOON path, and only when it exceeds
    // the rows returned.
    #[test]
    fn ac051_ac052_section_total_count_is_rendered_on_both_paths_only_when_larger() {
        use crate::kv::{KvItem, KvValue};
        let kv = |total| Section::kv("k", vec![KvItem { key: "k".into(), value: KvValue::Int(1) }]).total_count(total);
        let toon = |total| Section::toon("t", vec!["c".into()], vec![vec![Value::from("x")]]).total_count(total);
        let text = |s: Section| AgentResponse::new("t").section(s).render(OutputFormat::Text);
        assert_eq!(text(kv(Some(3))), "k: 1\ntotalCount: 3");
        assert_eq!(text(toon(Some(3))), "t[1]{c}:\n  x\ntotalCount: 3");
        assert_eq!(text(kv(Some(1))), "k: 1", "a total equal to the rows adds nothing");
        assert_eq!(text(toon(None)), "t[1]{c}:\n  x");
    }

    // AC-053: .hints() replaces the full hint list rather than appending to
    // hints previously added via .hint() — contrast with .recovery_hint()'s
    // append semantics (AC-006).
    #[test]
    fn ac053_hints_replaces_rather_than_appends() {
        let r = AgentResponse::new("t").hint("a").hints(vec![Hint::from("b")]);
        let out = r.render_hints_only();
        assert!(out.contains('b'), "got: {out}");
        assert!(!out.contains('a'), "got: {out}");
    }

    // AC-038 (extended): json::push_str's generic below-0x20 control-
    // character branch (\u00XX), distinct from the dedicated \n/\r/\t arms.
    #[test]
    #[cfg(feature = "serde")]
    fn ac038_control_character_below_0x20_is_escaped_and_json_stays_valid() {
        let r = AgentResponse::new("t").hint("before\u{1}after");
        let raw = r.render_json();
        assert!(raw.contains(r"\u0001"), "got: {raw}");
        let parsed: serde_json::Value = serde_json::from_str(&raw).expect("must be valid JSON");
        assert_eq!(parsed["hints"][0].as_str().unwrap(), "before\u{1}after");
    }

    // AC-049: DEFAULT_TRUNCATE_CELLS is a public constant equal to 200.
    #[test]
    fn ac049_default_truncate_cells_is_200() {
        assert_eq!(DEFAULT_TRUNCATE_CELLS, 200);
    }

    // AC-050: without an explicit .truncate_cells_at() call, the default
    // (200) boundary holds: 199 chars untruncated, 250 chars truncated.
    #[test]
    fn ac050_default_truncate_boundary_holds_without_explicit_call() {
        let untruncated = AgentResponse::new("t").items(vec![vec![Value::from("x".repeat(199))]], &["c"]);
        let out = untruncated.render(OutputFormat::Text);
        let cell_line = out.lines().nth(1).expect("row line present");
        assert!(cell_line.contains(&"x".repeat(199)), "199 chars must be untruncated, got: {cell_line:?}");

        let truncated = AgentResponse::new("t").items(vec![vec![Value::from("x".repeat(250))]], &["c"]);
        let out = truncated.render(OutputFormat::Text);
        let cell_line = out.lines().nth(1).expect("row line present");
        let value_part = cell_line.trim_start();
        assert!(
            value_part.chars().count() <= DEFAULT_TRUNCATE_CELLS,
            "got line with {} chars: {cell_line:?}",
            value_part.chars().count()
        );
    }

    #[test]
    fn has_human_content_false_when_empty() {
        let r = AgentResponse::new("items");
        assert!(!r.has_human_content());
    }

    #[test]
    fn has_human_content_true_when_set() {
        let r = AgentResponse::new("items").human_content("Some text for humans.");
        assert!(r.has_human_content());
    }
}
