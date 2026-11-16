//! A run record composes into one `AgentResponse` with its error: the step table and the failure are one document
//! (michi-core docs/spec/06-decisions.md, "Outcomes compose into one response"). Before, `RunRecord::render` wrote a
//! standalone `step[N]` document and `DomainError` another, and the two could not be joined into valid TOON.

mod common;

#[path = "../../michi-core/tests/support/toon_parser.rs"]
mod toon_parser;

use common::*;
use michi_core::{AgentResponse, DomainError, OutputFormat};
use michi_pipeline::{execute, ExecOptions, RunOutcome};

#[tokio::test(start_paused = true)]
async fn a_failed_run_and_its_error_render_as_one_document() {
    let log = Log::new();
    let p = plan(vec![
        act(&log, "fetch", &[], Act::Ok),
        act(&log, "upload", &["fetch"], Act::Err(fatal("bucket is read-only"))),
        act(&log, "notify", &["upload"], Act::Ok),
    ]);
    let record = execute(&p, rid("run-7"), &ExecOptions::new()).await;
    assert_ne!(record.outcome(), RunOutcome::Succeeded);
    let error: DomainError = record.error().expect("the run failed").into();

    let r = AgentResponse::new("run").error(error).pipeline("step", record.to_pipeline());
    let text = r.render(OutputFormat::Text);
    let doc = toon_parser::decode(&text).unwrap_or_else(|e| panic!("not one valid document: {e}\n{text}"));
    let keys: Vec<&str> = doc.entries.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(&keys[..2], ["error", "message"], "the error leads: {text}");
    assert!(keys.contains(&"step"), "{text}");
    assert!(text.contains("\n  upload,Step upload,failed,null"), "{text}");
    assert!(text.contains("\n  notify,Step notify,skipped,prerequisite_failed"), "{text}");

    let json: serde_json::Value = serde_json::from_str(&r.render(OutputFormat::Json)).expect("one JSON object");
    assert_eq!(json["isError"], true);
    assert!(json["error"]["message"].as_str().is_some_and(|m| m.contains("upload")), "{json}");
    assert_eq!(json["sections"][0]["name"], "step");
    assert_eq!(json["sections"][0]["rows"].as_array().map(Vec::len), Some(3));
    assert!(r.to_call_tool_result().is_error);
    // The record's own render is the same table, written by the same writer.
    assert!(text.contains(&record.render()), "{text}");
}
