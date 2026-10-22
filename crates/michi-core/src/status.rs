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
#[derive(Debug, Clone, PartialEq)]
pub struct StatusItem {
    /// Component key, e.g. `"index"`, `"cache"`.
    pub key: String,
    /// The component's actual value.
    pub value: KvValue,
    /// Optional health signal; `None` renders health and reason as `null`.
    pub health: Option<Health>,
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

    /// Attach contextual hints.
    #[must_use]
    pub fn with_hints(mut self, hints: Vec<Hint>) -> Self {
        self.hints = hints;
        self
    }

    /// Render this status response as a TOON 4.4 document, with no trailing newline:
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
    /// `items` is a tabular array (§9.3), `items: []` when there are none. `health` is `ok`, `degraded`, `error`, or
    /// `null` when the item carries no health signal; `reason` is the degraded/error reason, `null` otherwise. Item
    /// keys are cells, not root keys, so any key — even a repeated one — keeps the document valid. Every string is
    /// quoted per §7.2 and the value is a TOON primitive (see [`KvValue`]).
    #[must_use]
    pub fn render(&self) -> String {
        let capacity = 48
            + self.tool_name.len()
            + self.description.len()
            + self.items.iter().map(|i| i.key.len() + 32).sum::<usize>()
            + self.hints.len() * 50;
        let mut out = String::with_capacity(capacity);
        out.push_str("tool: ");
        out.push_str(&crate::toon::escape_value(&self.tool_name));
        out.push_str("\ndescription: ");
        out.push_str(&crate::toon::escape_value(&self.description));
        if self.items.is_empty() {
            out.push_str("\nitems: []");
        } else {
            out.push_str("\nitems[");
            out.push_str(&self.items.len().to_string());
            out.push_str("]{key,value,health,reason}:");
            for item in &self.items {
                out.push_str("\n  ");
                out.push_str(&crate::toon::escape_value(&item.key));
                out.push(',');
                crate::kv::push_kv_value(&mut out, &item.value);
                let (health, reason) = match &item.health {
                    None => ("null", None),
                    Some(Health::Ok) => ("ok", None),
                    Some(Health::Degraded(reason)) => ("degraded", Some(reason)),
                    Some(Health::Error(reason)) => ("error", Some(reason)),
                };
                out.push(',');
                out.push_str(health);
                out.push(',');
                match reason {
                    Some(reason) => out.push_str(&crate::toon::escape_value(reason)),
                    None => out.push_str("null"),
                }
            }
        }
        crate::hints::append_hints(&mut out, &self.hints);
        out
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
        assert_eq!(out, "tool: tool-x\ndescription: does things\nitems: []");
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
    fn ac021_new_defaults_hints_empty_and_with_hints_is_a_consuming_builder() {
        let resp = StatusResponse::new("tool", "desc", vec![]);
        assert!(resp.hints.is_empty());

        let hints = vec![Hint::new("a hint")];
        let with = resp.with_hints(hints.clone());
        assert_eq!(with.hints, hints);
        assert_eq!(with.tool_name, "tool");
        assert_eq!(with.description, "desc");
        assert!(with.items.is_empty());
    }

    // AC-023 (superseded): 3 lines (tool, description, items header) plus one row per item.
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
            assert_eq!(out.lines().count(), 3 + n, "n={n}, got: {out:?}");
        }
    }

    #[test]
    fn ac023a_with_hints_appends_the_help_block_on_its_own_line() {
        let items = || vec![StatusItem { key: "k".into(), value: KvValue::Int(1), health: None }];
        let hints = vec![Hint::new("h1"), Hint::new("h2")];
        let out = StatusResponse::new("tool", "desc", items()).with_hints(hints.clone()).render();
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
        assert_eq!(out, "tool: \"a\\nb\"\ndescription: \"c\\nd\"\nitems: []");
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
