//! Run by scripts/smoke-published.sh against the published crates. Panics (non-zero exit) on any mismatch.

// This binary exists to print its result.
#![allow(clippy::disallowed_macros)]

use michi_core::toon::{render_toon, ToonOptions, Value};
use michi_graph::{Dag, Order};
use michi_pipeline::{execute, step_fn, ExecOptions, ExecStep, Plan, RunId, RunOutcome, StepId};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    // michi-core: a TOON table.
    let opts = ToonOptions::new(
        "issues",
        vec!["number".to_string(), "title".to_string()],
        vec![vec![Value::Int(1), Value::from("first")], vec![Value::Int(2), Value::from("second")]],
    )
    .total_count(Some(2));
    let toon = render_toon(&opts).expect("valid toon input");
    assert_eq!(toon, "issues[2]{number,title}:\n  1,first\n  2,second\ntotalCount: 2");

    // michi-pipeline: a two-step plan on tokio.
    let fetch = StepId::try_new("fetch").expect("valid id");
    let upload = StepId::try_new("upload").expect("valid id");
    let plan = Plan::new(vec![
        ExecStep::new(fetch.clone(), "Fetch", step_fn(|_ctx| async { Ok(()) })),
        ExecStep::new(upload, "Upload", step_fn(|_ctx| async { Ok(()) })).depends_on(fetch),
    ])
    .expect("valid plan");
    let record = execute(&plan, RunId::try_new("smoke-1").expect("valid id"), &ExecOptions::new()).await;
    assert_eq!(record.outcome(), RunOutcome::Succeeded);
    assert_eq!(
        record.render(),
        "step[2]{id,name,status,reason}:\n  fetch,Fetch,completed,null\n  upload,Upload,completed,null"
    );

    // michi-graph: a diamond.
    let dag = Dag::from_nodes(
        [("app", vec!["lib", "util"]), ("lib", vec!["core"]), ("util", vec!["core"]), ("core", vec![])],
        Order::Declaration,
    )
    .expect("acyclic");
    let order: Vec<&str> = dag.keys_in_order().copied().collect();
    assert_eq!(order, ["core", "lib", "util", "app"]);
    assert_eq!(dag.layers().len(), 3);

    println!("crates smoke OK");
}
