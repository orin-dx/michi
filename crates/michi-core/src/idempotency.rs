use crate::toon::Value;

/// The columns of the table a [`PartialSuccess`] renders as.
pub(crate) const FIELDS: [&str; 4] = ["index", "operation", "status", "reason"];

/// An operation that completed within a batch: where it sat in the caller's op slice and what it was called.
///
/// `#[non_exhaustive]`: build it with [`CompletedOp::new`]; the fields stay public. It has no reason: a completed
/// operation renders none, so it cannot carry one that would be silently dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct CompletedOp {
    /// Position in the caller's op slice. `results[index]` answers `ops[index]`.
    pub index: usize,
    /// Display name for the operation.
    pub operation: String,
}

impl CompletedOp {
    /// A completed operation at `index`.
    #[must_use]
    pub fn new(index: usize, operation: impl Into<String>) -> Self {
        Self { index, operation: operation.into() }
    }
}

/// An operation that was not attempted within a batch, optionally with why.
///
/// `#[non_exhaustive]`: build it with [`SkippedOp::new`] (and [`SkippedOp::reason`]); the fields stay public.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SkippedOp {
    /// Position in the caller's op slice. `results[index]` answers `ops[index]`.
    pub index: usize,
    /// Display name for the operation.
    pub operation: String,
    /// A stable machine token such as `budget_exhausted`, `deadline` or `not_indexed`, never prose. michi does not
    /// define the vocabulary; the caller does. `None` renders `null`.
    pub reason: Option<String>,
}

impl SkippedOp {
    /// A skipped operation at `index` with no reason.
    #[must_use]
    pub fn new(index: usize, operation: impl Into<String>) -> Self {
        Self { index, operation: operation.into(), reason: None }
    }

    /// Attach the machine token saying why this operation was skipped.
    #[must_use]
    pub fn reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }
}

/// A single operation that failed within a larger multi-step operation.
///
/// `#[non_exhaustive]`: build it with [`FailedOp::new`] (and [`FailedOp::recovery`]); the fields stay public.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct FailedOp {
    /// Position in the caller's op slice.
    pub index: usize,
    /// Identifier of the operation that failed.
    pub operation: String,
    /// Human-readable failure reason.
    pub reason: String,
    /// Optional structured recovery hint for this specific failure.
    pub recovery: Option<crate::recovery::RecoveryHint>,
}

impl FailedOp {
    /// A failed operation at `index` with its reason and no recovery hint.
    #[must_use]
    pub fn new(index: usize, operation: impl Into<String>, reason: impl Into<String>) -> Self {
        Self { index, operation: operation.into(), reason: reason.into(), recovery: None }
    }

    /// Attach a structured recovery hint for this failure.
    #[must_use]
    pub fn recovery(mut self, hint: crate::recovery::RecoveryHint) -> Self {
        self.recovery = Some(hint);
        self
    }
}

/// Signals that an operation partially completed before a failure.
///
/// Use this when some steps of a multi-step operation succeeded: the agent can resume from the checkpoint rather
/// than retrying from scratch. Every entry carries its `index` in the caller's op slice, so the rendered report
/// keeps `results[i]` and `ops[i]` aligned.
///
/// For a fully-completed prior call, see [`crate::resilience::AlreadyDone`]
/// and [`crate::resilience::already_done`].
///
/// Build it with [`PartialSuccess::try_new`] (or [`PartialSuccess::default`] for an empty batch) and read it with the
/// getters. Each index has exactly one outcome: the fields are private, so a report that lists an operation as both
/// completed and failed cannot be built.
///
/// ```compile_fail,E0616
/// let mut ps = michi_core::PartialSuccess::default();
/// ps.completed.push(michi_core::CompletedOp::new(0, "a"));
/// ```
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct PartialSuccess {
    completed: Vec<CompletedOp>,
    failed: Vec<FailedOp>,
    skipped: Vec<SkippedOp>,
}

/// [`PartialSuccess::try_new`] was given two outcomes for one operation index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct DuplicateOpIndex {
    /// The index that appeared more than once across `completed`, `failed` and `skipped`.
    pub index: usize,
}

impl std::fmt::Display for DuplicateOpIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "operation index {} has more than one outcome", self.index)
    }
}

impl std::error::Error for DuplicateOpIndex {}

impl PartialSuccess {
    /// A partial-success report from the completed, failed and skipped operations, each kept in the order given.
    ///
    /// # Errors
    ///
    /// [`DuplicateOpIndex`] naming the first index (in completed, failed, skipped order) that appears more than once,
    /// within one list or across them.
    pub fn try_new(
        completed: Vec<CompletedOp>,
        failed: Vec<FailedOp>,
        skipped: Vec<SkippedOp>,
    ) -> Result<Self, DuplicateOpIndex> {
        let indices = completed
            .iter()
            .map(|c| c.index)
            .chain(failed.iter().map(|f| f.index))
            .chain(skipped.iter().map(|s| s.index));
        let mut seen = std::collections::HashSet::with_capacity(completed.len() + failed.len() + skipped.len());
        for index in indices {
            if !seen.insert(index) {
                return Err(DuplicateOpIndex { index });
            }
        }
        Ok(Self { completed, failed, skipped })
    }

    /// The operations that completed, in the order given.
    #[must_use]
    pub fn completed(&self) -> &[CompletedOp] {
        &self.completed
    }

    /// The operations that failed, in the order given.
    #[must_use]
    pub fn failed(&self) -> &[FailedOp] {
        &self.failed
    }

    /// The operations that were not attempted, in the order given.
    #[must_use]
    pub fn skipped(&self) -> &[SkippedOp] {
        &self.skipped
    }

    /// Render as a TOON 4.4 document, with no trailing newline: the response
    /// `AgentResponse::new("partial_success").partial_success("partial_success", ps)` with no cell cap, so it is the
    /// same writer a composed response uses (see [`crate::AgentResponse::partial_success`]).
    ///
    /// ```text
    /// partial_success[4]{index,operation,status,reason}:
    ///   0,create_issue,completed,null
    ///   1,add_label,completed,null
    ///   2,assign_user,failed,User 'ghost' not found
    ///   3,notify_team,skipped,budget_exhausted
    /// recovery[1]{tool,params,reason}:
    ///   assign_user,"{\"user\":\"alice\"}",null
    /// ```
    ///
    /// One row per operation, sorted by `index` (never renumbered); `status` is `completed`, `failed` or `skipped`,
    /// and `reason` is the failure reason, the skip token, or `null`. The failed operations' recovery hints are the
    /// document's one `recovery` table. An empty report is `partial_success: []` and `totalCount: 0`.
    #[must_use]
    pub fn render(&self) -> String {
        crate::AgentResponse::new("partial_success")
            .truncate_cells_at(usize::MAX)
            .partial_success("partial_success", self.clone())
            .render(crate::OutputFormat::Text)
    }

    /// The table a composed response renders: one row per operation, sorted by index.
    pub(crate) fn rows(&self) -> Vec<Vec<Value>> {
        let mut rows: Vec<(usize, Vec<Value>)> =
            Vec::with_capacity(self.completed.len() + self.failed.len() + self.skipped.len());
        for c in &self.completed {
            rows.push((
                c.index,
                vec![Value::from(c.index), Value::from(c.operation.as_str()), "completed".into(), Value::Null],
            ));
        }
        for f in &self.failed {
            let row = vec![
                Value::from(f.index),
                Value::from(f.operation.as_str()),
                "failed".into(),
                f.reason.as_str().into(),
            ];
            rows.push((f.index, row));
        }
        for s in &self.skipped {
            let reason = s.reason.as_deref().map_or(Value::Null, Value::from);
            rows.push((
                s.index,
                vec![Value::from(s.index), Value::from(s.operation.as_str()), "skipped".into(), reason],
            ));
        }
        // Indices are unique (`try_new`), so the order is total.
        rows.sort_unstable_by_key(|(index, _)| *index);
        rows.into_iter().map(|(_, row)| row).collect()
    }

    /// The failed operations' recovery hints, in index order.
    pub(crate) fn recovery_hints(&self) -> Vec<crate::recovery::RecoveryHint> {
        let mut failed: Vec<&FailedOp> = self.failed.iter().filter(|f| f.recovery.is_some()).collect();
        failed.sort_unstable_by_key(|f| f.index);
        failed.into_iter().filter_map(|f| f.recovery.clone()).collect()
    }

    /// `0` when all operations completed or were skipped; `1` when any failed.
    #[must_use]
    pub fn exit_code(&self) -> i32 {
        i32::from(!self.failed.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kv::KvValue;
    use crate::recovery::RecoveryHint;

    const HEAD: &str = "{index,operation,status,reason}:";

    fn fail(index: usize, operation: &str, reason: &str) -> FailedOp {
        FailedOp::new(index, operation, reason)
    }

    fn ps(completed: Vec<CompletedOp>, failed: Vec<FailedOp>, skipped: Vec<SkippedOp>) -> PartialSuccess {
        PartialSuccess::try_new(completed, failed, skipped).expect("no index repeats")
    }

    #[test]
    fn partial_success_full_example_matches_spec() {
        let ps = ps(
            vec![CompletedOp::new(0, "create_issue"), CompletedOp::new(1, "add_label")],
            vec![fail(2, "assign_user", "User 'ghost' not found")
                .recovery(RecoveryHint::new("assign_user").param("user", KvValue::Text("alice".into())))],
            vec![SkippedOp::new(3, "notify_team").reason("budget_exhausted")],
        );
        assert_eq!(
            ps.render(),
            format!(
                "partial_success[4]{HEAD}\n  0,create_issue,completed,null\n  1,add_label,completed,null\n  \
                 2,assign_user,failed,User 'ghost' not found\n  3,notify_team,skipped,budget_exhausted\n\
                 recovery[1]{{tool,params,reason}}:\n  assign_user,\"{{\\\"user\\\":\\\"alice\\\"}}\",null"
            )
        );
    }

    // The spec 07 batch, as one table.
    #[test]
    fn spec_07_example_renders_exactly() {
        let ps = ps(
            vec![CompletedOp::new(0, "symbols"), CompletedOp::new(1, "dependents")],
            vec![fail(2, "refs", "symbol not found: RetryPolicy")],
            vec![SkippedOp::new(3, "search").reason("budget_exhausted")],
        );
        assert_eq!(
            ps.render(),
            format!(
                "partial_success[4]{HEAD}\n  0,symbols,completed,null\n  1,dependents,completed,null\n  \
                 2,refs,failed,\"symbol not found: RetryPolicy\"\n  3,search,skipped,budget_exhausted"
            )
        );
    }

    // AC-032 (superseded): a report with no operation is the definitive empty list.
    #[test]
    fn ac032_fully_empty_partial_success_is_the_empty_list() {
        assert_eq!(ps(vec![], vec![], vec![]).render(), "partial_success: []\ntotalCount: 0");
    }

    // AC-033 (superseded): two completed, nothing else.
    #[test]
    fn ac033_two_completed_only_is_two_rows() {
        let ps = ps(vec![CompletedOp::new(0, "a"), CompletedOp::new(1, "b")], vec![], vec![]);
        assert_eq!(ps.render(), format!("partial_success[2]{HEAD}\n  0,a,completed,null\n  1,b,completed,null"));
    }

    // The index is the caller's slice position, written as given: never renumbered, and gaps survive. Rows sort by
    // it, so row order follows the caller's ops whatever order the three lists were built in.
    #[test]
    fn index_is_preserved_and_rows_sort_by_it() {
        let ps = ps(
            vec![CompletedOp::new(7, "x"), CompletedOp::new(2, "y")],
            vec![fail(5, "f", "r"), fail(1, "g", "s")],
            vec![SkippedOp::new(9, "k"), SkippedOp::new(4, "j")],
        );
        assert_eq!(
            ps.render(),
            format!(
                "partial_success[6]{HEAD}\n  1,g,failed,s\n  2,y,completed,null\n  4,j,skipped,null\n  \
                 5,f,failed,r\n  7,x,completed,null\n  9,k,skipped,null"
            )
        );
    }

    #[test]
    fn recovery_hints_follow_the_index_order() {
        let ps = ps(
            vec![],
            vec![
                fail(5, "f", "r").recovery(RecoveryHint::new("five")),
                fail(1, "g", "s").recovery(RecoveryHint::new("one")),
                fail(3, "h", "t"),
            ],
            vec![],
        );
        let tools: Vec<String> = ps.recovery_hints().into_iter().map(|r| r.tool).collect();
        assert_eq!(tools, ["one", "five"]);
    }

    #[test]
    fn large_index_renders_in_full() {
        let ps = ps(vec![CompletedOp::new(usize::MAX, "a")], vec![], vec![]);
        assert!(ps.render().ends_with(&format!("\n  {},a,completed,null", usize::MAX)), "{}", ps.render());
    }

    #[test]
    fn skipped_reason_is_rendered_and_a_missing_one_is_null() {
        let ps = ps(vec![], vec![], vec![SkippedOp::new(0, "a").reason("deadline"), SkippedOp::new(1, "b")]);
        assert_eq!(ps.render(), format!("partial_success[2]{HEAD}\n  0,a,skipped,deadline\n  1,b,skipped,null"));
    }

    // A literal "null" reason must not read back as the absent marker: it is quoted.
    #[test]
    fn skipped_reason_that_spells_null_is_quoted() {
        let ps = ps(vec![], vec![], vec![SkippedOp::new(0, "a").reason("null")]);
        assert!(ps.render().ends_with("\n  0,a,skipped,\"null\""), "got: {}", ps.render());
    }

    // Each index has one outcome: a repeat within a list or across lists is rejected, naming the index.
    #[test]
    fn duplicate_index_is_rejected() {
        let dup = |c: Vec<CompletedOp>, f: Vec<FailedOp>, s: Vec<SkippedOp>| PartialSuccess::try_new(c, f, s);
        assert_eq!(
            dup(vec![CompletedOp::new(0, "a")], vec![fail(0, "a", "boom")], vec![]),
            Err(DuplicateOpIndex { index: 0 })
        );
        assert_eq!(
            dup(vec![], vec![fail(4, "a", "x")], vec![SkippedOp::new(4, "a")]),
            Err(DuplicateOpIndex { index: 4 })
        );
        assert_eq!(
            dup(vec![CompletedOp::new(2, "a"), CompletedOp::new(2, "b")], vec![], vec![]),
            Err(DuplicateOpIndex { index: 2 })
        );
        assert_eq!(DuplicateOpIndex { index: 2 }.to_string(), "operation index 2 has more than one outcome");
        assert!(dup(vec![CompletedOp::new(0, "a")], vec![fail(1, "b", "x")], vec![SkippedOp::new(2, "c")]).is_ok());
    }

    #[test]
    fn hostile_operation_and_reason_strings_are_quoted_in_every_row() {
        let hostile = "a,b\n\"c\"\r: [d] {e}";
        let ps = ps(
            vec![CompletedOp::new(0, hostile)],
            vec![fail(1, hostile, hostile)],
            vec![SkippedOp::new(2, hostile).reason(hostile)],
        );
        let q = crate::toon::escape_value(hostile);
        assert!(q.starts_with('"'), "fixture must need quoting, got: {q}");
        assert_eq!(
            ps.render(),
            format!("partial_success[3]{HEAD}\n  0,{q},completed,null\n  1,{q},failed,{q}\n  2,{q},skipped,{q}")
        );
        assert!(!ps.render().contains('\r'), "no raw CR may survive");
        assert_eq!(ps.render().lines().count(), 4, "hostile text must not add lines");
    }

    #[test]
    fn partial_success_exit_code_zero_when_no_failures() {
        let ps = ps(vec![CompletedOp::new(0, "a")], vec![], vec![SkippedOp::new(1, "b")]);
        assert_eq!(ps.exit_code(), 0);
    }

    // AC-036: exit_code()==0 holds for the fully-empty combination too, not
    // just the completed+skipped-both-nonempty combination above.
    #[test]
    fn ac036_exit_code_zero_holds_when_completed_and_skipped_are_also_empty() {
        let ps = ps(vec![], vec![], vec![]);
        assert_eq!(ps.exit_code(), 0);
    }

    #[test]
    fn partial_success_exit_code_one_when_any_failed() {
        let ps = ps(vec![], vec![fail(0, "x", "y")], vec![]);
        assert_eq!(ps.exit_code(), 1);
    }

    // AC-037: exit_code()==1 regardless of how many entries failed contains
    // — the existing test above only proves the 1-entry case.
    #[test]
    fn ac037_exit_code_one_holds_for_five_failed_entries() {
        let ps = ps(vec![], (0..5).map(|i| fail(i, &format!("op{i}"), "y")).collect(), vec![]);
        assert_eq!(ps.exit_code(), 1);
    }

    #[test]
    fn failed_op_without_recovery_produces_no_help_block() {
        let ps = ps(vec![], vec![fail(0, "x", "y")], vec![]);
        assert!(!ps.render().contains("help["));
    }

    #[test]
    fn operation_with_comma_is_escaped() {
        let ps = ps(vec![], vec![fail(0, "create_issue, retry 2", "timeout")], vec![]);
        let out = ps.render();
        // operation contains comma — must be quoted to avoid breaking TOON column parsing
        assert!(out.contains(r#""create_issue, retry 2""#), "operation must be quoted, got: {out}");
    }

    // AC-034: a newline/CR in operation, even with no comma or quote present,
    // triggers quoting and renders as the \n / \r escapes inside the quotes.
    #[test]
    fn ac034_newline_and_cr_in_operation_are_quoted_and_escaped() {
        let ps = ps(vec![], vec![fail(3, "step\none\rtwo", "y")], vec![]);
        let out = ps.render();
        assert!(out.ends_with("\n  3,\"step\\none\\rtwo\",failed,y"), "got: {out:?}");
        assert!(!out.contains('\r'), "no raw CR may survive, got: {out:?}");
    }

    // AC-035 (superseded): recovery from a failed op appends one trailing recovery table — the same block as
    // render_recovery, not a help[] block, since help is the inline array of strings.
    #[test]
    fn ac035_recovery_hints_render_as_one_recovery_table() {
        let ps = ps(vec![], vec![fail(0, "x", "y").recovery(RecoveryHint::new("retry_x"))], vec![]);
        let out = ps.render();
        let recovery = crate::recovery::render_recovery(&[RecoveryHint::new("retry_x")]);
        assert!(out.ends_with(&format!("\n{recovery}")), "got: {out}");
        assert_eq!(out.matches("recovery[").count(), 1, "got: {out}");
        assert!(!out.contains("help["), "got: {out}");
    }
}
