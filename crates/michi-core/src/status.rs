use crate::hints::Hint;
use crate::kv::KvValue;

/// Health classification for an individual status item.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Health {
    /// Operating normally — renders health `ok` and reason `null`.
    Ok,
    /// Degraded but still serving; carries a short reason. Renders health `degraded`.
    Degraded(String),
    /// Not serving; carries a short reason. Renders health `error`.
    Error(String),
}

/// A single named component with a value and optional health signal.
///
/// `#[non_exhaustive]`: build it with [`StatusItem::new`] (and [`StatusItem::health`]); the fields stay public.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct StatusItem {
    /// Component key, e.g. `"index"`, `"cache"`.
    pub key: String,
    /// The component's actual value.
    pub value: KvValue,
    /// Optional health signal; `None` renders health and reason as `null`.
    pub health: Option<Health>,
}

impl StatusItem {
    /// A status item with no health signal.
    #[must_use]
    pub fn new(key: impl Into<String>, value: KvValue) -> Self {
        Self { key: key.into(), value, health: None }
    }

    /// Attach a health signal.
    #[must_use]
    pub fn health(mut self, health: Health) -> Self {
        self.health = Some(health);
        self
    }
}

/// Content-first orientation response (AXI **P8**).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct StatusResponse {
    /// The tool's name, shown as the first line.
    pub tool_name: String,
    /// One-line description, shown as the second line.
    pub description: String,
    /// Component statuses.
    pub items: Vec<StatusItem>,
    /// Optional contextual hints for the agent.
    pub hints: Vec<Hint>,
}

impl StatusResponse {
    /// Create a status response.
    pub fn new(tool_name: impl Into<String>, description: impl Into<String>, items: Vec<StatusItem>) -> Self {
        Self { tool_name: tool_name.into(), description: description.into(), items, hints: Vec::new() }
    }

    /// Replace the contextual hints with `hints`.
    #[must_use]
    pub fn hints(mut self, hints: Vec<Hint>) -> Self {
        self.hints = hints;
        self
    }

    /// Render this status response as a TOON 4.4 document, with no trailing newline: the response
    /// `AgentResponse::new("items").status(self)` with no cell cap, so it is the same writer a composed response uses
    /// (see [`crate::AgentResponse::status`]).
    ///
    /// ```text
    /// tool: my-tool
    /// description: does things
    /// items[2]{key,value,health,reason}:
    ///   index,ready,ok,null
    ///   cache,warm,degraded,near limit
    /// help[1]: Call get_status for detail
    /// ```
    ///
    /// `items` is a tabular array (§9.3); with no items it is `items: []` and `totalCount: 0`, the definitive empty
    /// list. `health` is `ok`, `degraded`, `error`, or `null` when the item carries no health signal; `reason` is the
    /// degraded/error reason, `null` otherwise. Item keys are cells, not root keys, so any key — even a repeated one —
    /// keeps the document valid. Every string is quoted per §7.2 and the value is the token [`KvValue`] renders.
    #[must_use]
    pub fn render(&self) -> String {
        crate::AgentResponse::new("items")
            .truncate_cells_at(usize::MAX)
            .status(self.clone())
            .render(crate::OutputFormat::Text)
    }

    /// The two sections a composed response holds: `tool` and `description` as a KV section named `status`, then the
    /// `items` table.
    pub(crate) fn sections(&self) -> [crate::section::Section; 2] {
        use crate::section::Section;
        use crate::toon::Value;
        let head = Section::kv(
            "status",
            vec![
                crate::kv::KvItem::new("tool", KvValue::Text(self.tool_name.clone())),
                crate::kv::KvItem::new("description", KvValue::Text(self.description.clone())),
            ],
        );
        let rows = self
            .items
            .iter()
            .map(|item| {
                let (health, reason) = match &item.health {
                    None => (Value::Null, Value::Null),
                    Some(Health::Ok) => ("ok".into(), Value::Null),
                    Some(Health::Degraded(reason)) => ("degraded".into(), reason.as_str().into()),
                    Some(Health::Error(reason)) => ("error".into(), reason.as_str().into()),
                };
                vec![item.key.as_str().into(), crate::kv::to_cell(&item.value), health, reason]
            })
            .collect();
        let fields = ["key", "value", "health", "reason"].map(String::from).to_vec();
        [head, Section::toon("items", fields, rows)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEAD: &str = "tool: tool\ndescription: desc\nitems[1]{key,value,health,reason}:\n  ";

    fn one(key: &str, value: KvValue, health: Option<Health>) -> String {
        StatusResponse::new("tool", "desc", vec![StatusItem { key: key.into(), value, health }]).render()
    }

    #[test]
    fn tool_and_description_lead_the_output() {
        let resp = StatusResponse::new("my-search-tool", "Semantic code search and symbol analysis", vec![]);
        let out = resp.render();
        assert!(
            out.starts_with("tool: my-search-tool\ndescription: Semantic code search and symbol analysis\n"),
            "got: {out}"
        );
    }

    #[test]
    fn empty_items_renders_tool_description_and_an_empty_items_array() {
        let out = StatusResponse::new("tool-x", "does things", vec![]).render();
        assert_eq!(out, "tool: tool-x\ndescription: does things\nitems: []\ntotalCount: 0");
    }

    #[test]
    fn health_ok_renders_ok_with_null_reason() {
        let out = one("index", KvValue::Text("ready".into()), Some(Health::Ok));
        assert_eq!(out, format!("{HEAD}index,ready,ok,null"));
    }

    #[test]
    fn health_degraded_renders_its_reason() {
        let out = one("cache", KvValue::Text("warm".into()), Some(Health::Degraded("slow eviction".into())));
        assert_eq!(out, format!("{HEAD}cache,warm,degraded,slow eviction"));
    }

    #[test]
    fn health_error_renders_its_reason() {
        let out = one("db", KvValue::Text("unreachable".into()), Some(Health::Error("connection refused".into())));
        assert_eq!(out, format!("{HEAD}db,unreachable,error,connection refused"));
    }

    #[test]
    fn multiple_items_all_appear_in_order() {
        let resp = StatusResponse::new(
            "tool",
            "desc",
            vec![
                StatusItem { key: "alpha".into(), value: KvValue::Int(1), health: None },
                StatusItem { key: "beta".into(), value: KvValue::Int(2), health: None },
            ],
        );
        assert_eq!(
            resp.render(),
            "tool: tool\ndescription: desc\nitems[2]{key,value,health,reason}:\n  alpha,1,null,null\n  beta,2,null,null"
        );
    }

    #[test]
    fn ac019_health_is_non_exhaustive() {
        let src = include_str!("status.rs");
        let idx = src.find("pub enum Health").expect("pub enum Health present");
        let preceding = &src[..idx];
        let attr_start = preceding.rfind("#[non_exhaustive]").expect("#[non_exhaustive] precedes pub enum Health");
        // confirm no other pub enum/struct declaration sits between the attribute and Health
        assert!(!preceding[attr_start..].contains("pub enum") && !preceding[attr_start..].contains("pub struct"));
    }

    #[test]
    fn ac021_new_defaults_hints_empty_and_hints_is_a_consuming_builder() {
        let resp = StatusResponse::new("tool", "desc", vec![]);
        assert!(resp.hints.is_empty());

        let hints = vec![Hint::new("a hint")];
        let with = resp.hints(hints.clone());
        assert_eq!(with.hints, hints);
        assert_eq!(with.tool_name, "tool");
        assert_eq!(with.description, "desc");
        assert!(with.items.is_empty());
    }

    // AC-023 (superseded): 3 lines (tool, description, items header) plus one row per item; no items is `items: []` and
    // `totalCount: 0`.
    #[test]
    fn ac023_line_count_is_3_plus_n_with_default_hints() {
        for n in 0..=3 {
            let items: Vec<StatusItem> = (0..n)
                .map(|i| StatusItem {
                    key: format!("k{i}"),
                    value: KvValue::Int(i.try_into().unwrap_or(0)),
                    health: None,
                })
                .collect();
            let out = StatusResponse::new("tool", "desc", items).render();
            assert_eq!(out.lines().count(), 3 + n.max(1), "n={n}, got: {out:?}");
        }
    }

    #[test]
    fn ac023a_hints_appends_the_help_block_on_its_own_line() {
        let items = || vec![StatusItem { key: "k".into(), value: KvValue::Int(1), health: None }];
        let hints = vec![Hint::new("h1"), Hint::new("h2")];
        let out = StatusResponse::new("tool", "desc", items()).hints(hints.clone()).render();
        let no_hints = StatusResponse::new("tool", "desc", items()).render();
        assert_eq!(out, format!("{no_hints}\n{}", crate::hints::render_hints(&hints)));
    }

    #[test]
    fn ac023a_newline_in_item_key_is_quoted_and_escaped() {
        let out = one("a\nb", KvValue::Text("v".into()), None);
        assert_eq!(out, format!("{HEAD}\"a\\nb\",v,null,null"));
        assert_eq!(out.lines().count(), 4, "the escaped \\n adds no line, got: {out:?}");
    }

    // AC-024 (superseded): `Health::Ok` renders `ok`; `None` renders `null` — the item carries no health signal.
    #[test]
    fn ac024_health_ok_and_none_are_distinct() {
        assert_eq!(one("x", KvValue::Int(1), Some(Health::Ok)), format!("{HEAD}x,1,ok,null"));
        assert_eq!(one("x", KvValue::Int(1), None), format!("{HEAD}x,1,null,null"));
    }

    #[test]
    fn ac024_value_text_containing_bracket_literal_is_quoted_not_mistaken_for_health() {
        let out = one("x", KvValue::Text("[DEGRADED: fake]".into()), None);
        assert_eq!(out, format!("{HEAD}x,\"[DEGRADED: fake]\",null,null"));
    }

    #[test]
    fn ac023_carriage_return_in_item_key_is_escaped() {
        let out = one("a\rb", KvValue::Text("v".into()), None);
        assert_eq!(out, format!("{HEAD}\"a\\rb\",v,null,null"));
        assert!(!out.contains('\r'), "no raw CR may survive, got: {out:?}");
    }

    #[test]
    fn ac023_text_value_with_newline_and_carriage_return_is_escaped() {
        let out = one("k", KvValue::Text("a\nb\rc".into()), None);
        assert_eq!(out, format!("{HEAD}k,\"a\\nb\\rc\",null,null"));
        assert_eq!(out.lines().count(), 4);
    }

    #[test]
    fn ac022a_tool_name_and_description_newlines_are_escaped() {
        let out = StatusResponse::new("a\nb", "c\nd", vec![]).render();
        assert_eq!(out, "tool: \"a\\nb\"\ndescription: \"c\\nd\"\nitems: []\ntotalCount: 0");
    }

    #[test]
    fn ac025a_degraded_reason_newline_is_escaped() {
        let out = one("x", KvValue::Text("v".into()), Some(Health::Degraded("a\nb".into())));
        assert_eq!(out, format!("{HEAD}x,v,degraded,\"a\\nb\""));
    }

    #[test]
    fn ac026a_error_reason_newline_is_escaped() {
        let out = one("x", KvValue::Text("v".into()), Some(Health::Error("a\nb".into())));
        assert_eq!(out, format!("{HEAD}x,v,error,\"a\\nb\""));
    }
}
