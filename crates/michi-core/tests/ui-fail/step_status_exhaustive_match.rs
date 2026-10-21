fn main() {
    let s = michi_core::pipeline::StepStatus::Completed;
    match s {
        michi_core::pipeline::StepStatus::Pending => {}
        michi_core::pipeline::StepStatus::Running => {}
        michi_core::pipeline::StepStatus::Completed => {}
        michi_core::pipeline::StepStatus::CompletedEarlier => {}
        michi_core::pipeline::StepStatus::Failed => {}
        michi_core::pipeline::StepStatus::OutcomeUnknown(_) => {}
        michi_core::pipeline::StepStatus::Skipped(_) => {}
        michi_core::pipeline::StepStatus::NotRun(_) => {}
        michi_core::pipeline::StepStatus::Panicked => {}
    }
}
