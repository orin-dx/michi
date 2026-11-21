//! `AgentResponse` render paths: the text and JSON bodies, `is_error()` and the MCP result, one list of `n` rows.
//! Compare with `toon_render`, which renders the same kind of list without the response around it.

use divan::Bencher;
use michi_core::toon::Value;
use michi_core::{AgentResponse, OutputFormat, Section};

fn main() {
    divan::main();
}

fn make_response(n: usize) -> AgentResponse {
    let rows = (0..n)
        .map(|i| vec![Value::Int(i as i64), Value::from(format!("Issue title number {i}")), Value::from("open")])
        .collect();
    let issues =
        Section::toon("issue", vec!["number".into(), "title".into(), "state".into()], rows).total_count(Some(n * 2));
    AgentResponse::new("issue").section(issues).hint("Call get_issue with number=<number>")
}

#[divan::bench(args = [10, 100, 1000])]
fn render_text(b: Bencher, n: usize) {
    let r = make_response(n);
    b.bench(|| r.render(OutputFormat::Text));
}

#[divan::bench(args = [10, 100, 1000])]
fn render_json(b: Bencher, n: usize) {
    let r = make_response(n);
    b.bench(|| r.render(OutputFormat::Json));
}

#[divan::bench(args = [10, 100, 1000])]
fn is_error(b: Bencher, n: usize) {
    let r = make_response(n);
    b.bench(|| r.is_error());
}

#[divan::bench(args = [10, 100, 1000])]
fn to_call_tool_result(b: Bencher, n: usize) {
    let r = make_response(n);
    b.bench(|| r.to_call_tool_result());
}
