//! Every outcome michi renders composes into one `AgentResponse` (docs/spec/06-decisions.md, "Outcomes compose into
//! one response"): `DomainError`, `PartialSuccess`, `StatusResponse`, `Pipeline` and `already_done` become sections or
//! the error block of the response, and render through its one writer on every surface. Each probe below was
//! impossible before: the type had its own document, and two documents could not be joined into one valid one.

use michi_core::kv::{KvItem, KvValue};
use michi_core::pipeline::{Pipeline, PipelineStep, SkipReason, StepStatus};
use michi_core::toon::Value;
use michi_core::{
    AgentResponse, Audience, BudgetSummary, CompletedOp, DomainError, ErrorCode, FailedOp, Health, OutputFormat,
    PartialSuccess, ProvenanceBlock, RecoveryHint, Section, SkippedOp, StatusItem, StatusResponse,
};
use proptest::prelude::*;

mod support;
use support::toon_parser::{decode, Doc, Node, Prim};

fn valid(text: &str) -> Doc {
    decode(text).unwrap_or_else(|e| panic!("not conformant TOON: {e}\n{text}"))
}

fn json(r: &AgentResponse) -> serde_json::Value {
    let text = r.render(OutputFormat::Json);
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("invalid JSON: {e}\n{text}"))
}

fn keys(v: &serde_json::Value) -> Vec<&str> {
    v.as_object().map(|o| o.keys().map(String::as_str).collect()).unwrap_or_default()
}

fn fields(names: &[&str]) -> Vec<String> {
    names.iter().map(|s| (*s).to_string()).collect()
}

/// The spec 07 section 4 batch: four ops, two answered, one failed with a recovery, one skipped.
fn batch_ops() -> PartialSuccess {
    PartialSuccess::try_new(
        vec![CompletedOp::new(1, "dependents"), CompletedOp::new(0, "symbols")],
        vec![FailedOp::new(2, "refs", "symbol not found: RetryPolicy")
            .recovery(RecoveryHint::new("search").param("query", KvValue::Text("RetryPolicy".into())))],
        vec![SkippedOp::new(3, "search").reason("budget_exhausted")],
    )
    .expect("one outcome per index")
}

fn batch() -> AgentResponse {
    AgentResponse::new("batch")
        .section(Section::toon("symbols", fields(&["name", "kind"]), vec![vec!["RetryPolicy".into(), "struct".into()]]))
        .section(Section::toon("dependents", fields(&["name"]), vec![vec!["Client".into()]]).total_count(Some(3)))
        .partial_success("ops", batch_ops())
        .budget(BudgetSummary::new().tokens_used(812).max_tokens(1000))
        .provenance(ProvenanceBlock::new().entry("analyzer", "rust-analyzer").fingerprint("abc123"))
        .hint("narrow the search query")
}

#[test]
fn the_spec_07_batch_is_one_valid_toon_document() {
    let text = batch().render(OutputFormat::Text);
    assert_eq!(
        text,
        "symbols[1]{name,kind}:\n  RetryPolicy,struct\n\
         dependents[1]{name}:\n  Client\ntotalCount: 3\n\
         ops[4]{index,operation,status,reason}:\n  0,symbols,completed,null\n  1,dependents,completed,null\n  \
         2,refs,failed,\"symbol not found: RetryPolicy\"\n  3,search,skipped,budget_exhausted\n\
         budget:\n  tokensUsed: 812\n  maxTokens: 1000\n  truncated: true\n\
         sections[3]{name,returned,total,truncated}:\n  symbols,1,null,false\n  dependents,1,3,true\n  \
         ops,4,null,false\n\
         provenance:\n  analyzer: rust-analyzer\n  fingerprint: abc123\n\
         help[1]: narrow the search query\n\
         recovery[1]{tool,params,reason}:\n  search,\"{\\\"query\\\":\\\"RetryPolicy\\\"}\",null"
    );
    let doc = valid(&text);
    // `recovery` appears once, at the root, carrying the failed op's hint: it is not a key of the ops block.
    assert_eq!(doc.entries.iter().filter(|(k, _)| k == "recovery").count(), 1);
    let Some(Node::Table { rows, .. }) = doc.get("ops") else { panic!("ops is a table: {doc:?}") };
    let indices: Vec<&Prim> = rows.iter().map(|r| &r[0]).collect();
    assert_eq!(
        indices,
        [&Prim::Num("0".into()), &Prim::Num("1".into()), &Prim::Num("2".into()), &Prim::Num("3".into())]
    );
}

#[test]
fn the_spec_07_batch_is_one_json_object_with_the_ops_as_a_section() {
    let r = batch();
    let v = json(&r);
    assert_eq!(keys(&v), ["body", "sections", "budget", "provenance", "hints", "recovery", "isError"]);
    assert_eq!(v["isError"], false, "a partial batch is not a failed tool call");
    let ops = &v["sections"][2];
    assert_eq!(ops["name"], "ops");
    assert_eq!(ops["kind"], "toon");
    assert_eq!(ops["fields"], serde_json::json!(["index", "operation", "status", "reason"]));
    assert_eq!(ops["rows"][2], serde_json::json!([2, "refs", "failed", "symbol not found: RetryPolicy"]));
    assert_eq!(ops["rows"][0], serde_json::json!([0, "symbols", "completed", null]));
    assert_eq!(v["recovery"], serde_json::json!([{"tool": "search", "params": {"query": "RetryPolicy"}}]));
    let result = r.to_call_tool_result();
    assert_eq!(result.content[0].text, r.render(OutputFormat::Text));
    let structured: serde_json::Value =
        serde_json::from_str(result.structured_content.as_ref().map(michi_core::JsonObject::as_str).unwrap_or(""))
            .expect("JSON");
    assert_eq!(structured, v);
    assert_eq!(r.render_for(Audience::Assistant), r.render(OutputFormat::Text));
}

// ---- DomainError: one error shape ----

fn timeout() -> DomainError {
    DomainError::new(ErrorCode::Timeout, "POST /charge timed out")
        .cause("upstream closed the connection")
        .recovery(RecoveryHint::new("get_charge").param("id", KvValue::Text("ch_1".into())))
}

#[test]
fn a_domain_error_composes_with_sections_into_one_document() {
    let r = AgentResponse::new("charge")
        .kv_items(vec![KvItem::new("attempted", KvValue::Int(1))])
        .error(timeout())
        .hint("check the dashboard");
    assert!(r.is_error());
    let text = r.render(OutputFormat::Text);
    assert_eq!(
        text,
        "error: timeout\nmessage: POST /charge timed out\ncauses[1]: upstream closed the connection\nexit_code: 1\n\
         attempted: 1\n\
         help[2]: \"the operation may have taken effect; verify whether it completed before retrying, unless it is \
         idempotent\",check the dashboard\n\
         recovery[1]{tool,params,reason}:\n  get_charge,\"{\\\"id\\\":\\\"ch_1\\\"}\",null"
    );
    valid(&text);
    let v = json(&r);
    assert_eq!(keys(&v), ["body", "error", "sections", "hints", "recovery", "isError"]);
    assert_eq!(
        v["error"],
        serde_json::json!({
            "code": "timeout",
            "message": "POST /charge timed out",
            "causes": ["upstream closed the connection"],
            "retryable": false
        })
    );
    assert_eq!(v["isError"], true);
    assert!(r.to_call_tool_result().is_error);
}

#[test]
fn a_domain_error_renders_the_same_alone_and_in_a_response() {
    for e in [timeout(), DomainError::new(ErrorCode::NotFound, "no such issue").hint("list issues first")] {
        let alone = AgentResponse::new("anything").error(e.clone());
        for format in [OutputFormat::Text, OutputFormat::Json] {
            assert_eq!(e.render(format), alone.render(format), "{format:?}");
        }
        assert_eq!(e.to_call_tool_result(), alone.to_call_tool_result());
    }
}

#[test]
fn there_is_one_error_json_shape() {
    // A DomainError and an error response carry the same keys, and `recovery` is an array in both.
    let from_error: serde_json::Value =
        serde_json::from_str(&timeout().render(OutputFormat::Json)).expect("DomainError JSON");
    let marked: serde_json::Value = serde_json::from_str(
        &AgentResponse::new("t").as_error().recovery_hint(RecoveryHint::new("x")).render(OutputFormat::Json),
    )
    .expect("response JSON");
    assert_eq!(keys(&from_error), ["body", "error", "hints", "recovery", "isError"]);
    assert_eq!(keys(&marked), ["body", "hints", "recovery", "isError"]);
    assert!(from_error["recovery"].is_array() && marked["recovery"].is_array());
    assert_eq!(keys(&from_error["error"]), ["code", "message", "causes", "retryable"]);
    // `body` is the document before `help` and `recovery`, as in every response's JSON.
    let text = timeout().render(OutputFormat::Text);
    let body = from_error["body"].as_str().expect("body is a string");
    assert!(text.starts_with(body) && body.ends_with("exit_code: 1"), "{body}");
}

#[test]
fn a_second_error_replaces_the_first() {
    let r = AgentResponse::new("t")
        .error(DomainError::new(ErrorCode::NotFound, "first"))
        .error(DomainError::new(ErrorCode::Conflict, "second"));
    assert_eq!(r.render(OutputFormat::Text), "error: conflict\nmessage: second\nexit_code: 1");
}

#[test]
fn a_domain_error_and_a_failed_section_keep_the_error_and_add_the_failure_as_a_cause() {
    // Before: the two error blocks collided on `error`, so the text read `toon_validation_failed` ("duplicate root
    // key 'error'", a key the caller never used) while the JSON still said `not_found`. See tests/response_consistency.rs.
    let bad = Section::toon("t", fields(&["a"]), vec![vec![Value::Int(1), Value::Int(2)]]);
    let r = AgentResponse::new("t").section(bad).error(DomainError::new(ErrorCode::NotFound, "gone"));
    let text = r.render(OutputFormat::Text);
    assert_eq!(
        text,
        "error: not_found\nmessage: gone\ncauses[1]: \"toon_validation_failed: row 0 has 2 values but 1 fields \
         declared\"\nexit_code: 1"
    );
    valid(&text);
    assert!(r.is_error());
    assert_eq!(json(&r)["error"]["code"], "not_found");
}

// ---- PartialSuccess ----

#[test]
fn partial_success_composes_without_a_root_recovery_of_its_own() {
    // Before: `PartialSuccess::render` wrote its own `recovery[N]` table, a root key `AgentResponse` writes too, so
    // the two could not be one document.
    let r = AgentResponse::new("t").partial_success("ops", batch_ops()).recovery_hint(RecoveryHint::new("retry_all"));
    let text = r.render(OutputFormat::Text);
    let doc = valid(&text);
    let Some(Node::Table { rows, .. }) = doc.get("recovery") else { panic!("one recovery table: {text}") };
    assert_eq!(rows.len(), 2, "the failed op's hint, then the response's own: {text}");
    assert_eq!(rows[0][0], Prim::str("search"));
    assert_eq!(rows[1][0], Prim::str("retry_all"));
}

#[test]
fn partial_success_standalone_render_is_the_response_render() {
    let ps = batch_ops();
    assert_eq!(
        ps.render(),
        AgentResponse::new("partial_success")
            .truncate_cells_at(usize::MAX)
            .partial_success("partial_success", ps.clone())
            .render(OutputFormat::Text)
    );
    valid(&ps.render());
    assert_eq!(PartialSuccess::default().render(), "partial_success: []\ntotalCount: 0");
}

// ---- StatusResponse ----

fn status() -> StatusResponse {
    StatusResponse::new(
        "my-tool",
        "does things",
        vec![
            StatusItem::new("index", KvValue::Text("ready".into())).health(Health::Ok),
            StatusItem::new("cache", KvValue::Float(0.256, 2)).health(Health::Degraded("near limit".into())),
            StatusItem::new("uptime", KvValue::Duration(std::time::Duration::from_millis(1500))),
        ],
    )
    .hints(vec!["call get_status for detail".into()])
}

#[test]
fn a_status_response_composes_into_a_response() {
    let r = AgentResponse::new("t").status(status()).section(Section::text("note", "all good"));
    let text = r.render(OutputFormat::Text);
    assert_eq!(
        text,
        "tool: my-tool\ndescription: does things\nitems[3]{key,value,health,reason}:\n  index,ready,ok,null\n  \
         cache,0.26,degraded,near limit\n  uptime,1.5s,null,null\nnote: all good\nhelp[1]: call get_status for detail"
    );
    valid(&text);
    let v = json(&r);
    assert_eq!(v["sections"][1]["rows"][1], serde_json::json!(["cache", 0.26, "degraded", "near limit"]));
    assert_eq!(
        status().render(),
        "tool: my-tool\ndescription: does things\nitems[3]{key,value,health,reason}:\n  \
         index,ready,ok,null\n  cache,0.26,degraded,near limit\n  uptime,1.5s,null,null\n\
         help[1]: call get_status for detail"
    );
}

// ---- Pipeline ----

fn pipeline() -> Pipeline {
    Pipeline::new(vec![
        PipelineStep::new("fetch", "Fetch, then parse", StepStatus::Completed),
        PipelineStep::new("upload", "Upload", StepStatus::Skipped(SkipReason::PrerequisiteFailed)),
    ])
}

#[test]
fn a_pipeline_composes_into_a_response() {
    let r = AgentResponse::new("run")
        .pipeline("step", pipeline())
        .error(DomainError::new(ErrorCode::ExternalFailure, "upload failed"));
    let text = r.render(OutputFormat::Text);
    assert_eq!(
        text,
        "error: external_failure\nmessage: upload failed\nexit_code: 1\n\
         step[2]{id,name,status,reason}:\n  fetch,\"Fetch, then parse\",completed,null\n  \
         upload,Upload,skipped,prerequisite_failed"
    );
    valid(&text);
    assert_eq!(
        json(&r)["sections"][0]["rows"][1],
        serde_json::json!(["upload", "Upload", "skipped", "prerequisite_failed"])
    );
    assert_eq!(
        pipeline().render(),
        "step[2]{id,name,status,reason}:\n  fetch,\"Fetch, then parse\",completed,null\n  \
         upload,Upload,skipped,prerequisite_failed"
    );
}

// ---- already_done ----

#[test]
fn already_done_composes_into_a_response() {
    let r = AgentResponse::new("t").already_done("delete_item", "deleted at 12:00").hint("nothing to do");
    let text = r.render(OutputFormat::Text);
    assert_eq!(
        text,
        "operation: delete_item\nstatus: already_done\nsummary: \"deleted at 12:00\"\nhelp[1]: nothing to do"
    );
    assert_eq!(text, michi_core::render_already_done("delete_item", "deleted at 12:00", &["nothing to do".into()]));
    valid(&text);
    assert_eq!(json(&r)["sections"][0]["items"][1], serde_json::json!({"key": "status", "value": "already_done"}));
}

// The "One response" example in docs/spec/02-toon-format.md, verbatim.
#[test]
fn the_spec_02_one_response_example_renders_exactly() {
    let ops = PartialSuccess::try_new(
        vec![CompletedOp::new(0, "symbols")],
        vec![FailedOp::new(1, "refs", "not indexed").recovery(RecoveryHint::new("index_workspace"))],
        vec![SkippedOp::new(2, "search").reason("budget_exhausted")],
    )
    .expect("one outcome per index");
    let r = AgentResponse::new("batch")
        .error(DomainError::new(ErrorCode::ExternalFailure, "refs is not indexed"))
        .section(Section::toon("symbols", fields(&["name", "kind"]), vec![vec!["RetryPolicy".into(), "struct".into()]]))
        .partial_success("ops", ops)
        .hint("run index_workspace first");
    let text = r.render(OutputFormat::Text);
    assert_eq!(
        text,
        "error: external_failure\nmessage: refs is not indexed\nexit_code: 1\nsymbols[1]{name,kind}:\n  \
         RetryPolicy,struct\nops[3]{index,operation,status,reason}:\n  0,symbols,completed,null\n  \
         1,refs,failed,not indexed\n  2,search,skipped,budget_exhausted\nhelp[1]: run index_workspace first\n\
         recovery[1]{tool,params,reason}:\n  index_workspace,\"{}\",null"
    );
    valid(&text);
}

// ---- every outcome at once, with hostile text ----

fn hostile() -> impl Strategy<Value = String> {
    let c = prop_oneof![
        Just(','),
        Just('"'),
        Just('\\'),
        Just('\n'),
        Just('\r'),
        Just(':'),
        Just('['),
        Just('{'),
        Just('-'),
        Just(' '),
        Just('\u{1}'),
        Just('\u{2028}'),
        proptest::char::range('a', 'z'),
        proptest::char::range('0', '9'),
    ];
    proptest::collection::vec(c, 0..16).prop_map(|c| c.into_iter().collect::<String>())
}

fn arb_error() -> impl Strategy<Value = DomainError> {
    (0usize..ErrorCode::ALL.len(), hostile(), proptest::collection::vec(hostile(), 0..3), any::<bool>(), any::<bool>())
        .prop_map(|(code, message, causes, retryable, recovery)| {
            let mut e = DomainError::new(ErrorCode::ALL[code], message).retryable(retryable);
            for c in causes {
                e = e.cause(c);
            }
            if recovery {
                e = e.recovery(RecoveryHint::new("retry"));
            }
            e
        })
}

fn arb_partial() -> impl Strategy<Value = PartialSuccess> {
    proptest::collection::vec((0u8..3, hostile(), proptest::option::of(hostile()), any::<bool>()), 0..6).prop_map(
        |ops| {
            let (mut completed, mut failed, mut skipped) = (Vec::new(), Vec::new(), Vec::new());
            // Indices are scattered so the composed table sorts them.
            for (i, (kind, name, reason, recovery)) in ops.into_iter().enumerate().rev() {
                let index = i * 7;
                match kind {
                    0 => completed.push(CompletedOp::new(index, name)),
                    1 => {
                        let f = FailedOp::new(index, name, reason.unwrap_or_default());
                        failed.push(if recovery { f.recovery(RecoveryHint::new("fix")) } else { f });
                    }
                    _ => {
                        let s = SkippedOp::new(index, name);
                        skipped.push(match reason {
                            Some(r) => s.reason(r),
                            None => s,
                        });
                    }
                }
            }
            PartialSuccess::try_new(completed, failed, skipped).expect("distinct indices")
        },
    )
}

fn arb_status() -> impl Strategy<Value = StatusResponse> {
    (hostile(), hostile(), proptest::collection::vec((hostile(), hostile(), 0u8..4), 0..4), hostile()).prop_map(
        |(tool, desc, items, hint)| {
            let items = items
                .into_iter()
                .map(|(k, v, h)| {
                    let item = StatusItem::new(k, KvValue::Text(v.clone()));
                    match h {
                        0 => item,
                        1 => item.health(Health::Ok),
                        2 => item.health(Health::Degraded(v)),
                        _ => item.health(Health::Error(v)),
                    }
                })
                .collect();
            StatusResponse::new(tool, desc, items).hints(vec![hint.into()])
        },
    )
}

fn arb_pipeline() -> impl Strategy<Value = Pipeline> {
    proptest::collection::vec((hostile(), hostile(), 0usize..StepStatus::ALL.len()), 0..5).prop_map(|steps| {
        Pipeline::new(steps.into_iter().map(|(id, name, s)| PipelineStep::new(id, name, StepStatus::ALL[s])).collect())
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn every_mix_of_outcomes_is_one_valid_toon_document_and_one_json_object(
        error in proptest::option::of(arb_error()),
        partial in proptest::option::of(arb_partial()),
        status in proptest::option::of(arb_status()),
        pipeline in proptest::option::of(arb_pipeline()),
        done in proptest::option::of((hostile(), hostile())),
        rows in proptest::collection::vec(hostile(), 0..4),
        hint in hostile(),
        cap in 1usize..40,
    ) {
        let mut r = AgentResponse::new("batch").truncate_cells_at(cap);
        let rows = rows.iter().map(|c| vec![Value::from(c.clone())]).collect();
        r = r.section(Section::toon("results", fields(&["v"]), rows));
        let caller_error = error.clone();
        if let Some(e) = error {
            r = r.error(e);
        }
        if let Some(ps) = partial {
            r = r.partial_success("ops", ps);
        }
        if let Some(s) = status {
            r = r.status(s);
        }
        if let Some(p) = pipeline {
            r = r.pipeline("step", p);
        }
        if let Some((op, summary)) = done {
            r = r.already_done(op, summary);
        }
        let r = r.hint(hint).budget(BudgetSummary::new().tokens_used(1));

        let text = r.render(OutputFormat::Text);
        let doc = decode(&text);
        prop_assert!(doc.is_ok(), "invalid TOON: {:?}\n{}", doc.err(), text);
        let v: serde_json::Value = match serde_json::from_str(&r.render(OutputFormat::Json)) {
            Ok(v) => v,
            Err(e) => return Err(TestCaseError::fail(format!("invalid JSON: {e}"))),
        };
        prop_assert_eq!(v["isError"].as_bool(), Some(r.is_error()));
        prop_assert_eq!(r.is_error(), r.to_call_tool_result().is_error);
        // One error shape: the caller's error, or else a validation failure, is the `error` object; nothing else sets
        // `isError` here.
        prop_assert_eq!(v.get("error").is_some(), r.is_error());
        if let Some(e) = caller_error {
            // The caller's error is never hidden: it leads the text and is the JSON `error`, and a validation failure
            // can only add one cause after its own.
            prop_assert!(r.is_error());
            let code = e.code().label();
            prop_assert!(text.starts_with(&format!("error: {code}\n")), "{}", text);
            prop_assert_eq!(v["error"]["code"].as_str(), Some(code));
            prop_assert_eq!(v["error"]["message"].as_str(), Some(e.message()));
            let causes: Vec<&str> =
                v["error"]["causes"].as_array().map(|c| c.iter().filter_map(|c| c.as_str()).collect()).unwrap_or_default();
            let own: Vec<&str> = e.causes().iter().map(String::as_str).collect();
            prop_assert!(causes.starts_with(&own) && causes.len() <= own.len() + 1, "{:?}", causes);
        }
        let result = r.to_call_tool_result();
        prop_assert_eq!(result.content[0].text.as_str(), text.as_str());
        prop_assert_eq!(v["body"].as_str().map(|b| text.starts_with(b)), Some(true));
        // One `help`, one `recovery`, one `error`: TOON forbids a repeated root key, and the decoder checks it.
        let doc = doc.unwrap_or_else(|_| unreachable!());
        for key in ["help", "recovery", "error"] {
            prop_assert!(doc.entries.iter().filter(|(k, _)| k == key).count() <= 1, "{} twice in\n{}", key, text);
        }
    }
}
