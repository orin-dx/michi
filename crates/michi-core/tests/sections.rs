//! Repeatable sections: several blocks in one response (spec 07, slice 1).

use michi_core::kv::{KvItem, KvValue};
use michi_core::toon::Value;
use michi_core::{AgentResponse, Audience, OutputFormat, RecoveryHint, Section, SectionBody};

fn fields(names: &[&str]) -> Vec<String> {
    names.iter().map(|s| (*s).to_string()).collect()
}

fn kv(key: &str, n: i64) -> KvItem {
    KvItem::new(key, KvValue::Int(n))
}

fn symbols() -> Section {
    Section::toon(
        "brief.symbols",
        fields(&["name", "kind"]),
        vec![
            vec![Value::from("RetryPolicy"), Value::from("struct")],
            vec![Value::from("Backoff"), Value::from("enum")],
        ],
    )
    .total_count(Some(40))
}

fn refs() -> Section {
    Section::toon("brief.refs", fields(&["symbol", "line"]), vec![vec![Value::from("RetryPolicy"), Value::Int(88)]])
}

const REFS_TEXT: &str = "brief.refs[1]{symbol,line}:\n  RetryPolicy,88";

#[test]
fn two_sections_render_as_the_spec_example() {
    let r = AgentResponse::new("brief").section(symbols()).section(refs());
    assert_eq!(
        r.render(OutputFormat::Text),
        "brief.symbols[2]{name,kind}:\n  RetryPolicy,struct\n  Backoff,enum\ntotalCount: 40\n\
         brief.refs[1]{symbol,line}:\n  RetryPolicy,88"
    );
}

#[test]
fn order_is_insertion_order() {
    let symbols_first = AgentResponse::new("t").section(symbols()).section(refs()).render(OutputFormat::Text);
    let refs_first = AgentResponse::new("t").section(refs()).section(symbols()).render(OutputFormat::Text);
    assert!(symbols_first.find("brief.symbols").unwrap() < symbols_first.find("brief.refs").unwrap());
    assert!(refs_first.find("brief.refs").unwrap() < refs_first.find("brief.symbols").unwrap());
}

#[test]
fn mixed_toon_kv_and_text() {
    let r = AgentResponse::new("t")
        .section(refs())
        .section(Section::kv("receipt", vec![kv("id", 7), kv("n", 2)]))
        .section(Section::text("note", "free prose\nsecond line"));
    assert_eq!(r.render(OutputFormat::Text), format!("{REFS_TEXT}\nid: 7\nn: 2\nfree prose\nsecond line"));
}

#[test]
fn total_count_renders_only_when_it_exceeds_the_returned_count() {
    for total in [1, 0] {
        let r = AgentResponse::new("t").section(refs().total_count(Some(total)));
        assert_eq!(r.render(OutputFormat::Text), REFS_TEXT, "total {total}");
    }
    let more = AgentResponse::new("t").section(refs().total_count(Some(2)));
    assert_eq!(more.render(OutputFormat::Text), format!("{REFS_TEXT}\ntotalCount: 2"));
    let kv_more = Section::kv("k", vec![kv("a", 1)]).total_count(Some(5));
    assert_eq!(AgentResponse::new("t").section(kv_more).render(OutputFormat::Text), "a: 1\ntotalCount: 5");
}

#[test]
fn returned_count_is_derived_for_toon_and_kv_and_declared_for_text() {
    assert_eq!(symbols().returned_count, 2);
    assert_eq!(symbols().returned_count(99).count(), 2, "setter is ignored for Toon");
    assert_eq!(Section::kv("k", vec![kv("a", 1)]).returned_count(9).count(), 1);
    assert_eq!(Section::text("t", "x").count(), 1);
    assert_eq!(Section::text("t", "").count(), 0);
    assert_eq!(Section::text("t", "a\nb\nc").returned_count(3).count(), 3);
    // A hand-edited field cannot disagree with the body: neither 0 nor 99 changes what renders.
    for bad in [0, 99] {
        let mut s = refs().total_count(Some(1));
        s.returned_count = bad;
        assert_eq!(s.count(), 1);
        assert_eq!(AgentResponse::new("t").section(s).render(OutputFormat::Text), REFS_TEXT, "bad {bad}");
    }
}

#[test]
fn a_section_with_no_rows_renders_an_empty_list_and_keeps_its_total() {
    let empty = Section::toon("hits", fields(&["a"]), vec![]);
    let r = AgentResponse::new("t").section(empty.clone()).section(refs());
    assert_eq!(r.render(OutputFormat::Text), format!("hits: []\n{REFS_TEXT}"));
    let r = AgentResponse::new("t").section(empty.total_count(Some(3)));
    assert_eq!(r.render(OutputFormat::Text), "hits: []\ntotalCount: 3");
    let r = AgentResponse::new("t").section(Section::kv("k", vec![])).section(refs());
    assert_eq!(r.render(OutputFormat::Text), REFS_TEXT);
}

#[test]
fn hints_and_recovery_follow_the_sections() {
    let r = AgentResponse::new("t")
        .section(symbols())
        .section(refs())
        .hint("narrow with --kind")
        .recovery_hint(RecoveryHint::new("search").reason("more rows"));
    let out = r.render(OutputFormat::Text);
    let help = out.find("help[").expect("help block");
    let recovery = out.find("recovery[").expect("recovery block");
    assert!(out.find("brief.refs").unwrap() < help && help < recovery, "{out}");
    assert!(out.starts_with("brief.symbols[2]"), "{out}");
}

#[test]
fn cell_cap_applies_to_every_toon_section_and_hint_lines_merge_into_one_help() {
    let long = "x".repeat(50);
    let a = Section::toon("a", fields(&["c"]), vec![vec![Value::from(long.clone())]]);
    let b = Section::toon("b", fields(&["c"]), vec![vec![Value::from(long)]]);
    let out = AgentResponse::new("t")
        .section(a)
        .section(b)
        .truncate_cells_at(10)
        .truncation_hint("full=true")
        .hint("mine")
        .render(OutputFormat::Text);
    assert_eq!(out.matches("xxxxxxxxx…").count(), 2, "{out}");
    assert_eq!(out.matches("help[").count(), 1, "{out}");
    assert!(out.contains("mine") && out.contains("full=true"), "{out}");
}

#[test]
fn invalid_section_is_an_error_on_every_surface() {
    let bad = Section::toon("t", fields(&["a", "b"]), vec![vec![Value::Int(1)]]);
    let r = AgentResponse::new("t").section(refs()).section(bad);
    assert!(r.is_error());
    assert!(r.render(OutputFormat::Text).contains("error: toon_validation_failed"));
    assert!(r.render(OutputFormat::Json).contains("\"isError\":true"));
    assert!(r.to_call_tool_result().is_error);
    assert!(!AgentResponse::new("t").section(refs()).is_error());
}

#[test]
fn one_section_matches_the_sugar_on_every_surface() {
    let rows = vec![vec![Value::Int(1), Value::from("open")]];
    let sugar = AgentResponse::new("issues").items(rows.clone(), &["id", "state"]).hint("h");
    let sect = AgentResponse::new("issues").section(Section::toon("issues", fields(&["id", "state"]), rows)).hint("h");
    assert_eq!(sect.render(OutputFormat::Text), sugar.render(OutputFormat::Text));
    assert_eq!(sect.render(OutputFormat::Json), sugar.render(OutputFormat::Json));
    assert_eq!(sect.render_for(Audience::Assistant), sugar.render_for(Audience::Assistant));
    assert_eq!(sect.to_call_tool_result().content[0].text, sugar.to_call_tool_result().content[0].text);

    let items = vec![kv("id", 1)];
    let sugar = AgentResponse::new("t").kv_items(items.clone());
    let sect = AgentResponse::new("t").section(Section::kv("t", items));
    assert_eq!(sect.render(OutputFormat::Text), sugar.render(OutputFormat::Text));
    assert_eq!(sect.render(OutputFormat::Json), sugar.render(OutputFormat::Json));
}

#[test]
fn sugar_and_sections_render_in_call_order() {
    let r = AgentResponse::new("t").items(vec![vec![Value::from("x")]], &["a"]).section(refs());
    assert_eq!(r.render(OutputFormat::Text), format!("t[1]{{a}}:\n  x\n{REFS_TEXT}"));
    let flipped = AgentResponse::new("t").section(refs()).items(vec![vec![Value::from("x")]], &["a"]);
    assert_eq!(flipped.render(OutputFormat::Text), format!("{REFS_TEXT}\nt[1]{{a}}:\n  x"));
}

#[test]
fn json_carries_the_sections_between_body_and_hints() {
    let r = AgentResponse::new("t").section(symbols()).section(refs());
    let json = r.render(OutputFormat::Json);
    assert!(json.starts_with("{\"body\":\"brief.symbols[2]{name,kind}:\\n"), "{json}");
    assert!(json.contains("\",\"sections\":[{\"name\":\"brief.symbols\",\"kind\":\"toon\""), "{json}");
    assert!(json.ends_with("],\"hints\":[],\"recovery\":[],\"isError\":false}"), "{json}");
}

#[test]
fn section_body_is_matchable() {
    assert!(matches!(symbols().body, SectionBody::Toon { .. }));
}
