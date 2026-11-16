use std::fmt::Write as _;

use crate::toon::table::{self, Cell};

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

    /// Render as a TOON 4.4 root object, with no trailing newline: a quoted summary, one tabular array per non-empty
    /// outcome category, then the failed operations' recovery hints as a `recovery[N]{tool,params,reason}:` table
    /// (see [`crate::recovery::render_recovery`]).
    ///
    /// Format:
    /// ```text
    /// partial_success: "2 completed, 1 failed, 1 skipped"
    /// completed[2]{index,operation}:
    ///   0,create_issue
    ///   1,add_label
    /// failed[1]{index,operation,reason}:
    ///   2,assign_user,User 'ghost' not found
    /// skipped[1]{index,operation,reason}:
    ///   3,notify_team,budget_exhausted
    /// recovery[1]{tool,params,reason}:
    ///   assign_user,"{\"user\":\"alice\"}",null
    /// ```
    ///
    /// Rows keep the order of the vectors, and `index` is written as given (never renumbered). Every string is quoted
    /// per §7.2; a skipped step with no reason renders `null` in the `reason` column. Empty categories are omitted.
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = String::with_capacity(
            64 + self.completed.iter().map(|s| s.operation.len() + 24).sum::<usize>()
                + self.failed.iter().map(|f| f.operation.len() + f.reason.len() + 80).sum::<usize>()
                + self
                    .skipped
                    .iter()
                    .map(|s| s.operation.len() + s.reason.as_ref().map_or(4, String::len) + 24)
                    .sum::<usize>(),
        );
        let _ = write!(
            out,
            "partial_success: \"{} completed, {} failed, {} skipped\"",
            self.completed.len(),
            self.failed.len(),
            self.skipped.len()
        );

        if !self.completed.is_empty() {
            table::push_header(&mut out, "completed", self.completed.len(), ["index", "operation"]);
            for c in &self.completed {
                table::push_row(&mut out, &[Cell::count(c.index), Cell::Str(&c.operation)]);
            }
        }

        if !self.failed.is_empty() {
            table::push_header(&mut out, "failed", self.failed.len(), ["index", "operation", "reason"]);
            for f in &self.failed {
                table::push_row(&mut out, &[Cell::count(f.index), Cell::Str(&f.operation), Cell::Str(&f.reason)]);
            }
        }

        if !self.skipped.is_empty() {
            table::push_header(&mut out, "skipped", self.skipped.len(), ["index", "operation", "reason"]);
            for s in &self.skipped {
                let cells = [Cell::count(s.index), Cell::Str(&s.operation), Cell::opt_str(s.reason.as_deref())];
                table::push_row(&mut out, &cells);
            }
        }

        let recovery_hints: Vec<crate::recovery::RecoveryHint> =
            self.failed.iter().filter_map(|f| f.recovery.clone()).collect();
        crate::recovery::append_recovery(&mut out, &recovery_hints);

        out
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
            "partial_success: \"2 completed, 1 failed, 1 skipped\"\ncompleted[2]{index,operation}:\n  0,create_issue\n  \
             1,add_label\nfailed[1]{index,operation,reason}:\n  2,assign_user,User 'ghost' not found\n\
             skipped[1]{index,operation,reason}:\n  3,notify_team,budget_exhausted\n\
             recovery[1]{tool,params,reason}:\n  assign_user,\"{\\\"user\\\":\\\"alice\\\"}\",null"
        );
    }

    // The spec 07 example, verbatim.
    #[test]
    fn spec_07_example_renders_exactly() {
        let ps = ps(
            vec![CompletedOp::new(0, "symbols"), CompletedOp::new(1, "dependents")],
            vec![fail(2, "refs", "symbol not found: RetryPolicy")],
            vec![SkippedOp::new(3, "search").reason("budget_exhausted")],
        );
        assert_eq!(
            ps.render(),
            "partial_success: \"2 completed, 1 failed, 1 skipped\"\ncompleted[2]{index,operation}:\n  0,symbols\n  \
             1,dependents\nfailed[1]{index,operation,reason}:\n  2,refs,\"symbol not found: RetryPolicy\"\n\
             skipped[1]{index,operation,reason}:\n  3,search,budget_exhausted"
        );
    }

    #[test]
    fn partial_success_empty_categories_omitted() {
        let ps = ps(vec![CompletedOp::new(0, "a")], vec![], vec![]);
        let out = ps.render();
        assert!(!out.contains("failed["), "empty failed category must be omitted, got: {out}");
        assert!(!out.contains("skipped["), "empty skipped category must be omitted, got: {out}");
    }

    // AC-032: fully-empty PartialSuccess — exact first line, no section
    // headers at all (not just failed/skipped omitted, as the test above
    // checks — completed[] must also be absent here since it's empty too).
    #[test]
    fn ac032_fully_empty_partial_success_has_exact_summary_and_no_sections() {
        let ps = ps(vec![], vec![], vec![]);
        let out = ps.render();
        assert_eq!(out, "partial_success: \"0 completed, 0 failed, 0 skipped\"");
        assert!(!out.contains("completed["), "got: {out}");
        assert!(!out.contains("failed["), "got: {out}");
        assert!(!out.contains("skipped["), "got: {out}");
    }

    // AC-033: two completed, nothing else — exact first line, exact
    // completed[2] table listing both in order, no failed/skipped sections.
    #[test]
    fn ac033_two_completed_only_has_exact_summary_and_section() {
        let ps = ps(vec![CompletedOp::new(0, "a"), CompletedOp::new(1, "b")], vec![], vec![]);
        let out = ps.render();
        assert_eq!(
            out,
            "partial_success: \"2 completed, 0 failed, 0 skipped\"\ncompleted[2]{index,operation}:\n  0,a\n  1,b"
        );
    }

    // The index is the caller's slice position, written as given: not renumbered, not sorted, and gaps survive.
    #[test]
    fn index_and_order_are_preserved_not_renumbered() {
        let ps = ps(
            vec![CompletedOp::new(7, "x"), CompletedOp::new(2, "y")],
            vec![fail(5, "f", "r"), fail(1, "g", "s")],
            vec![SkippedOp::new(9, "k"), SkippedOp::new(4, "j")],
        );
        assert_eq!(
            ps.render(),
            "partial_success: \"2 completed, 2 failed, 2 skipped\"\ncompleted[2]{index,operation}:\n  7,x\n  2,y\n\
             failed[2]{index,operation,reason}:\n  5,f,r\n  1,g,s\n\
             skipped[2]{index,operation,reason}:\n  9,k,null\n  4,j,null"
        );
    }

    #[test]
    fn large_index_renders_in_full() {
        let ps = ps(vec![CompletedOp::new(usize::MAX, "a")], vec![], vec![]);
        assert!(ps.render().ends_with(&format!("\n  {},a", usize::MAX)));
    }

    #[test]
    fn skipped_reason_is_rendered_and_a_missing_one_is_null() {
        let ps = ps(vec![], vec![], vec![SkippedOp::new(0, "a").reason("deadline"), SkippedOp::new(1, "b")]);
        assert_eq!(
            ps.render(),
            "partial_success: \"0 completed, 0 failed, 2 skipped\"\nskipped[2]{index,operation,reason}:\n  \
             0,a,deadline\n  1,b,null"
        );
    }

    // A literal "null" reason must not read back as the absent marker: escape_value quotes it.
    #[test]
    fn skipped_reason_that_spells_null_is_quoted() {
        let ps = ps(vec![], vec![], vec![SkippedOp::new(0, "a").reason("null")]);
        assert!(ps.render().ends_with("\n  0,a,\"null\""), "got: {}", ps.render());
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
    fn hostile_operation_and_reason_strings_are_quoted_in_every_table() {
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
            format!(
                "partial_success: \"1 completed, 1 failed, 1 skipped\"\ncompleted[1]{{index,operation}}:\n  0,{q}\n\
                 failed[1]{{index,operation,reason}}:\n  1,{q},{q}\nskipped[1]{{index,operation,reason}}:\n  2,{q},{q}"
            )
        );
        assert!(!ps.render().contains('\r'), "no raw CR may survive");
        assert_eq!(ps.render().lines().count(), 7, "hostile text must not add lines");
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
        assert!(out.ends_with("\n  3,\"step\\none\\rtwo\",y"), "got: {out:?}");
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
