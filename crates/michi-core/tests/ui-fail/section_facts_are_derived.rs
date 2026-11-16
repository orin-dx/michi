// A section's count and cut are derived from its body and total, so no caller can set them apart: there is no field
// to assign, no `truncated(bool)` setter, and no hand-typed budget roll-up line.
use michi_core::toon::Value;
use michi_core::{BudgetSummary, Section};

fn main() {
    let mut s = Section::toon("t", vec!["a".into()], vec![vec![Value::Int(1)]]).total_count(Some(5));
    s.truncated = false;
    s.returned_count = 50;
    let _ = s.clone().truncated(false);
    let _ = BudgetSummary::new().section(michi_core::SectionCost::new("ghost", 99));
}
