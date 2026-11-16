//! Complexity-class checks: work must grow about linearly with input size.
//!
//! These catch an accidental O(n²) (a `contains` in a loop, a rescan per row), which is the regression that matters for a
//! library whose inputs come from tools and can be large. They do not track constant factors: wall time on a shared
//! runner moves by tens of percent, so a threshold on absolute time or on a percentage change would be either flaky
//! or too loose to mean anything. A tenfold input makes linear code take about 10x as long and quadratic code about
//! 100x, so a 30x bound separates them with a threefold margin for noise.

use std::hint::black_box;
use std::time::{Duration, Instant};

use michi_core::kv::{render_kv, KvItem, KvValue};
use michi_core::toon::{render_toon, ToonOptions, Value};
use michi_core::truncate::truncate;
use michi_core::truncation::truncate_by_size;

/// Input growth between the two measured sizes.
const GROWTH: u32 = 10;
/// Linear work takes ~10x as long for 10x the input; quadratic ~100x. Anything past this is not linear.
const MAX_RATIO: f64 = 30.0;
/// Each size is timed this many times and the fastest run counts, which discards scheduler and cache noise.
const RUNS: usize = 5;

fn best_of(mut f: impl FnMut()) -> Duration {
    (0..RUNS)
        .map(|_| {
            let start = Instant::now();
            f();
            start.elapsed()
        })
        .min()
        .unwrap_or_default()
}

/// Times `work(n)` at `n` and `n * GROWTH`, building the input outside the timed region.
fn assert_linear<I>(label: &str, n: usize, build: impl Fn(usize) -> I, work: impl Fn(&I)) {
    let (small, large) = (build(n), build(n * GROWTH as usize));
    let t_small = best_of(|| work(black_box(&small)));
    let t_large = best_of(|| work(black_box(&large)));
    let ratio = t_large.as_secs_f64() / t_small.as_secs_f64().max(1e-9);
    assert!(
        ratio < MAX_RATIO,
        "{label}: {GROWTH}x the input took {ratio:.1}x as long ({t_small:?} -> {t_large:?}); linear is ~{GROWTH}x"
    );
}

#[test]
fn render_toon_is_linear_in_rows() {
    assert_linear(
        "render_toon rows",
        1_000,
        |n| {
            ToonOptions::new(
                "issue",
                vec!["number".into(), "title".into(), "state".into()],
                (0..n)
                    .map(|i| vec![Value::Int(i as i64), Value::from(format!("Issue, number {i}")), Value::from("open")])
                    .collect(),
            )
        },
        |opts| {
            black_box(render_toon(opts).expect("valid options"));
        },
    );
}

#[test]
fn render_toon_is_linear_in_cell_length() {
    assert_linear(
        "render_toon long cell",
        10_000,
        |n| ToonOptions::new("t", vec!["body".into()], vec![vec![Value::from("a, \"b\"\n".repeat(n))]]),
        |opts| {
            black_box(render_toon(opts).expect("valid options"));
        },
    );
}

#[test]
fn render_kv_is_linear_in_items() {
    assert_linear(
        "render_kv items",
        1_000,
        |n| (0..n).map(|i| KvItem::new(format!("key{i}"), KvValue::Text(format!("value {i}")))).collect::<Vec<_>>(),
        |items| {
            black_box(render_kv(items).expect("valid items"));
        },
    );
}

#[test]
fn truncate_is_linear_in_string_length() {
    assert_linear(
        "truncate long text",
        20_000,
        |n| "é".repeat(n),
        |s| {
            black_box(truncate(s, s.chars().count() / 2));
        },
    );
}

#[test]
fn truncate_by_size_is_linear_in_items() {
    assert_linear(
        "truncate_by_size items",
        20_000,
        |n| (0..n).collect::<Vec<usize>>(),
        |items| {
            black_box(truncate_by_size(items.clone(), items.len() * 2, |i| i % 7 + 1));
        },
    );
}

#[test]
fn envelope_render_is_linear_in_section_count() {
    use michi_core::{AgentResponse, BudgetSummary, OutputFormat, Section};
    // Every section carries a cut cell and a total, so the merged `truncated` and `totalCount` tables, the budget
    // roll-up (derived from the sections) and the JSON `sections` array all grow with the section count.
    for format in [OutputFormat::Text, OutputFormat::Json] {
        assert_linear(
            "AgentResponse sections",
            300,
            |n| {
                let mut r = AgentResponse::new("t").truncate_cells_at(8).truncation_hint("full=true");
                for i in 0..n {
                    let name = format!("section.{i}");
                    let rows = vec![vec![Value::from(format!("a long cell value {i}")), Value::Int(i as i64)]];
                    r = r.section(Section::toon(name, vec!["a".into(), "b".into()], rows).total_count(Some(n)));
                }
                r.budget(BudgetSummary::new().tokens_used(1))
            },
            |r| {
                black_box(r.render(format));
            },
        );
    }
}

#[test]
fn response_render_is_linear_in_rows() {
    use michi_core::{AgentResponse, OutputFormat, Section};
    // Every row has a cell over the cap, so the list and the cut list (and so the `truncated` block) grow with the row
    // count, and the JSON `sections` array carries every row in full.
    for format in [OutputFormat::Text, OutputFormat::Json] {
        assert_linear(
            "AgentResponse rows",
            1_000,
            |n| {
                let rows = (0..n)
                    .map(|i| vec![Value::Int(i as i64), Value::from(format!("a title cell that is cut, number {i}"))])
                    .collect();
                AgentResponse::new("t")
                    .truncate_cells_at(12)
                    .truncation_hint("full=true")
                    .section(Section::toon("t", vec!["n".into(), "title".into()], rows).total_count(Some(n * 2)))
            },
            |r| {
                black_box(r.render(format));
            },
        );
    }
}

#[test]
fn response_to_call_tool_result_and_is_error_are_linear_in_rows() {
    use michi_core::{AgentResponse, Section};
    assert_linear(
        "AgentResponse::to_call_tool_result rows",
        1_000,
        |n| {
            let rows = (0..n).map(|i| vec![Value::Int(i as i64), Value::from(format!("title {i}"))]).collect();
            AgentResponse::new("t").section(Section::toon("t", vec!["n".into(), "title".into()], rows))
        },
        |r| {
            black_box(r.is_error());
            black_box(r.to_call_tool_result());
        },
    );
}

// ---- Composed outcomes: each part is built and rendered inside the timed region, so a quadratic row builder, cause
// writer or hint merge shows up, not only a quadratic render. ----

fn causes_error(n: usize) -> michi_core::DomainError {
    let mut e = michi_core::DomainError::new(michi_core::ErrorCode::ExternalFailure, "upstream failed");
    for i in 0..n {
        e = e.cause(format!("layer {i} said: connection reset, retry later"));
    }
    e
}

#[test]
fn error_causes_render_linearly() {
    use michi_core::{AgentResponse, OutputFormat};
    for format in [OutputFormat::Text, OutputFormat::Json] {
        assert_linear("DomainError causes", 1_000, causes_error, |e| {
            black_box(AgentResponse::new("t").error(e.clone()).render(format));
        });
    }
}

fn ops(n: usize) -> michi_core::PartialSuccess {
    use michi_core::{CompletedOp, FailedOp, PartialSuccess, RecoveryHint, SkippedOp};
    let (mut completed, mut failed, mut skipped) = (Vec::new(), Vec::new(), Vec::new());
    // Descending indices, so the composed table has to sort them.
    for i in (0..n).rev() {
        match i % 3 {
            0 => completed.push(CompletedOp::new(i, format!("op{i}"))),
            1 => failed.push(FailedOp::new(i, format!("op{i}"), "not indexed").recovery(RecoveryHint::new("index"))),
            _ => skipped.push(SkippedOp::new(i, format!("op{i}")).reason("budget_exhausted")),
        }
    }
    PartialSuccess::try_new(completed, failed, skipped).expect("distinct indices")
}

#[test]
fn partial_success_composes_and_renders_linearly() {
    use michi_core::{AgentResponse, OutputFormat};
    for format in [OutputFormat::Text, OutputFormat::Json] {
        assert_linear("PartialSuccess ops", 3_000, ops, |ps| {
            black_box(AgentResponse::new("t").partial_success("ops", ps.clone()).render(format));
        });
    }
}

fn status(n: usize) -> michi_core::StatusResponse {
    use michi_core::{Health, Hint, StatusItem, StatusResponse};
    let items = (0..n)
        .map(|i| {
            StatusItem::new(format!("check{i}"), KvValue::Float(i as f64 / 7.0, 2))
                .health(Health::Degraded(format!("slow {i}")))
        })
        .collect();
    StatusResponse::new("tool", "does things", items)
        .hints((0..n).map(|i| Hint::new(format!("look at check{i}"))).collect())
}

#[test]
fn status_composes_and_renders_linearly() {
    use michi_core::{AgentResponse, OutputFormat};
    for format in [OutputFormat::Text, OutputFormat::Json] {
        assert_linear("StatusResponse items", 1_000, status, |s| {
            black_box(AgentResponse::new("t").status(s.clone()).render(format));
        });
    }
}

fn pipeline(n: usize) -> michi_core::pipeline::Pipeline {
    use michi_core::pipeline::{Pipeline, PipelineStep, StepStatus};
    Pipeline::new(
        (0..n)
            .map(|i| {
                PipelineStep::new(
                    format!("step{i}"),
                    format!("Step, number {i}"),
                    StepStatus::ALL[i % StepStatus::ALL.len()],
                )
            })
            .collect(),
    )
}

#[test]
fn pipeline_composes_and_renders_linearly() {
    use michi_core::{AgentResponse, OutputFormat};
    for format in [OutputFormat::Text, OutputFormat::Json] {
        assert_linear("Pipeline steps", 1_000, pipeline, |p| {
            black_box(AgentResponse::new("t").pipeline("step", p.clone()).render(format));
        });
    }
}

#[test]
fn a_response_mixing_every_outcome_renders_linearly() {
    use michi_core::{AgentResponse, BudgetSummary, Hint, OutputFormat, RecoveryHint, Section};
    // Every part grows with `n`: the error's causes, the batch, the status table and its hints, the pipeline, a data
    // section, the caller's hints and recovery hints. The response is built and rendered on every surface.
    assert_linear(
        "AgentResponse mixed outcomes",
        500,
        |n| (causes_error(n), ops(n), status(n), pipeline(n), n),
        |(e, ps, s, p, n)| {
            let rows = (0..*n).map(|i| vec![Value::Int(i as i64), Value::from(format!("result {i}"))]).collect();
            let mut r = AgentResponse::new("batch")
                .error(e.clone())
                .section(Section::toon("results", vec!["n".into(), "title".into()], rows).total_count(Some(n * 2)))
                .partial_success("ops", ps.clone())
                .status(s.clone())
                .pipeline("step", p.clone())
                .budget(BudgetSummary::new().tokens_used(1));
            r = r.hints((0..*n).map(|i| Hint::new(format!("hint {i}"))).collect());
            for i in 0..*n {
                r = r.hint(format!("more {i}")).recovery_hint(RecoveryHint::new(format!("tool{i}")));
            }
            black_box(r.render(OutputFormat::Text));
            black_box(r.render(OutputFormat::Json));
            black_box(r.is_error());
            black_box(r.to_call_tool_result());
        },
    );
}

#[cfg(feature = "serde")]
#[test]
fn toon_list_is_linear_in_items_and_sparse_keys() {
    #[derive(serde::Serialize)]
    struct Issue {
        number: usize,
        title: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        assignee: Option<String>,
    }
    assert_linear(
        "toon::list items",
        1_000,
        |n| {
            (0..n)
                .map(|i| Issue {
                    number: i,
                    title: format!("Issue {i}"),
                    assignee: (i % 3 == 0).then(|| format!("user{i}")),
                })
                .collect::<Vec<_>>()
        },
        |items| {
            black_box(michi_core::toon::list("issue", items).expect("valid items"));
        },
    );
}

/// Fails, instead of hanging, when `work` does not finish within `limit`: a quadratic regression on a large input can
/// run for minutes in a debug build, and a hung test says nothing. The work runs on its own thread, which is left to
/// die with the test process on a timeout.
fn finishes_within(label: &str, limit: Duration, work: impl FnOnce() + Send + 'static) {
    let (done, finished) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work));
        // The receiver is gone after a timeout.
        let _ = done.send(outcome);
    });
    match finished.recv_timeout(limit) {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => std::panic::resume_unwind(panic),
        Err(_) => panic!("{label}: did not finish within {limit:?}; linear work on this input takes about a second"),
    }
}

#[test]
fn redact_common_shapes_is_linear_in_pairs() {
    // One long line of pairs: each quoted, unterminated or scheme value must be found without rescanning the rest of
    // the text. A per-value scan to the end of the line was quadratic here. The size is small (5,000 pairs at most) and
    // the whole check is bounded, because a quadratic regression on a large input runs for minutes in a debug build.
    finishes_within("redact_common_shapes pairs", Duration::from_secs(10), || {
        for unit in ["token=\"", "token: Basic ", "a:b=c ", "x=1&password='p'&", "Cookie: a=1; "] {
            assert_linear(
                unit,
                500,
                |n| unit.repeat(n),
                |text| {
                    black_box(michi_core::redact::redact_common_shapes(text));
                },
            );
        }
    });
}
