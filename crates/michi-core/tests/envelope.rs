//! The envelope wired into `AgentResponse`: sections, `budget:` and `provenance:` together (spec 07, slice 5).

use michi_core::kv::{KvItem, KvValue};
use michi_core::toon::Value;
use michi_core::{AgentResponse, BudgetSummary, OutputFormat, ProvenanceBlock, RecoveryHint, Section};
use proptest::prelude::*;

mod support;
use support::toon_parser::{decode, Node, Prim};

fn fields(names: &[&str]) -> Vec<String> {
    names.iter().map(|s| (*s).to_string()).collect()
}

fn symbols() -> Section {
    Section::toon("brief.symbols", fields(&["name"]), vec![vec![Value::from("RetryPolicy")]]).total_count(Some(40))
}

fn refs() -> Section {
    Section::toon("brief.refs", fields(&["symbol"]), vec![vec![Value::from("RetryPolicy")]])
}

fn long_cell_section(name: &str) -> Section {
    Section::toon(name, fields(&["c"]), vec![vec![Value::from("x".repeat(50))]])
}

fn valid(text: &str) -> support::toon_parser::Doc {
    decode(text).unwrap_or_else(|e| panic!("not conformant TOON: {e}\n{text}"))
}

fn json(r: &AgentResponse) -> serde_json::Value {
    let text = r.render(OutputFormat::Json);
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("invalid JSON: {e}\n{text}"))
}

#[test]
fn full_envelope_renders_in_the_documented_order() {
    let budget = BudgetSummary::new().tokens_used(10);
    let r = AgentResponse::new("brief")
        .section(symbols())
        .section(refs())
        .budget(budget)
        .provenance(ProvenanceBlock::new().entry("analyzer", "ra").fingerprint("abc"))
        .hint("narrow it")
        .recovery_hint(RecoveryHint::new("search"));
    let out = r.render(OutputFormat::Text);
    let at = |needle: &str| out.find(needle).unwrap_or_else(|| panic!("{needle} missing in\n{out}"));
    assert!(at("brief.symbols") < at("brief.refs"));
    assert!(at("brief.refs") < at("budget:"));
    assert!(at("budget:") < at("sections[2]{name,returned,total,truncated}:"));
    assert!(at("sections[2]") < at("provenance:"));
    assert!(at("provenance:") < at("help["));
    assert!(at("help[") < at("recovery["));
    valid(&out);
}

#[test]
fn sugar_and_sections_all_count_as_blocks_for_the_rollup() {
    let budget = || BudgetSummary::new().max_items(5);
    // `items()` is a section: it plus one more is two blocks, in call order, roll-up shown.
    let two = AgentResponse::new("t").items(vec![vec![Value::Int(1)]], &["n"]).section(refs()).budget(budget());
    let out = two.render(OutputFormat::Text);
    assert!(out.starts_with("t[1]{n}:\n  1\nbrief.refs[1]{symbol}:"), "{out}");
    assert!(
        out.ends_with("sections[2]{name,returned,total,truncated}:\n  t,1,null,false\n  brief.refs,1,null,false"),
        "{out}"
    );
    // An empty KV section is still a section: two blocks, roll-up shown.
    let empty_kv = AgentResponse::new("t").kv_items(vec![]).section(refs()).budget(budget());
    assert!(empty_kv.render(OutputFormat::Text).contains("sections["), "{}", empty_kv.render(OutputFormat::Text));
    let one = AgentResponse::new("t").section(refs()).budget(budget());
    assert!(!one.render(OutputFormat::Text).contains("sections["));
    // A single `items()` section: no roll-up either.
    let slot_only = AgentResponse::new("t").items(vec![vec![Value::Int(1)]], &["n"]).budget(budget());
    assert!(!slot_only.render(OutputFormat::Text).contains("sections["));
}

#[test]
fn budget_or_provenance_alone_has_no_empty_list_line() {
    let r = AgentResponse::new("t").budget(BudgetSummary::new().tokens_used(3));
    assert_eq!(r.render(OutputFormat::Text), "budget:\n  tokensUsed: 3\n  truncated: false");
    let r = AgentResponse::new("t").provenance(ProvenanceBlock::new().entry("a", "b"));
    assert_eq!(r.render(OutputFormat::Text), "provenance:\n  a: b");
    assert!(!r.is_error());
}

#[test]
fn a_response_with_no_section_renders_the_empty_list() {
    assert_eq!(AgentResponse::new("t").render(OutputFormat::Text), "t: []\ntotalCount: 0");
    let r = AgentResponse::new("t")
        .section(Section::toon("t", fields(&["n"]), vec![vec![Value::Int(1)]]).total_count(Some(9)));
    assert_eq!(r.render(OutputFormat::Text), "t[1]{n}:\n  1\ntotalCount: 9");
}

#[test]
fn one_cut_section_keeps_the_single_slot_shape() {
    let cut = AgentResponse::new("t")
        .section(long_cell_section("a").total_count(Some(9)))
        .section(refs())
        .truncate_cells_at(20);
    let out = cut.render(OutputFormat::Text);
    assert!(
        out.contains("a[1]{c}:\n  xxxxxxxxxxxxxxxxxxx…\ntruncated[1]{row,field,removed}:\n  0,c,31\ntotalCount: 9\n"),
        "{out}"
    );
    valid(&out);
}

#[test]
fn several_cut_sections_share_one_valid_truncated_table() {
    let r = AgentResponse::new("t")
        .section(long_cell_section("a"))
        .section(long_cell_section("b,c"))
        .section(refs())
        .truncate_cells_at(20)
        .truncation_hint("full=true");
    let out = r.render(OutputFormat::Text);
    assert_eq!(out.matches("truncated[").count(), 1, "{out}");
    assert!(out.contains("truncated[2]{section,row,field,removed}:\n  a,0,c,31\n  \"b,c\",0,c,31"), "{out}");
    assert_eq!(out.matches("help[").count(), 1, "{out}");
    assert_eq!(out.matches("Cut cells are listed").count(), 1, "one michi line, not one per section: {out}");
    let doc = valid(&out);
    let Some(Node::Table { fields, rows }) = doc.get("truncated") else { panic!("no truncated table in\n{out}") };
    assert_eq!(fields, &["section", "row", "field", "removed"]);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1][0], Prim::str("b,c"));
}

#[test]
fn several_totals_share_one_valid_table() {
    let r = AgentResponse::new("t")
        .section(Section::toon("t", fields(&["n"]), vec![vec![Value::Int(1)]]).total_count(Some(5)))
        .section(symbols())
        .section(refs());
    let out = r.render(OutputFormat::Text);
    assert_eq!(out.matches("totalCount").count(), 1, "{out}");
    assert!(out.ends_with("totalCount[2]{section,total}:\n  t,5\n  brief.symbols,40"), "{out}");
    let doc = valid(&out);
    assert!(matches!(doc.get("totalCount"), Some(Node::Table { rows, .. }) if rows.len() == 2));
}

#[test]
fn json_carries_sections_budget_and_provenance_in_camel_case() {
    let r = AgentResponse::new("t")
        .section(symbols())
        .section(Section::kv("receipt", vec![KvItem::new("id", KvValue::Int(7))]))
        .section(Section::text("note", "hi"))
        .budget(BudgetSummary::new().max_tokens(8))
        .provenance(ProvenanceBlock::new().entry("analyzer", "ra").fingerprint("fp"));
    let v = json(&r);
    assert_eq!(
        v["sections"][0],
        serde_json::json!({
            "name": "brief.symbols", "kind": "toon", "fields": ["name"], "rows": [["RetryPolicy"]],
            "returnedCount": 1, "totalCount": 40, "truncated": true
        })
    );
    assert_eq!(v["sections"][1]["kind"], "kv");
    assert_eq!(v["sections"][1]["items"], serde_json::json!([{"key": "id", "value": 7}]));
    assert_eq!(v["sections"][2]["text"], "hi");
    assert!(v["sections"][2].get("totalCount").is_none());
    // The budget carries no per-section list of its own: `sections` above is the one copy of those counts.
    assert_eq!(v["budget"], serde_json::json!({"maxTokens": 8, "truncated": true}));
    assert_eq!(v["provenance"]["entries"][0], serde_json::json!({"label": "analyzer", "value": "ra"}));
    assert_eq!(v["provenance"]["fingerprint"], "fp");
    let structured: serde_json::Value =
        serde_json::from_str(r.to_call_tool_result().structured_content.unwrap().as_str()).unwrap();
    assert_eq!(structured, v);
}

#[test]
fn structured_content_has_the_four_keys_and_gains_sections_only_with_a_section() {
    let kv = AgentResponse::new("t").kv_items(vec![KvItem::new("k", KvValue::Int(1))]);
    let keys_of = |r: &AgentResponse| {
        let v: serde_json::Value =
            serde_json::from_str(r.to_call_tool_result().structured_content.unwrap().as_str()).unwrap();
        let mut keys: Vec<String> = v.as_object().unwrap().keys().cloned().collect();
        keys.sort_unstable();
        keys
    };
    assert_eq!(keys_of(&AgentResponse::new("t")), ["body", "hints", "isError", "recovery"]);
    for r in [AgentResponse::new("t").items(vec![vec![Value::Int(1)]], &["n"]).hint("h"), kv] {
        assert_eq!(keys_of(&r), ["body", "hints", "isError", "recovery", "sections"]);
    }
}

#[test]
fn a_failed_section_still_flags_the_error_with_an_envelope() {
    let bad = Section::toon("bad", fields(&["a", "b"]), vec![vec![Value::Int(1)]]);
    let r = AgentResponse::new("t").section(bad).budget(BudgetSummary::new());
    assert!(r.is_error());
    assert!(r.to_call_tool_result().is_error);
    assert!(r.render(OutputFormat::Text).contains("error: toon_validation_failed"));
}

fn hostile() -> impl Strategy<Value = String> {
    let c = prop_oneof![
        Just(','),
        Just('"'),
        Just('\\'),
        Just('\n'),
        Just('\r'),
        Just('\t'),
        Just(':'),
        Just('['),
        Just(']'),
        Just('{'),
        Just('-'),
        Just('#'),
        Just(' '),
        Just('\u{1}'),
        Just('\u{2028}'),
        Just('é'),
        proptest::char::range('a', 'z'),
        proptest::char::range('0', '9'),
    ];
    prop_oneof![proptest::collection::vec(c, 0..24).prop_map(|c| c.into_iter().collect::<String>()), "(?s).{0,16}"]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn hostile_strings_across_a_whole_envelope_stay_valid_toon_and_json(
        names in proptest::collection::vec(hostile(), 1..5),
        cells in proptest::collection::vec(hostile(), 1..6),
        labels in proptest::collection::vec((hostile(), hostile()), 0..4),
        fp in proptest::option::of(hostile()),
        cap in 1usize..12,
        totals in proptest::collection::vec(proptest::option::of(0usize..100), 5),
        slot in proptest::bool::ANY,
        hint in hostile(),
    ) {
        let mut r = AgentResponse::new("root").truncate_cells_at(cap).truncation_hint("full=true");
        if slot {
            let rows = cells.iter().map(|c| vec![Value::from(c.clone())]).collect();
            r = r.section(Section::toon("root", fields(&["c"]), rows).total_count(Some(7)));
        }
        let budget = BudgetSummary::new().max_tokens(1).truncated(true);
        for (i, name) in names.iter().enumerate() {
            // The index prefix keeps names distinct and off every reserved key; the rest is hostile.
            let name = format!("s{i}{name}");
            let section = match i % 3 {
                0 => Section::toon(
                    name.clone(),
                    vec![format!("f{}", names[0]), "g".into()],
                    cells.iter().map(|c| vec![Value::from(c.clone()), Value::Int(1)]).collect(),
                ),
                1 => Section::kv(
                    name.clone(),
                    cells.iter().enumerate().map(|(j, c)| KvItem::new(format!("k{i}_{j}{c}"), KvValue::Text(c.clone()))).collect(),
                ),
                _ => Section::text(name.clone(), format!("note{i}: ok")),
            };
            r = r.section(section.total_count(totals[i % totals.len()]));
        }
        let mut prov = ProvenanceBlock::new();
        for (i, (l, v)) in labels.iter().enumerate() {
            prov = prov.entry(format!("l{i}{l}"), v.clone());
        }
        if let Some(fp) = fp {
            prov = prov.fingerprint(fp);
        }
        let r = r.budget(budget).provenance(prov).hint(hint).recovery_hint(RecoveryHint::new("search"));

        let text = r.render(OutputFormat::Text);
        prop_assert!(decode(&text).is_ok(), "invalid TOON: {:?}\n{text}", decode(&text).err());
        let v = json(&r);
        prop_assert_eq!(v["sections"].as_array().map(Vec::len), Some(names.len() + usize::from(slot)));
        prop_assert_eq!(v["body"].as_str().map(|b| text.starts_with(b)), Some(true));
        // `truncated` is never a separate claim: it is exactly "the total exceeds the count", in every section.
        for s in v["sections"].as_array().into_iter().flatten() {
            let cut = s["totalCount"].as_u64().is_some_and(|t| Some(t) > s["returnedCount"].as_u64());
            prop_assert_eq!(s["truncated"].as_bool(), Some(cut), "{}", s);
        }
    }
}
