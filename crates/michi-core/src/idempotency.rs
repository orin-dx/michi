/// A single operation that failed within a larger multi-step operation.
///
/// `#[non_exhaustive]`: build it with [`FailedOp::new`] (and [`FailedOp::recovery`]); the fields stay public.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct FailedOp {
    /// Identifier of the operation that failed.
    pub operation: String,
    /// Human-readable failure reason.
    pub reason: String,
    /// Optional structured recovery hint for this specific failure.
    pub recovery: Option<crate::recovery::RecoveryHint>,
}

impl FailedOp {
    /// A failed operation with its reason and no recovery hint.
    #[must_use]
    pub fn new(operation: impl Into<String>, reason: impl Into<String>) -> Self {
        Self { operation: operation.into(), reason: reason.into(), recovery: None }
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
/// Use this when some steps of a multi-step operation succeeded — the agent
/// can resume from the checkpoint rather than retrying from scratch.
///
/// `#[non_exhaustive]`: build it with [`PartialSuccess::new`] or [`PartialSuccess::default`]; the fields stay
/// public.
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct PartialSuccess {
    /// Identifiers of steps that completed successfully.
    pub completed: Vec<String>,
    /// Steps that failed, with reason and optional recovery hint.
    pub failed: Vec<FailedOp>,
    /// Identifiers of steps that were not attempted.
    pub skipped: Vec<String>,
}

impl PartialSuccess {
    /// A partial-success report from the completed step ids, the failed operations and the skipped step ids.
    #[must_use]
    pub fn new(completed: Vec<String>, failed: Vec<FailedOp>, skipped: Vec<String>) -> Self {
        Self { completed, failed, skipped }
    }

    /// Render as a TOON 4.4 root object, with no trailing newline: a quoted summary, one entry per non-empty outcome
    /// category, then the failed operations' recovery hints as a `recovery[N]{tool,params,reason}:` table (see
    /// [`crate::recovery::render_recovery`]).
    ///
    /// Format:
    /// ```text
    /// partial_success: "2 completed, 1 failed, 1 skipped"
    /// completed[2]: create_issue,add_label
    /// failed[1]{operation,reason}:
    ///   assign_user,User 'ghost' not found
    /// skipped[1]: notify_team
    /// recovery[1]{tool,params,reason}:
    ///   assign_user,"{\"user\":\"alice\"}",null
    /// ```
    ///
    /// `completed` and `skipped` are inline primitive arrays (§9.1), `failed` a tabular array (§9.3); every string is
    /// quoted per §7.2. Empty categories are omitted.
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = String::with_capacity(
            64 + self.completed.iter().map(|s| s.len() + 1).sum::<usize>()
                + self.failed.iter().map(|f| f.operation.len() + f.reason.len() + 60).sum::<usize>()
                + self.skipped.iter().map(|s| s.len() + 1).sum::<usize>(),
        );
        out.push_str("partial_success: \"");
        out.push_str(&self.completed.len().to_string());
        out.push_str(" completed, ");
        out.push_str(&self.failed.len().to_string());
        out.push_str(" failed, ");
        out.push_str(&self.skipped.len().to_string());
        out.push_str(" skipped\"");

        push_inline_array(&mut out, "completed", &self.completed);

        if !self.failed.is_empty() {
            out.push_str("\nfailed[");
            out.push_str(&self.failed.len().to_string());
            out.push_str("]{operation,reason}:");
            for f in &self.failed {
                out.push_str("\n  ");
                out.push_str(&crate::toon::escape_value(&f.operation));
                out.push(',');
                out.push_str(&crate::toon::escape_value(&f.reason));
            }
        }

        push_inline_array(&mut out, "skipped", &self.skipped);

        let recovery_hints: Vec<crate::recovery::RecoveryHint> =
            self.failed.iter().filter_map(|f| f.recovery.clone()).collect();
        crate::recovery::append_recovery(&mut out, &recovery_hints);

        out
    }
}

/// Append `\n{key}[N]: v1,v2` (a TOON 4.4 inline primitive array, §9.1, each value quoted per §7.2); nothing when
/// `values` is empty.
fn push_inline_array(out: &mut String, key: &str, values: &[String]) {
    if values.is_empty() {
        return;
    }
    out.push('\n');
    out.push_str(key);
    out.push('[');
    out.push_str(&values.len().to_string());
    out.push_str("]: ");
    for (i, v) in values.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&crate::toon::escape_value(v));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kv::KvValue;
    use crate::recovery::RecoveryHint;

    #[test]
    fn partial_success_full_example_matches_spec() {
        let ps = PartialSuccess {
            completed: vec!["create_issue".into(), "add_label".into()],
            failed: vec![FailedOp {
                operation: "assign_user".into(),
                reason: "User 'ghost' not found".into(),
                recovery: Some(RecoveryHint::new("assign_user").param("user", KvValue::Text("alice".into()))),
            }],
            skipped: vec!["notify_team".into()],
        };
        assert_eq!(
            ps.render(),
            "partial_success: \"2 completed, 1 failed, 1 skipped\"\ncompleted[2]: create_issue,add_label\n\
             failed[1]{operation,reason}:\n  assign_user,User 'ghost' not found\nskipped[1]: notify_team\n\
             recovery[1]{tool,params,reason}:\n  assign_user,\"{\\\"user\\\":\\\"alice\\\"}\",null"
        );
    }

    #[test]
    fn partial_success_empty_categories_omitted() {
        let ps = PartialSuccess { completed: vec!["a".into()], failed: vec![], skipped: vec![] };
        let out = ps.render();
        assert!(!out.contains("failed["), "empty failed category must be omitted, got: {out}");
        assert!(!out.contains("skipped["), "empty skipped category must be omitted, got: {out}");
    }

    // AC-032: fully-empty PartialSuccess — exact first line, no section
    // headers at all (not just failed/skipped omitted, as the test above
    // checks — completed[] must also be absent here since it's empty too).
    #[test]
    fn ac032_fully_empty_partial_success_has_exact_summary_and_no_sections() {
        let ps = PartialSuccess { completed: vec![], failed: vec![], skipped: vec![] };
        let out = ps.render();
        assert_eq!(out, "partial_success: \"0 completed, 0 failed, 0 skipped\"");
        assert!(!out.contains("completed["), "got: {out}");
        assert!(!out.contains("failed["), "got: {out}");
        assert!(!out.contains("skipped["), "got: {out}");
    }

    // AC-033: two completed, nothing else — exact first line, exact
    // completed[2] section listing both in order, no failed/skipped sections.
    #[test]
    fn ac033_two_completed_only_has_exact_summary_and_section() {
        let ps = PartialSuccess { completed: vec!["a".into(), "b".into()], failed: vec![], skipped: vec![] };
        let out = ps.render();
        assert_eq!(out, "partial_success: \"2 completed, 0 failed, 0 skipped\"\ncompleted[2]: a,b");
    }

    #[test]
    fn failed_op_without_recovery_produces_no_help_block() {
        let ps = PartialSuccess {
            completed: vec![],
            failed: vec![FailedOp { operation: "x".into(), reason: "y".into(), recovery: None }],
            skipped: vec![],
        };
        assert!(!ps.render().contains("help["));
    }

    #[test]
    fn operation_with_comma_is_escaped() {
        let ps = PartialSuccess {
            completed: vec![],
            failed: vec![FailedOp {
                operation: "create_issue, retry 2".into(),
                reason: "timeout".into(),
                recovery: None,
            }],
            skipped: vec![],
        };
        let out = ps.render();
        // operation contains comma — must be quoted to avoid breaking TOON column parsing
        assert!(out.contains(r#""create_issue, retry 2""#), "operation must be quoted, got: {out}");
    }

    // AC-034: a newline/CR in operation, even with no comma or quote present,
    // triggers quoting and renders as the \n / \r escapes inside the quotes.
    #[test]
    fn ac034_newline_and_cr_in_operation_are_quoted_and_escaped() {
        let ps = PartialSuccess {
            completed: vec![],
            failed: vec![FailedOp { operation: "step\none\rtwo".into(), reason: "y".into(), recovery: None }],
            skipped: vec![],
        };
        let out = ps.render();
        assert!(out.ends_with("\n  \"step\\none\\rtwo\",y"), "got: {out:?}");
        assert!(!out.contains('\r'), "no raw CR may survive, got: {out:?}");
    }

    // AC-035 (superseded): recovery from a failed op appends one trailing recovery table — the same block as
    // render_recovery, not a help[] block, since help is the inline array of strings.
    #[test]
    fn ac035_recovery_hints_render_as_one_recovery_table() {
        let ps = PartialSuccess {
            completed: vec![],
            failed: vec![FailedOp {
                operation: "x".into(),
                reason: "y".into(),
                recovery: Some(RecoveryHint::new("retry_x")),
            }],
            skipped: vec![],
        };
        let out = ps.render();
        let recovery = crate::recovery::render_recovery(&[RecoveryHint::new("retry_x")]);
        assert!(out.ends_with(&format!("\n{recovery}")), "got: {out}");
        assert_eq!(out.matches("recovery[").count(), 1, "got: {out}");
        assert!(!out.contains("help["), "got: {out}");
    }
}
