use std::fmt::Write as _;

/// Status of an individual pipeline step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum StepStatus {
    /// Step completed successfully.
    Completed,
    /// Step was skipped.
    Skipped,
    /// Step failed.
    Failed,
    /// Step has not been attempted yet.
    Pending,
}

impl StepStatus {
    /// The short label string used in rendered output.
    #[must_use]
    pub fn label(&self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Skipped => "skipped",
            Self::Failed => "failed",
            Self::Pending => "pending",
        }
    }
}

/// A pipeline step definition.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct PipelineStep {
    /// Step identifier.
    ///
    /// michi does not validate uniqueness — callers are responsible for ensuring
    /// step IDs are distinct within a pipeline if they intend to reference steps
    /// by ID. Duplicate IDs render without error but produce ambiguous output.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Current status.
    pub status: StepStatus,
}

/// A pipeline run state.
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Pipeline {
    /// Pipeline identifier.
    pub id: String,
    /// All steps in declaration order.
    pub steps: Vec<PipelineStep>,
}

impl Pipeline {
    /// Render the pipeline state as a TOON-style list.
    #[must_use]
    pub fn render(&self) -> String {
        let n = self.steps.len();
        let capacity = 30 + n * 40;
        let mut out = String::with_capacity(capacity);

        out.push_str("step[");
        let _ = write!(out, "{n}");
        out.push_str("]{id,name,status}:\n");

        for step in &self.steps {
            out.push_str("  ");
            out.push_str(&crate::toon::escape_value(&step.id));
            out.push(',');
            out.push_str(&crate::toon::escape_value(&step.name));
            out.push(',');
            out.push_str(step.status.label());
            out.push('\n');
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_pipeline_steps() {
        let p = Pipeline {
            id: "my-pipeline".into(),
            steps: vec![
                PipelineStep { id: "fetch".into(), name: "Fetch Data".into(), status: StepStatus::Completed },
                PipelineStep { id: "upload".into(), name: "Upload".into(), status: StepStatus::Pending },
            ],
        };
        let out = p.render();
        assert_eq!(out, "step[2]{id,name,status}:\n  fetch,Fetch Data,completed\n  upload,Upload,pending\n");
    }

    #[test]
    fn empty_pipeline_renders_zero_steps() {
        let p = Pipeline { id: "empty".into(), steps: vec![] };
        let out = p.render();
        assert_eq!(out, "step[0]{id,name,status}:\n");
    }

    #[test]
    fn failed_step_label() {
        let p = Pipeline {
            id: "p".into(),
            steps: vec![PipelineStep { id: "build".into(), name: "Build".into(), status: StepStatus::Failed }],
        };
        assert!(p.render().contains(",failed\n"), "got: {}", p.render());
    }

    #[test]
    fn skipped_step_label() {
        let p = Pipeline {
            id: "p".into(),
            steps: vec![PipelineStep { id: "lint".into(), name: "Lint".into(), status: StepStatus::Skipped }],
        };
        assert!(p.render().contains(",skipped\n"), "got: {}", p.render());
    }

    #[test]
    fn step_id_with_comma_is_toon_escaped() {
        let p = Pipeline {
            id: "p".into(),
            steps: vec![PipelineStep { id: "a,b".into(), name: "A,B Step".into(), status: StepStatus::Pending }],
        };
        let out = p.render();
        // Comma in id/name must be escaped so TOON field separation is preserved
        assert!(!out.contains("a,b,A,B"), "raw commas must not appear unescaped, got: {out}");
    }

    // AC-001: an exhaustive match with exactly these 4 arms and no wildcard
    // compiles today; adding a 5th variant would break this (non_exhaustive
    // only blocks exhaustive matching from OUTSIDE the crate — see AC-003's
    // trybuild fixture for that half).
    #[test]
    fn ac001_step_status_has_exactly_four_variants() {
        fn all_variants(s: StepStatus) -> &'static str {
            match s {
                StepStatus::Completed => "completed",
                StepStatus::Skipped => "skipped",
                StepStatus::Failed => "failed",
                StepStatus::Pending => "pending",
            }
        }
        assert_eq!(all_variants(StepStatus::Completed), "completed");
    }

    #[test]
    fn ac002_step_status_is_copy_and_comparable() {
        let a = StepStatus::Completed;
        let b = a;
        assert_eq!(a, b, "must still be usable after copy");
        assert_eq!(StepStatus::Completed, StepStatus::Completed);
        assert_ne!(StepStatus::Completed, StepStatus::Failed);
    }

    #[test]
    fn ac004_completed_label_is_exact() {
        assert_eq!(StepStatus::Completed.label(), "completed");
    }

    #[test]
    fn ac005_skipped_label_is_exact() {
        assert_eq!(StepStatus::Skipped.label(), "skipped");
    }

    #[test]
    fn ac006_failed_label_is_exact() {
        assert_eq!(StepStatus::Failed.label(), "failed");
    }

    #[test]
    fn ac007_pending_label_is_exact() {
        assert_eq!(StepStatus::Pending.label(), "pending");
    }

    #[test]
    fn ac010_pipeline_default_has_empty_id_and_steps() {
        let p = Pipeline::default();
        assert_eq!(p.id, "");
        assert_eq!(p.steps.len(), 0);
    }

    // AC-013: a distinctive, non-overlapping id must not leak into render()
    // output for an empty-steps pipeline.
    #[test]
    fn ac013_distinctive_id_never_appears_in_empty_render_output() {
        let p = Pipeline { id: "zzz-unique-marker-999".into(), steps: vec![] };
        let out = p.render();
        assert_eq!(out, "step[0]{id,name,status}:\n");
        assert!(!out.contains("zzz-unique-marker-999"), "got: {out}");
    }

    // AC-014: the existing step_id_with_comma_is_toon_escaped test above only
    // checks a negative substring, not the exact quoted output.
    #[test]
    fn ac014_comma_in_id_and_name_renders_exact_quoted_literal() {
        let p = Pipeline {
            id: "p".into(),
            steps: vec![PipelineStep { id: "a,b".into(), name: "A,B Step".into(), status: StepStatus::Pending }],
        };
        assert_eq!(p.render(), "step[1]{id,name,status}:\n  \"a,b\",\"A,B Step\",pending\n");
    }

    #[test]
    fn ac017_render_is_idempotent_and_does_not_mutate() {
        let p = Pipeline {
            id: "p".into(),
            steps: vec![PipelineStep { id: "s".into(), name: "n".into(), status: StepStatus::Completed }],
        };
        let first = p.render();
        let second = p.render();
        assert_eq!(first, second);
        assert_eq!(p.id, "p");
        assert_eq!(p.steps[0].id, "s");
        assert_eq!(p.steps[0].name, "n");
        assert_eq!(p.steps[0].status, StepStatus::Completed);
    }

    #[test]
    fn ac018_step_order_is_preserved_not_reordered() {
        let p = Pipeline {
            id: "p".into(),
            steps: vec![
                PipelineStep { id: "C".into(), name: "c".into(), status: StepStatus::Completed },
                PipelineStep { id: "A".into(), name: "a".into(), status: StepStatus::Completed },
                PipelineStep { id: "B".into(), name: "b".into(), status: StepStatus::Completed },
            ],
        };
        assert_eq!(p.render(), "step[3]{id,name,status}:\n  C,c,completed\n  A,a,completed\n  B,b,completed\n");
    }

    #[test]
    fn ac019_duplicate_ids_render_without_error_or_dedup() {
        let p = Pipeline {
            id: "p".into(),
            steps: vec![
                PipelineStep { id: "dup".into(), name: "first".into(), status: StepStatus::Completed },
                PipelineStep { id: "dup".into(), name: "second".into(), status: StepStatus::Pending },
            ],
        };
        assert_eq!(p.render(), "step[2]{id,name,status}:\n  dup,first,completed\n  dup,second,pending\n");
    }

    #[test]
    fn ac020_empty_id_and_name_render_without_panic() {
        let p = Pipeline {
            id: "p".into(),
            steps: vec![PipelineStep { id: String::new(), name: String::new(), status: StepStatus::Pending }],
        };
        assert_eq!(p.render(), "step[1]{id,name,status}:\n  ,,pending\n");
    }

    #[test]
    fn ac025_embedded_newline_in_id_is_quoted_and_escaped() {
        let id: String = ['l', 'i', 'n', 'e', '1', '\n', 'l', 'i', 'n', 'e', '2'].into_iter().collect();
        let p = Pipeline {
            id: "p".into(),
            steps: vec![PipelineStep { id, name: "ok".into(), status: StepStatus::Pending }],
        };
        assert_eq!(p.render(), "step[1]{id,name,status}:\n  \"line1\\nline2\",ok,pending\n");
    }

    // AC-025's own text says "id or name" interchangeably, but every fixture
    // in this spec's evidence only ever put a newline/CR/quote into id --
    // name only ever appears with a comma (AC-014). escape_value() is called
    // identically on both fields in render(), so this closes that
    // family-sibling gap without a new criterion ID.
    #[test]
    fn ac025_embedded_newline_in_name_is_also_quoted_and_escaped() {
        let name: String = ['l', 'i', 'n', 'e', '1', '\n', 'l', 'i', 'n', 'e', '2'].into_iter().collect();
        let p = Pipeline {
            id: "p".into(),
            steps: vec![PipelineStep { id: "ok".into(), name, status: StepStatus::Pending }],
        };
        assert_eq!(p.render(), "step[1]{id,name,status}:\n  ok,\"line1\\nline2\",pending\n");
    }

    #[test]
    fn ac026_embedded_quote_in_id_is_escaped_within_quoted_field() {
        let id: String = ['a', '"', 'b'].into_iter().collect();
        let p = Pipeline {
            id: "p".into(),
            steps: vec![PipelineStep { id, name: "c".into(), status: StepStatus::Pending }],
        };
        assert_eq!(p.render(), "step[1]{id,name,status}:\n  \"a\\\"b\",c,pending\n");
    }

    #[test]
    fn ac027_clone_is_deep_and_independent() {
        let p = Pipeline {
            id: "orig".into(),
            steps: vec![PipelineStep { id: "s".into(), name: "n".into(), status: StepStatus::Completed }],
        };
        let mut q = p.clone();
        assert_eq!(q.id, "orig");
        assert_eq!(q.steps.len(), 1);
        assert_eq!(q.steps[0].id, "s");
        assert_eq!(q.steps[0].name, "n");
        assert_eq!(q.steps[0].status, StepStatus::Completed);

        q.id = "changed".to_string();
        assert_eq!(p.id, "orig", "mutating the clone must not affect the original");
    }

    #[test]
    fn ac028_debug_output_is_exact() {
        assert_eq!(format!("{:?}", StepStatus::Completed), "Completed");
        assert_eq!(
            format!("{:?}", PipelineStep { id: "s".into(), name: "n".into(), status: StepStatus::Failed }),
            "PipelineStep { id: \"s\", name: \"n\", status: Failed }"
        );
        assert_eq!(format!("{:?}", Pipeline::default()), "Pipeline { id: \"\", steps: [] }");
    }

    #[test]
    fn ac029_step_status_satisfies_eq_bound() {
        fn requires_eq<T: Eq>() {}
        requires_eq::<StepStatus>();
    }

    #[test]
    fn ac030_embedded_carriage_return_in_id_is_quoted_and_escaped() {
        let id: String = ['l', 'i', 'n', 'e', '1', '\r', 'l', 'i', 'n', 'e', '2'].into_iter().collect();
        let p = Pipeline {
            id: "p".into(),
            steps: vec![PipelineStep { id, name: "ok".into(), status: StepStatus::Pending }],
        };
        assert_eq!(p.render(), "step[1]{id,name,status}:\n  \"line1\\rline2\",ok,pending\n");
    }
}
