//! Allocation counts for the `AgentResponse` render paths.
//!
//! A count is deterministic where wall time is not, so this is the check for "does this path copy the data": rendering
//! a response must cost about what rendering the same list with `render_toon` costs, and `is_error()` (documented as
//! "validates without rendering") must not allocate per row. Before the render went through a borrowed view, each
//! validate and each render cloned every row, about two allocations per row.
//!
//! One test function holds every measurement. The counting allocator is process-wide, so a second test running on
//! another thread would add its allocations to the count.

use std::alloc::System;

use michi_core::toon::{render_toon, ToonOptions, Value};
use michi_core::{AgentResponse, OutputFormat, Section};
use stats_alloc::{Region, StatsAlloc, INSTRUMENTED_SYSTEM};

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

const ROWS: usize = 1_000;

/// Allocations made while `f` runs.
fn allocations<T>(f: impl FnOnce() -> T) -> (usize, T) {
    let region = Region::new(GLOBAL);
    let out = f();
    (region.change().allocations, out)
}

fn fields() -> Vec<String> {
    vec!["number".into(), "title".into(), "state".into()]
}

/// Rows whose title cell is longer than a `CompactString` keeps inline (24 bytes), so each row owns heap data a clone
/// has to copy.
fn rows() -> Vec<Vec<Value>> {
    (0..ROWS)
        .map(|i| {
            vec![
                Value::Int(i as i64),
                Value::from(format!("Issue number {i}: a title long enough to live on the heap")),
                Value::from("open"),
            ]
        })
        .collect()
}

#[test]
fn rendering_a_response_does_not_copy_its_rows() {
    let opts = ToonOptions::new("issues", fields(), rows());
    let (baseline, expected) = allocations(|| render_toon(&opts).expect("valid options"));

    let response = AgentResponse::new("issues").section(Section::toon("issues", fields(), rows()));

    // The composer adds a handful of buffers of its own (parts, joined body, flags); it must not add one per row.
    let slack = 40;

    let (text, rendered) = allocations(|| response.render(OutputFormat::Text));
    assert!(rendered.starts_with(&expected), "the response renders the same list");
    assert!(text <= baseline + slack, "render: {text} allocations for {ROWS} rows, render_toon needs {baseline}");

    let (validate, is_error) = allocations(|| response.is_error());
    assert!(!is_error);
    assert!(validate <= slack, "is_error: {validate} allocations for {ROWS} rows; validation must not copy them");

    let (both, _) = allocations(|| {
        let _ = response.is_error();
        response.to_call_tool_result()
    });
    assert!(
        both <= 2 * baseline + 2 * slack,
        "is_error + to_call_tool_result: {both} allocations for {ROWS} rows, render_toon needs {baseline}"
    );
}
