// 0.4 API hygiene: these structs are `#[non_exhaustive]`, so a struct literal outside michi-core fails to compile.
// Each has a public constructor instead (see tests/public_construction.rs).
fn main() {
    let _ = michi_core::FailedOp { operation: "op".to_string(), reason: "r".to_string(), recovery: None };
    let _ = michi_core::PartialSuccess { completed: vec![], failed: vec![], skipped: vec![] };
    let _ = michi_core::KvItem { key: "k".to_string(), value: michi_core::KvValue::Int(1) };
    let _ = michi_core::StatusItem { key: "k".to_string(), value: michi_core::KvValue::Int(1), health: None };
    let _ = michi_core::RecoveryHint { tool: "t".to_string(), params: vec![], reason: None };
    let _ = michi_core::pipeline::PipelineStep {
        id: "s".to_string(),
        name: "n".to_string(),
        status: michi_core::pipeline::StepStatus::Pending,
    };
    let _ = michi_core::pipeline::Pipeline { id: "p".to_string(), steps: vec![] };
}
