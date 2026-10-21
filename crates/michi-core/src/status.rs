use crate::hints::Hint;
use crate::kv::{KvItem, KvValue};

/// Health classification for an individual status item.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum Health {
    /// Operating normally — no annotation shown.
    Ok,
    /// Degraded but still serving; carries a short reason.
    Degraded(String),
    /// Not serving; carries a short reason.
    Error(String),
}

/// A single named component with a value and optional health signal.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct StatusItem {
    /// Component key, e.g. `"index"`, `"cache"`.
    pub key: String,
    /// The component's actual value.
    pub value: KvValue,
    /// Optional health signal.
    pub health: Option<Health>,
}

/// Content-first orientation response (AXI **P8**).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
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

    /// Render this status response as an agent-readable string.
    #[must_use]
    pub fn render(&self) -> String {
        let mut kv_items = Vec::with_capacity(self.items.len() + 2);
        kv_items.push(KvItem { key: "tool".to_string(), value: KvValue::Text(self.tool_name.clone()) });
        kv_items.push(KvItem { key: "description".to_string(), value: KvValue::Text(self.description.clone()) });
        for item in &self.items {
            let annotated = match &item.health {
                None | Some(Health::Ok) => item.value.clone(),
                Some(Health::Degraded(reason)) => annotate(&item.value, "DEGRADED", reason),
                Some(Health::Error(reason)) => annotate(&item.value, "ERROR", reason),
            };
            kv_items.push(KvItem { key: item.key.clone(), value: annotated });
        }
        crate::kv::render_kv(&kv_items, None, &self.hints)
    }
}

fn annotate(value: &KvValue, label: &str, reason: &str) -> KvValue {
    let mut display = String::new();
    crate::kv::push_kv_value(&mut display, value);
    KvValue::Text(format!("{display}  [{label}: {reason}]"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_and_description_lead_the_output() {
        let resp = StatusResponse::new("my-search-tool", "Semantic code search and symbol analysis", vec![]);
        let out = resp.render();
        assert!(
            out.starts_with("tool:        my-search-tool\ndescription: Semantic code search and symbol analysis\n"),
            "got: {out}"
        );
    }

    #[test]
    fn empty_items_renders_only_tool_and_description() {
        let resp = StatusResponse::new("tool-x", "does things", vec![]);
        let out = resp.render();
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 2, "only tool + description lines, got: {out}");
    }

    #[test]
    fn health_ok_produces_no_annotation() {
        let resp = StatusResponse::new(
            "tool",
            "desc",
            vec![StatusItem { key: "index".into(), value: KvValue::Text("ready".into()), health: Some(Health::Ok) }],
        );
        let out = resp.render();
        assert!(!out.contains("DEGRADED") && !out.contains("ERROR"), "Ok should add no annotation, got: {out}");
    }

    #[test]
    fn health_degraded_annotation_appears() {
        let resp = StatusResponse::new(
            "tool",
            "desc",
            vec![StatusItem {
                key: "cache".into(),
                value: KvValue::Text("warm".into()),
                health: Some(Health::Degraded("slow eviction".into())),
            }],
        );
        let out = resp.render();
        assert!(out.contains("[DEGRADED: slow eviction]"), "got: {out}");
    }

    #[test]
    fn ac025_degraded_suffix_has_exactly_two_leading_spaces() {
        let resp = StatusResponse::new(
            "tool",
            "desc",
            vec![StatusItem {
                key: "cache".into(),
                value: KvValue::Text("warm".into()),
                health: Some(Health::Degraded("slow eviction".into())),
            }],
        );
        let out = resp.render();
        let line = out.lines().find(|l| l.starts_with("cache:")).expect("cache line present");
        assert!(line.ends_with("  [DEGRADED: slow eviction]"), "got: {line:?}");
        assert!(!line.ends_with("   [DEGRADED: slow eviction]"), "three spaces, got: {line:?}");
    }

    #[test]
    fn health_error_annotation_appears() {
        let resp = StatusResponse::new(
            "tool",
            "desc",
            vec![StatusItem {
                key: "db".into(),
                value: KvValue::Text("unreachable".into()),
                health: Some(Health::Error("connection refused".into())),
            }],
        );
        let out = resp.render();
        assert!(out.contains("[ERROR: connection refused]"), "got: {out}");
    }

    #[test]
    fn ac026_error_suffix_has_exactly_two_leading_spaces() {
        let resp = StatusResponse::new(
            "tool",
            "desc",
            vec![StatusItem {
                key: "db".into(),
                value: KvValue::Text("unreachable".into()),
                health: Some(Health::Error("connection refused".into())),
            }],
        );
        let out = resp.render();
        let line = out.lines().find(|l| l.starts_with("db:")).expect("db line present");
        assert!(line.ends_with("  [ERROR: connection refused]"), "got: {line:?}");
        assert!(!line.ends_with("   [ERROR: connection refused]"), "three spaces, got: {line:?}");
    }

    #[test]
    fn multiple_items_all_appear() {
        let resp = StatusResponse::new(
            "tool",
            "desc",
            vec![
                StatusItem { key: "alpha".into(), value: KvValue::Int(1), health: None },
                StatusItem { key: "beta".into(), value: KvValue::Int(2), health: None },
            ],
        );
        let out = resp.render();
        assert!(out.contains("alpha:"), "got: {out}");
        assert!(out.contains("beta:"), "got: {out}");
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

    #[test]
    fn ac023_line_count_is_2_plus_n_with_default_hints() {
        for n in 0..=3 {
            let items: Vec<StatusItem> = (0..n)
                .map(|i| StatusItem {
                    key: format!("k{i}"),
                    value: KvValue::Int(i.try_into().unwrap_or(0)),
                    health: None,
                })
                .collect();
            let resp = StatusResponse::new("tool", "desc", items);
            let out = resp.render();
            assert_eq!(out.lines().count(), 2 + n, "n={n}, got: {out:?}");
        }
    }

    #[test]
    fn ac023a_with_hints_appends_append_hints_block_after_the_2_plus_n_lines() {
        let items = vec![StatusItem { key: "k".into(), value: KvValue::Int(1), health: None }];
        let hints = vec![Hint::new("h1"), Hint::new("h2")];
        let resp = StatusResponse::new("tool", "desc", items).with_hints(hints.clone());
        let out = resp.render();
        let no_hints = StatusResponse::new(
            "tool",
            "desc",
            vec![StatusItem { key: "k".into(), value: KvValue::Int(1), health: None }],
        )
        .render();
        assert!(out.starts_with(&no_hints), "got: {out:?}");
        assert_eq!(&out[no_hints.len()..], crate::hints::render_hints(&hints));
    }

    #[test]
    fn ac023a_newline_in_item_key_is_escaped_and_keeps_the_2_plus_n_formula_with_hints() {
        let items = vec![StatusItem { key: "a\nb".into(), value: KvValue::Text("v".into()), health: None }];
        let hints = vec![Hint::new("h1")];
        let resp = StatusResponse::new("tool", "desc", items).with_hints(hints);
        let out = resp.render();
        assert_eq!(out, "tool:        tool\ndescription: desc\na\\nb:        v\nhelp[1]: h1\n");
        let lines_before_help = out.lines().take_while(|l| !l.starts_with("help[")).count();
        assert_eq!(lines_before_help, 3, "2 (tool+desc) + 1 item, the escaped \\n adds no line, got: {out:?}");
    }

    #[test]
    fn ac024_health_ok_renders_byte_identical_to_health_none() {
        let ok_item = StatusItem { key: "x".into(), value: KvValue::Int(1), health: Some(Health::Ok) };
        let none_item = StatusItem { key: "x".into(), value: KvValue::Int(1), health: None };
        let ok_out = StatusResponse::new("tool", "desc", vec![ok_item]).render();
        let none_out = StatusResponse::new("tool", "desc", vec![none_item]).render();
        assert_eq!(ok_out, none_out);
    }

    #[test]
    fn ac024_value_text_containing_bracket_literal_is_not_mistaken_for_a_real_annotation() {
        let item = StatusItem { key: "x".into(), value: KvValue::Text("[DEGRADED: fake]".into()), health: None };
        let out = StatusResponse::new("tool", "desc", vec![item]).render();
        assert_eq!(out, "tool:        tool\ndescription: desc\nx:           [DEGRADED: fake]\n");
    }

    #[test]
    fn ac023b_newline_in_item_key_is_escaped_and_adds_no_line_beyond_the_2_plus_n_formula() {
        let items = vec![StatusItem { key: "a\nb".into(), value: KvValue::Text("v".into()), health: None }];
        let resp = StatusResponse::new("tool", "desc", items);
        let out = resp.render();
        assert_eq!(out, "tool:        tool\ndescription: desc\na\\nb:        v\n");
        assert_eq!(out.lines().count(), 3, "2 (tool+desc) + 1 item, the escaped \\n adds no line, got: {out:?}");
    }

    #[test]
    fn ac023_carriage_return_in_item_key_is_escaped() {
        let items = vec![StatusItem { key: "a\rb".into(), value: KvValue::Text("v".into()), health: None }];
        let resp = StatusResponse::new("tool", "desc", items);
        let out = resp.render();
        assert_eq!(out, "tool:        tool\ndescription: desc\na\\rb:        v\n");
        assert!(!out.contains('\r'), "no raw CR may survive, got: {out:?}");
    }

    #[test]
    fn ac023_text_value_with_newline_and_carriage_return_is_escaped_and_preserves_line_count() {
        let items = vec![StatusItem { key: "k".into(), value: KvValue::Text("a\nb\rc".into()), health: None }];
        let resp = StatusResponse::new("tool", "desc", items);
        let out = resp.render();
        assert_eq!(out, "tool:        tool\ndescription: desc\nk:           a\\nb\\rc\n");
        assert_eq!(out.lines().count(), 3, "2 (tool+desc) + 1 item, value is escaped so count is unaffected");
    }

    #[test]
    fn ac022a_tool_name_and_description_newlines_are_escaped() {
        let resp = StatusResponse::new("a\nb", "c\nd", vec![]);
        let out = resp.render();
        assert_eq!(out, "tool:        a\\nb\ndescription: c\\nd\n");
    }

    #[test]
    fn ac025a_degraded_reason_newline_is_escaped() {
        let items = vec![StatusItem {
            key: "x".into(),
            value: KvValue::Text("v".into()),
            health: Some(Health::Degraded("a\nb".into())),
        }];
        let resp = StatusResponse::new("tool", "desc", items);
        let out = resp.render();
        assert!(out.contains("[DEGRADED: a\\nb]"), "got: {out:?}");
        assert_eq!(out.lines().count(), 3);
    }

    #[test]
    fn ac026a_error_reason_newline_is_escaped() {
        let items = vec![StatusItem {
            key: "x".into(),
            value: KvValue::Text("v".into()),
            health: Some(Health::Error("a\nb".into())),
        }];
        let resp = StatusResponse::new("tool", "desc", items);
        let out = resp.render();
        assert!(out.contains("[ERROR: a\\nb]"), "got: {out:?}");
        assert_eq!(out.lines().count(), 3);
    }
}
