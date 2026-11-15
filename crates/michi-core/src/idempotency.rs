/// One operation's identity within a batch: where it sat in the caller's op slice, what it was called, and (for
/// skipped operations) why it did not run.
///
/// `#[non_exhaustive]`: build it with [`OpRef::new`] (and [`OpRef::reason`]); the fields stay public.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct OpRef {
    /// Position in the caller's op slice. `results[index]` answers `ops[index]`.
    pub index: usize,
    /// Display name for the operation.
    pub operation: String,
    /// A stable machine token such as `budget_exhausted`, `deadline` or `not_indexed`, never prose. michi does not
    /// define the vocabulary; the caller does. Rendered for skipped operations only; completed operations have no
    /// reason column.
    pub reason: Option<String>,
}

impl OpRef {
    /// An operation at `index` with no reason.
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
/// `#[non_exhaustive]`: build it with [`PartialSuccess::new`] or [`PartialSuccess::default`]; the fields stay
/// public.
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct PartialSuccess {
    /// Steps that completed successfully.
    pub completed: Vec<OpRef>,
    /// Steps that failed, with reason and optional recovery hint.
    pub failed: Vec<FailedOp>,
    /// Steps that were not attempted, each optionally with the reason it was skipped.
    pub skipped: Vec<OpRef>,
}

impl PartialSuccess {
    /// A partial-success report from the completed steps, the failed operations and the skipped steps.
    #[must_use]
    pub fn new(completed: Vec<OpRef>, failed: Vec<FailedOp>, skipped: Vec<OpRef>) -> Self {
        Self { completed, failed, skipped }
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
    /// per §7.2; a skipped step with no reason renders `null` in the `reason` column. A completed step's `reason` is
    /// not rendered. Empty categories are omitted.
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
        out.push_str("partial_success: \"");
        out.push_str(&self.completed.len().to_string());
        out.push_str(" completed, ");
        out.push_str(&self.failed.len().to_string());
        out.push_str(" failed, ");
        out.push_str(&self.skipped.len().to_string());
        out.push_str(" skipped\"");

        if !self.completed.is_empty() {
            push_table_header(&mut out, "completed", self.completed.len(), "index,operation");
            for c in &self.completed {
                push_row_start(&mut out, c.index);
                out.push_str(&crate::toon::escape_value(&c.operation));
            }
        }

        if !self.failed.is_empty() {
            push_table_header(&mut out, "failed", self.failed.len(), "index,operation,reason");
            for f in &self.failed {
                push_row_start(&mut out, f.index);
                out.push_str(&crate::toon::escape_value(&f.operation));
                out.push(',');
                out.push_str(&crate::toon::escape_value(&f.reason));
            }
        }

        if !self.skipped.is_empty() {
            push_table_header(&mut out, "skipped", self.skipped.len(), "index,operation,reason");
            for s in &self.skipped {
                push_row_start(&mut out, s.index);
                out.push_str(&crate::toon::escape_value(&s.operation));
                out.push(',');
                match &s.reason {
                    Some(r) => out.push_str(&crate::toon::escape_value(r)),
                    None => out.push_str("null"),
                }
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

/// Append `\n{key}[N]{columns}:`, the header of a TOON 4.4 tabular array (§9.3).
fn push_table_header(out: &mut String, key: &str, len: usize, columns: &str) {
    out.push('\n');
    out.push_str(key);
    out.push('[');
    out.push_str(&len.to_string());
    out.push_str("]{");
    out.push_str(columns);
    out.push_str("}:");
}

/// Append `\n  {index},`, the start of a table row.
fn push_row_start(out: &mut String, index: usize) {
    out.push_str("\n  ");
    out.push_str(&index.to_string());
    out.push(',');
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kv::KvValue;
    use crate::recovery::RecoveryHint;

    fn fail(index: usize, operation: &str, reason: &str) -> FailedOp {
        FailedOp::new(index, operation, reason)
    }

    #[test]
    fn partial_success_full_example_matches_spec() {
        let ps = PartialSuccess::new(
            vec![OpRef::new(0, "create_issue"), OpRef::new(1, "add_label")],
            vec![fail(2, "assign_user", "User 'ghost' not found")
                .recovery(RecoveryHint::new("assign_user").param("user", KvValue::Text("alice".into())))],
            vec![OpRef::new(3, "notify_team").reason("budget_exhausted")],
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
        let ps = PartialSuccess::new(
            vec![OpRef::new(0, "symbols"), OpRef::new(1, "dependents")],
            vec![fail(2, "refs", "symbol not found: RetryPolicy")],
            vec![OpRef::new(3, "search").reason("budget_exhausted")],
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
        let ps = PartialSuccess::new(vec![OpRef::new(0, "a")], vec![], vec![]);
        let out = ps.render();
        assert!(!out.contains("failed["), "empty failed category must be omitted, got: {out}");
        assert!(!out.contains("skipped["), "empty skipped category must be omitted, got: {out}");
    }

    // AC-032: fully-empty PartialSuccess — exact first line, no section
    // headers at all (not just failed/skipped omitted, as the test above
    // checks — completed[] must also be absent here since it's empty too).
    #[test]
    fn ac032_fully_empty_partial_success_has_exact_summary_and_no_sections() {
        let ps = PartialSuccess::new(vec![], vec![], vec![]);
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
        let ps = PartialSuccess::new(vec![OpRef::new(0, "a"), OpRef::new(1, "b")], vec![], vec![]);
        let out = ps.render();
        assert_eq!(
            out,
            "partial_success: \"2 completed, 0 failed, 0 skipped\"\ncompleted[2]{index,operation}:\n  0,a\n  1,b"
        );
    }

    // The index is the caller's slice position, written as given: not renumbered, not sorted, and gaps survive.
    #[test]
    fn index_and_order_are_preserved_not_renumbered() {
        let ps = PartialSuccess::new(
            vec![OpRef::new(7, "x"), OpRef::new(2, "y")],
            vec![fail(5, "f", "r"), fail(1, "g", "s")],
            vec![OpRef::new(9, "k"), OpRef::new(4, "j")],
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
        let ps = PartialSuccess::new(vec![OpRef::new(usize::MAX, "a")], vec![], vec![]);
        assert!(ps.render().ends_with(&format!("\n  {},a", usize::MAX)));
    }

    #[test]
    fn skipped_reason_is_rendered_and_a_missing_one_is_null() {
        let ps = PartialSuccess::new(vec![], vec![], vec![OpRef::new(0, "a").reason("deadline"), OpRef::new(1, "b")]);
        assert_eq!(
            ps.render(),
            "partial_success: \"0 completed, 0 failed, 2 skipped\"\nskipped[2]{index,operation,reason}:\n  \
             0,a,deadline\n  1,b,null"
        );
    }

    // A literal "null" reason must not read back as the absent marker: escape_value quotes it.
    #[test]
    fn skipped_reason_that_spells_null_is_quoted() {
        let ps = PartialSuccess::new(vec![], vec![], vec![OpRef::new(0, "a").reason("null")]);
        assert!(ps.render().ends_with("\n  0,a,\"null\""), "got: {}", ps.render());
    }

    #[test]
    fn completed_reason_is_not_rendered() {
        let ps = PartialSuccess::new(vec![OpRef::new(0, "a").reason("because")], vec![], vec![]);
        assert!(!ps.render().contains("because"), "got: {}", ps.render());
    }

    #[test]
    fn hostile_operation_and_reason_strings_are_quoted_in_every_table() {
        let hostile = "a,b\n\"c\"\r: [d] {e}";
        let ps = PartialSuccess::new(
            vec![OpRef::new(0, hostile)],
            vec![fail(1, hostile, hostile)],
            vec![OpRef::new(2, hostile).reason(hostile)],
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
        let ps = PartialSuccess::new(vec![OpRef::new(0, "a")], vec![], vec![OpRef::new(1, "b")]);
        assert_eq!(ps.exit_code(), 0);
    }

    // AC-036: exit_code()==0 holds for the fully-empty combination too, not
    // just the completed+skipped-both-nonempty combination above.
    #[test]
    fn ac036_exit_code_zero_holds_when_completed_and_skipped_are_also_empty() {
        let ps = PartialSuccess::new(vec![], vec![], vec![]);
        assert_eq!(ps.exit_code(), 0);
    }

    #[test]
    fn partial_success_exit_code_one_when_any_failed() {
        let ps = PartialSuccess::new(vec![], vec![fail(0, "x", "y")], vec![]);
        assert_eq!(ps.exit_code(), 1);
    }

    // AC-037: exit_code()==1 regardless of how many entries failed contains
    // — the existing test above only proves the 1-entry case.
    #[test]
    fn ac037_exit_code_one_holds_for_five_failed_entries() {
        let ps = PartialSuccess::new(vec![], (0..5).map(|i| fail(i, &format!("op{i}"), "y")).collect(), vec![]);
        assert_eq!(ps.exit_code(), 1);
    }

    #[test]
    fn failed_op_without_recovery_produces_no_help_block() {
        let ps = PartialSuccess::new(vec![], vec![fail(0, "x", "y")], vec![]);
        assert!(!ps.render().contains("help["));
    }

    #[test]
    fn operation_with_comma_is_escaped() {
        let ps = PartialSuccess::new(vec![], vec![fail(0, "create_issue, retry 2", "timeout")], vec![]);
        let out = ps.render();
        // operation contains comma — must be quoted to avoid breaking TOON column parsing
        assert!(out.contains(r#""create_issue, retry 2""#), "operation must be quoted, got: {out}");
    }

    // AC-034: a newline/CR in operation, even with no comma or quote present,
    // triggers quoting and renders as the \n / \r escapes inside the quotes.
    #[test]
    fn ac034_newline_and_cr_in_operation_are_quoted_and_escaped() {
        let ps = PartialSuccess::new(vec![], vec![fail(3, "step\none\rtwo", "y")], vec![]);
        let out = ps.render();
        assert!(out.ends_with("\n  3,\"step\\none\\rtwo\",y"), "got: {out:?}");
        assert!(!out.contains('\r'), "no raw CR may survive, got: {out:?}");
    }

    // AC-035 (superseded): recovery from a failed op appends one trailing recovery table — the same block as
    // render_recovery, not a help[] block, since help is the inline array of strings.
    #[test]
    fn ac035_recovery_hints_render_as_one_recovery_table() {
        let ps = PartialSuccess::new(vec![], vec![fail(0, "x", "y").recovery(RecoveryHint::new("retry_x"))], vec![]);
        let out = ps.render();
        let recovery = crate::recovery::render_recovery(&[RecoveryHint::new("retry_x")]);
        assert!(out.ends_with(&format!("\n{recovery}")), "got: {out}");
        assert_eq!(out.matches("recovery[").count(), 1, "got: {out}");
        assert!(!out.contains("help["), "got: {out}");
    }
}
