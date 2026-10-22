//! List-level truncation: `TruncationOutcome` reports what a list cap cut, whichever strategy decided it
//! (docs/spec/06-decisions.md, "`TruncationOutcome` reports what a list cap cut").

use michi_core::response::{AgentResponse, OutputFormat};
use michi_core::toon::Value;
use michi_core::{truncate_by_count, truncate_by_size, Hint, TruncationOutcome};

mod support;
use support::toon_parser::{decode, Node, Prim};

// --- nothing cut -----------------------------------------------------------------------------------------------------

#[test]
fn empty_input_cuts_nothing() {
    let by_count = truncate_by_count(Vec::<u32>::new(), 5);
    let by_size = truncate_by_size(Vec::<u32>::new(), 5, |_| 1);
    for outcome in [by_count, by_size] {
        assert!(outcome.kept.is_empty());
        assert_eq!(outcome.total_before, 0);
        assert!(!outcome.truncated());
        assert_eq!(outcome.omitted(), 0);
        assert_eq!(outcome.hint("pass --all"), None);
    }
}

#[test]
fn empty_input_with_a_zero_cap_cuts_nothing() {
    let outcome = truncate_by_count(Vec::<u32>::new(), 0);
    assert!(!outcome.truncated());
    assert_eq!(outcome.hint("pass --all"), None);
    let outcome = truncate_by_size(Vec::<u32>::new(), 0, |_| 1);
    assert!(!outcome.truncated());
    assert_eq!(outcome.hint("pass --all"), None);
}

#[test]
fn a_zero_cap_on_items_keeps_none_and_reports_all_omitted() {
    let outcome = truncate_by_count(vec![1, 2, 3], 0);
    assert!(outcome.kept.is_empty());
    assert_eq!(outcome.total_before, 3);
    assert!(outcome.truncated());
    assert_eq!(outcome.omitted(), 3);
    assert_eq!(outcome.hint("pass --all"), Some(Hint::new("showing 0 of 3; pass --all")));
}

#[test]
fn an_exact_fit_cuts_nothing() {
    let by_count = truncate_by_count(vec![1, 2, 3], 3);
    assert_eq!(by_count.kept, vec![1, 2, 3]);
    assert!(!by_count.truncated());
    assert_eq!(by_count.hint("pass --all"), None);

    let by_size = truncate_by_size(vec![2, 3, 5], 10, |n| *n);
    assert_eq!(by_size.kept, vec![2, 3, 5]);
    assert!(!by_size.truncated());
    assert_eq!(by_size.hint("pass --all"), None);
}

// --- strategies ------------------------------------------------------------------------------------------------------

#[test]
fn by_count_keeps_the_first_n_in_order() {
    let outcome = truncate_by_count(vec!["e", "d", "c", "b", "a"], 2);
    assert_eq!(outcome.kept, vec!["e", "d"]);
    assert_eq!(outcome.total_before, 5);
    assert_eq!(outcome.omitted(), 3);
    assert!(outcome.truncated());
}

#[test]
fn by_count_with_a_cap_above_the_length_keeps_everything() {
    let outcome = truncate_by_count(vec![1, 2], 10);
    assert_eq!(outcome.kept, vec![1, 2]);
    assert_eq!(outcome.total_before, 2);
    assert!(!outcome.truncated());
}

#[test]
fn by_size_stops_at_the_first_item_that_does_not_fit() {
    // 3 + 4 = 7 fits in 8; 7 + 5 does not, so the walk stops there even though the trailing 1 would fit.
    let outcome = truncate_by_size(vec![3, 4, 5, 1], 8, |n| *n);
    assert_eq!(outcome.kept, vec![3, 4]);
    assert_eq!(outcome.total_before, 4);
    assert_eq!(outcome.omitted(), 2);
    assert!(outcome.truncated());
}

#[test]
fn by_size_with_an_oversized_first_item_keeps_nothing() {
    let outcome = truncate_by_size(vec![100, 1, 1], 10, |n| *n);
    assert!(outcome.kept.is_empty());
    assert_eq!(outcome.total_before, 3);
    assert!(outcome.truncated());
    assert_eq!(outcome.hint("raise --max-tokens"), Some(Hint::new("showing 0 of 3; raise --max-tokens")));
}

#[test]
fn by_size_zero_sized_items_always_fit() {
    let outcome = truncate_by_size(vec!["a", "b"], 0, |_| 0);
    assert_eq!(outcome.kept, vec!["a", "b"]);
    assert!(!outcome.truncated());
}

#[test]
fn by_size_accumulation_never_overflows() {
    // A sum past usize::MAX must not wrap around to a small number and look like it fits.
    let outcome = truncate_by_size(vec![1u8, 2, 3], usize::MAX, |_| usize::MAX);
    assert_eq!(outcome.kept, vec![1], "one usize::MAX item fits a usize::MAX budget, a second does not");
    assert_eq!(outcome.omitted(), 2);

    let outcome = truncate_by_size(vec![1u8, 2], usize::MAX - 1, |_| usize::MAX);
    assert!(outcome.kept.is_empty());

    let outcome = truncate_by_size(vec![1u8, 2, 3], usize::MAX, |n| if *n == 1 { 1 } else { usize::MAX });
    assert_eq!(outcome.kept, vec![1], "1 + usize::MAX overflows, so the second item does not fit");
}

// --- hint ------------------------------------------------------------------------------------------------------------

#[test]
fn hint_wording_is_exact() {
    let outcome = truncate_by_count((0..120).collect::<Vec<_>>(), 50);
    assert_eq!(
        outcome.hint("pass --limit 120 to see all"),
        Some(Hint::new("showing 50 of 120; pass --limit 120 to see all"))
    );
    assert_eq!(outcome.hint(String::from("use offset=50")), Some(Hint::new("showing 50 of 120; use offset=50")));
}

#[test]
fn an_empty_guidance_drops_the_clause() {
    let outcome = truncate_by_count(vec![1, 2, 3], 1);
    assert_eq!(outcome.hint(""), Some(Hint::new("showing 1 of 3")));
}

#[test]
fn a_new_built_outcome_renders_the_same_hint_as_the_built_ins() {
    let built_in = truncate_by_count(vec!['a', 'b', 'c', 'd'], 2);
    // A caller's own heuristic (here: keep the last two) wraps its result in the same contract.
    let custom = TruncationOutcome::new(vec!['c', 'd'], 4);
    assert_eq!(custom.hint("pass --all"), built_in.hint("pass --all"));
    assert_eq!(custom.hint("pass --all"), Some(Hint::new("showing 2 of 4; pass --all")));

    let by_size = truncate_by_size(vec![1, 1, 1, 1], 2, |n| *n);
    assert_eq!(custom.hint("x"), by_size.hint("x"));
}

#[test]
fn new_normalizes_a_total_below_the_kept_count() {
    // §6: infallible constructors normalize. A total smaller than what was kept is impossible, so it is raised.
    let outcome = TruncationOutcome::new(vec![1, 2, 3], 1);
    assert_eq!(outcome.total_before, 3);
    assert!(!outcome.truncated());
    assert_eq!(outcome.omitted(), 0);
    assert_eq!(outcome.hint("pass --all"), None);

    let outcome = TruncationOutcome::new(vec![1, 2], 7);
    assert_eq!(outcome.total_before, 7);
    assert_eq!(outcome.omitted(), 5);
}

#[test]
fn outcomes_compare_and_clone_by_value() {
    let a = truncate_by_count(vec![1, 2, 3], 2);
    assert_eq!(a.clone(), TruncationOutcome::new(vec![1, 2], 3));
    assert_ne!(a, TruncationOutcome::new(vec![1, 2], 4));
    assert_eq!(format!("{a:?}"), "TruncationOutcome { kept: [1, 2], total_before: 3 }");
}

// --- AgentResponse ---------------------------------------------------------------------------------------------------

fn rows(kept: &[i64]) -> Vec<Vec<Value>> {
    kept.iter().map(|n| vec![Value::Int(*n)]).collect()
}

#[test]
fn agent_response_truncation_sets_the_count_and_hint_together() {
    let outcome = truncate_by_count(vec![1, 2, 3, 4, 5], 2);
    let r = AgentResponse::new("issues")
        .items(rows(&outcome.kept), &["n"])
        .truncation(&outcome, "pass --limit 5")
        .hint("call get_issue for details");
    let out = r.render(OutputFormat::Text);
    assert_eq!(
        out,
        "issues[2]{n}:\n  1\n  2\ntotalCount: 5\nhelp[2]: showing 2 of 5; pass --limit 5,call get_issue for details"
    );
    let doc = decode(&out).unwrap_or_else(|e| panic!("not conformant TOON 4.4: {e}\n{out}"));
    assert_eq!(doc.get("totalCount"), Some(&Node::Prim(Prim::Num("5".into()))));
    assert_eq!(
        doc.get("help"),
        Some(&Node::Inline(vec![Prim::str("showing 2 of 5; pass --limit 5"), Prim::str("call get_issue for details")]))
    );
    let json = r.render(OutputFormat::Json);
    assert!(json.contains("\"hints\":[\"showing 2 of 5; pass --limit 5\",\"call get_issue for details\"]"), "{json}");
    assert!(json.contains("totalCount: 5"), "{json}");
}

#[test]
fn agent_response_truncation_without_a_cut_sets_the_count_and_no_hint() {
    let outcome = truncate_by_count(vec![1, 2], 5);
    let r = AgentResponse::new("issues").items(rows(&outcome.kept), &["n"]).truncation(&outcome, "pass --limit 5");
    let out = r.render(OutputFormat::Text);
    assert_eq!(out, "issues[2]{n}:\n  1\n  2\ntotalCount: 2");
    decode(&out).unwrap_or_else(|e| panic!("not conformant TOON 4.4: {e}\n{out}"));
}

#[test]
fn agent_response_truncation_with_nothing_kept_is_conformant() {
    let outcome = truncate_by_size(vec!["huge".to_string()], 1, String::len);
    let r = AgentResponse::new("blocks").items(vec![], &["text"]).truncation(&outcome, "raise --budget");
    let out = r.render(OutputFormat::Text);
    assert_eq!(out, "blocks: []\ntotalCount: 1\nhelp[1]: showing 0 of 1; raise --budget");
    decode(&out).unwrap_or_else(|e| panic!("not conformant TOON 4.4: {e}\n{out}"));
}
