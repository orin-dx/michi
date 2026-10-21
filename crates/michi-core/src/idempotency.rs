/// A single operation that failed within a larger multi-step operation.
#[derive(Debug, Clone, PartialEq)]
pub struct FailedOp {
    /// Identifier of the operation that failed.
    pub operation: String,
    /// Human-readable failure reason.
    pub reason: String,
    /// Optional structured recovery hint for this specific failure.
    pub recovery: Option<crate::recovery::RecoveryHint>,
}

/// Signals that an operation partially completed before a failure.
///
/// Use this when some steps of a multi-step operation succeeded — the agent
/// can resume from the checkpoint rather than retrying from scratch.
///
/// For a fully-completed prior call, see [`crate::resilience::AlreadyDone`]
/// and [`crate::resilience::already_done`].
#[derive(Debug, Clone, PartialEq)]
pub struct PartialSuccess {
    /// Identifiers of steps that completed successfully.
    pub completed: Vec<String>,
    /// Steps that failed, with reason and optional recovery hint.
    pub failed: Vec<FailedOp>,
    /// Identifiers of steps that were not attempted.
    pub skipped: Vec<String>,
}

impl PartialSuccess {
    /// Render as an agent-readable string: a summary line, one block per
    /// non-empty outcome category, then any per-op recovery hints folded into
    /// a trailing `help[]` block.
    ///
    /// Format:
    /// ```text
    /// partial_success: 2 completed, 1 failed, 1 skipped
    /// completed[2]:
    ///   create_issue
    ///   add_label
    /// failed[1]{operation,reason}:
    ///   assign_user,User 'ghost' not found
    /// skipped[1]:
    ///   notify_team
    /// help[1]:
    ///   assign_user: suggestedParams: { user: alice }
    /// ```
    ///
    /// Empty categories are omitted.
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = String::with_capacity(
            64 + self.completed.iter().map(String::len).sum::<usize>()
                + self.failed.iter().map(|f| f.operation.len() + f.reason.len() + 60).sum::<usize>()
                + self.skipped.iter().map(String::len).sum::<usize>(),
        );
        out.push_str("partial_success: ");
        out.push_str(&self.completed.len().to_string());
        out.push_str(" completed, ");
        out.push_str(&self.failed.len().to_string());
        out.push_str(" failed, ");
        out.push_str(&self.skipped.len().to_string());
        out.push_str(" skipped\n");

        if !self.completed.is_empty() {
            out.push_str("completed[");
            out.push_str(&self.completed.len().to_string());
            out.push_str("]:\n");
            for op in &self.completed {
                out.push_str("  ");
                out.push_str(&crate::text::escape_line_breaks(op));
                out.push('\n');
            }
        }

        if !self.failed.is_empty() {
            out.push_str("failed[");
            out.push_str(&self.failed.len().to_string());
            out.push_str("]{operation,reason}:\n");
            for f in &self.failed {
                out.push_str("  ");
                // Both cells follow TOON 4.4 §7.2: quoted only when required (delimiter, colon, quote, leading
                // hyphen, line break, …), escaped per §7.1.
                out.push_str(&crate::toon::escape_value(&f.operation));
                out.push(',');
                out.push_str(&crate::toon::escape_value(&f.reason));
                out.push('\n');
            }
        }

        if !self.skipped.is_empty() {
            out.push_str("skipped[");
            out.push_str(&self.skipped.len().to_string());
            out.push_str("]:\n");
            for op in &self.skipped {
                out.push_str("  ");
                out.push_str(&crate::text::escape_line_breaks(op));
                out.push('\n');
            }
        }

        let recovery_hints: Vec<crate::recovery::RecoveryHint> =
            self.failed.iter().filter_map(|f| f.recovery.clone()).collect();
        if !recovery_hints.is_empty() {
            out.push_str("help[");
            out.push_str(&recovery_hints.len().to_string());
            out.push_str("]:\n");
            // Use inner append_recovery_lines (pub(crate)) to avoid the `recovery[N]:` header
            // that append_recovery would write — the section header here is `help[N]:`.
            crate::recovery::append_recovery_lines(&mut out, &recovery_hints);
        }

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
        let out = ps.render();
        assert!(out.starts_with("partial_success: 2 completed, 1 failed, 1 skipped\n"), "got: {out}");
        assert!(out.contains("completed[2]:\n  create_issue\n  add_label\n"), "got: {out}");
        assert!(out.contains("failed[1]{operation,reason}:\n  assign_user,User 'ghost' not found\n"), "got: {out}");
        assert!(out.contains("skipped[1]:\n  notify_team\n"), "got: {out}");
        assert!(out.contains("help[1]:"), "got: {out}");
        assert!(out.contains("assign_user: suggestedParams: { user: alice }"), "got: {out}");
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
        assert_eq!(out, "partial_success: 0 completed, 0 failed, 0 skipped\n");
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
        assert_eq!(out, "partial_success: 2 completed, 0 failed, 0 skipped\ncompleted[2]:\n  a\n  b\n");
    }

    #[test]
    fn partial_success_exit_code_zero_when_no_failures() {
        let ps = PartialSuccess { completed: vec!["a".into()], failed: vec![], skipped: vec!["b".into()] };
        assert_eq!(ps.exit_code(), 0);
    }

    // AC-036: exit_code()==0 holds for the fully-empty combination too, not
    // just the completed+skipped-both-nonempty combination above.
    #[test]
    fn ac036_exit_code_zero_holds_when_completed_and_skipped_are_also_empty() {
        let ps = PartialSuccess { completed: vec![], failed: vec![], skipped: vec![] };
        assert_eq!(ps.exit_code(), 0);
    }

    #[test]
    fn partial_success_exit_code_one_when_any_failed() {
        let ps = PartialSuccess {
            completed: vec![],
            failed: vec![FailedOp { operation: "x".into(), reason: "y".into(), recovery: None }],
            skipped: vec![],
        };
        assert_eq!(ps.exit_code(), 1);
    }

    // AC-037: exit_code()==1 regardless of how many entries failed contains
    // — the existing test above only proves the 1-entry case.
    #[test]
    fn ac037_exit_code_one_holds_for_five_failed_entries() {
        let ps = PartialSuccess {
            completed: vec![],
            failed: (0..5)
                .map(|i| FailedOp { operation: format!("op{i}"), reason: "y".into(), recovery: None })
                .collect(),
            skipped: vec![],
        };
        assert_eq!(ps.exit_code(), 1);
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
        assert!(out.contains("  \"step\\none\\rtwo\",y\n"), "got: {out:?}");
        assert!(!out.contains('\r'), "no raw CR may survive, got: {out:?}");
    }

    // AC-035: recovery from a failed op appends a trailing help[] block with
    // no duplicate leading "recovery[N]:" header line.
    #[test]
    fn ac035_recovery_help_block_has_no_duplicate_recovery_header() {
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
        assert!(out.contains("help[1]:\n  retry_x\n"), "got: {out}");
        assert!(!out.contains("recovery["), "must not duplicate a recovery[N]: header, got: {out}");
    }
}
