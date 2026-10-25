use michi_core::toon::{render_toon, ToonOptions, Value};

#[test]
fn renders_basic_list() {
    let opts = ToonOptions::new(
        "issue",
        vec!["number".into(), "title".into(), "state".into()],
        vec![
            vec![Value::Int(42), Value::Str("Fix login".into()), Value::Str("open".into())],
            vec![Value::Int(43), Value::Str("Add dark mode".into()), Value::Str("open".into())],
        ],
    )
    .total_count(Some(47))
    .hints(vec!["Call get_issue with number=<number> for full detail".to_string()]);

    let out = render_toon(&opts).expect("valid input renders");
    assert_eq!(
        out,
        "issue[2]{number,title,state}:\n  42,Fix login,open\n  43,Add dark mode,open\ntotalCount: 47\nhelp[1]: Call get_issue with number=<number> for full detail"
    );
}

#[test]
fn renders_empty_state() {
    let opts = ToonOptions::new("issue", vec![], vec![])
        .total_count(Some(0))
        .hints(vec!["Try list_issues with a broader filter".to_string()]);
    let out = render_toon(&opts).expect("valid input renders");
    assert_eq!(out, "issue: []\ntotalCount: 0\nhelp[1]: Try list_issues with a broader filter");
}

#[test]
fn escapes_comma_in_value() {
    let opts = ToonOptions::new("item", vec!["name".into()], vec![vec![Value::Str("Update deps, bump major".into())]]);
    let out = render_toon(&opts).expect("valid input renders");
    assert!(out.contains(r#""Update deps, bump major""#));
}

#[test]
fn null_value_renders_as_null_literal() {
    let opts = ToonOptions::new("item", vec!["a".into(), "b".into()], vec![vec![Value::Str("x".into()), Value::Null]]);
    let out = render_toon(&opts).expect("valid input renders");
    assert!(out.ends_with("\n  x,null"), "got: {out:?}");
}

#[test]
fn bool_values_render_as_true_false() {
    let opts = ToonOptions::new(
        "flag",
        vec!["enabled".into(), "visible".into()],
        vec![vec![Value::Bool(true), Value::Bool(false)]],
    );
    let out = render_toon(&opts).expect("valid input renders");
    assert!(out.ends_with("\n  true,false"), "got: {out:?}");
}

#[test]
fn no_total_count_when_none() {
    let opts = ToonOptions::new("item", vec!["id".into()], vec![vec![Value::Int(1)]]);
    let out = render_toon(&opts).expect("valid input renders");
    assert!(!out.contains("totalCount"));
}

#[test]
fn multiple_hints_render_correctly() {
    let opts = ToonOptions::new("item", vec!["id".into()], vec![])
        .total_count(Some(0))
        .hints(vec!["hint one".to_string(), "hint two".to_string()]);
    let out = render_toon(&opts).expect("valid input renders");
    assert!(out.ends_with("\nhelp[2]: hint one,hint two"), "got: {out:?}");
}

#[test]
fn mixed_numeric_row_uses_comma_separators() {
    let opts = ToonOptions::new(
        "point",
        vec!["x".into(), "y".into(), "z".into()],
        vec![vec![Value::Int(-7), Value::Float(2.5), Value::Int(0)]],
    );
    let out = render_toon(&opts).expect("valid input renders");
    assert!(out.ends_with("\n  -7,2.5,0"), "expected comma-separated numeric row, got: {out}");
}

#[test]
#[allow(clippy::approx_constant)]
fn float_value_renders() {
    let opts = ToonOptions::new("measurement", vec!["value".into()], vec![vec![Value::Float(3.14)]]);
    let out = render_toon(&opts).expect("valid input renders");
    assert!(out.contains("3.14"), "expected 3.14 in output, got: {out}");
}

#[test]
fn quote_in_value_is_escaped() {
    let opts = ToonOptions::new("item", vec!["description".into()], vec![vec![Value::Str(r#"say "hello""#.into())]]);
    let out = render_toon(&opts).expect("valid input renders");
    assert!(out.contains(r#""say \"hello\"""#), "expected escaped quotes in output, got: {out}");
}

#[test]
fn newline_in_value_is_quoted_and_escaped() {
    let opts = ToonOptions::new("item", vec!["body".into()], vec![vec![Value::Str("line1\nline2".into())]]);
    let out = render_toon(&opts).expect("valid input renders");
    assert!(out.ends_with("\n  \"line1\\nline2\""), "expected quoted, escaped newline value, got: {out:?}");
    assert_eq!(out.lines().count(), 2, "the escaped newline must not inject a row line, got: {out:?}");
}

#[test]
fn long_cell_value_is_truncated_per_max_cell_len() {
    let long_title = "x".repeat(300);
    let opts = ToonOptions::new("issue", vec!["title".to_string()], vec![vec![Value::Str(long_title.into())]])
        .max_cell_len(50);
    let out = render_toon(&opts).expect("valid input renders");
    // 49 chars are kept and a plain ellipsis makes 50; the 251 cut chars are reported in the `truncated` block.
    let row_line = out.lines().nth(1).expect("row line exists");
    assert_eq!(row_line, format!("  {}\u{2026}", "x".repeat(49)));
    assert_eq!(out.lines().nth(3), Some("  0,title,251"), "got: {out:?}");
}

#[test]
fn short_cell_value_is_not_truncated() {
    let opts = ToonOptions::new("issue", vec!["title".to_string()], vec![vec![Value::Str("short".into())]]);
    let out = render_toon(&opts).expect("valid input renders");
    assert!(out.contains("short"));
    assert!(!out.contains("truncated"));
}

#[test]
fn hints_field_accepts_hint_type() {
    let opts = ToonOptions::new("issue", vec![], vec![]).hints(vec!["do this".to_string()]);
    let out = render_toon(&opts).expect("valid input renders");
    assert!(out.ends_with("\nhelp[1]: do this"), "got: {out:?}");
}

#[test]
fn crate_root_reexports_are_reachable() {
    type KvResult = Result<String, michi_core::KvError>;
    let _: fn(&[michi_core::kv::KvItem], Option<usize>, &[michi_core::Hint]) -> KvResult = michi_core::render_kv;
    let _ = michi_core::empty_state("t");
    let _: fn(Option<String>) -> michi_core::AlreadyDone = michi_core::already_done;
    let _: fn(&str, &str, &[String]) -> String = michi_core::render_already_done;
    let _ = michi_core::RetryConfig::default();
    let _: fn(&str) -> Option<std::time::Duration> = michi_core::parse_retry_after;
    let _: fn(
        &michi_core::resilience::RetryConfig,
        u32,
        f64,
        Option<std::time::Duration>,
    ) -> Option<std::time::Duration> = michi_core::next_retry_delay;
    let _: fn(&str, usize) -> michi_core::Truncated = michi_core::truncate;
    let _ = michi_core::RecoveryHint::new("t");
    let _ = michi_core::StatusResponse::new("t", "d", vec![]);
}

#[test]
#[cfg(feature = "serde")]
fn list_builds_toon_options_from_serializable_struct_slice() {
    #[derive(serde::Serialize)]
    struct Issue {
        number: u64,
        title: String,
        state: String,
    }

    let issues = vec![Issue { number: 51815, title: "[Bug]: Telegram plugin".to_string(), state: "open".to_string() }];
    let out = michi_core::toon::list("issues", &issues).expect("list failed");
    assert!(out.starts_with("issues[1]{number,title,state}:\n"), "got: {out}");
    // The title contains brackets and a colon, so TOON 4.4 §7.2 quotes it.
    assert!(out.contains("51815,\"[Bug]: Telegram plugin\",open"), "got: {out}");
}

#[test]
#[cfg(feature = "serde")]
fn list_handles_empty_slice() {
    #[derive(serde::Serialize)]
    struct Empty {
        x: i32,
    }
    let items: Vec<Empty> = vec![];
    let out = michi_core::toon::list("nothing", &items).expect("list failed");
    // Empty slice: the empty-array form `key: []` (TOON 4.4 §9.1), a single line.
    assert_eq!(out, "nothing: []");
}

#[test]
#[cfg(feature = "serde")]
fn list_stringifies_nested_values_losslessly() {
    #[derive(serde::Serialize)]
    struct WithNested {
        id: u64,
        tags: Vec<String>,
    }
    let items = vec![WithNested { id: 1, tags: vec!["a".to_string(), "b".to_string()] }];
    let out = michi_core::toon::list("t", &items).expect("list failed");
    // Nested array is stringified then quoted (commas inside require quoting).
    // Expected cell: "[\"a\",\"b\"]"
    assert!(out.contains(r#""[\"a\",\"b\"]""#), "nested value must be stringified and quoted, got: {out}");
}

#[test]
fn render_toon_errors_on_row_arity_mismatch() {
    let short = ToonOptions::new("issue", vec!["a".to_string(), "b".to_string()], vec![vec![Value::from("x")]]);
    let err = render_toon(&short).expect_err("a 1-value row against 2 fields must be rejected, not panic");
    assert_eq!(err, michi_core::toon::ToonError::RowLengthMismatch { row_index: 0, expected: 2, actual: 1 });

    let long = ToonOptions::new(
        "issue",
        vec!["a".to_string(), "b".to_string()],
        vec![vec![Value::from("x"), Value::from("y"), Value::from("z")]],
    );
    let err =
        render_toon(&long).expect_err("a 3-value row against 2 fields must be rejected, no value dropped silently");
    assert_eq!(err, michi_core::toon::ToonError::RowLengthMismatch { row_index: 0, expected: 2, actual: 3 });
}

#[test]
fn render_toon_is_profile_invariant() {
    let valid = ToonOptions::new(
        "issue",
        vec!["a".to_string(), "b".to_string()],
        vec![vec![Value::from("x"), Value::from("y")]],
    );
    assert_eq!(render_toon(&valid).unwrap(), "issue[1]{a,b}:\n  x,y");

    let short = ToonOptions::new("issue", vec!["a".to_string(), "b".to_string()], vec![vec![Value::from("x")]]);
    assert_eq!(
        render_toon(&short).unwrap_err(),
        michi_core::toon::ToonError::RowLengthMismatch { row_index: 0, expected: 2, actual: 1 }
    );

    let long = ToonOptions::new("issue", vec!["a".to_string()], vec![vec![Value::from("x"), Value::from("y")]]);
    assert_eq!(
        render_toon(&long).unwrap_err(),
        michi_core::toon::ToonError::RowLengthMismatch { row_index: 0, expected: 1, actual: 2 }
    );

    let bad_type = ToonOptions::new("help", vec!["x".to_string()], vec![vec![Value::from("v")]]);
    assert_eq!(
        render_toon(&bad_type).unwrap_err(),
        michi_core::toon::ToonError::InvalidTypeName { name: "help".to_string() }
    );

    let bad_field = ToonOptions::new("t", vec!["a".to_string(), "a".to_string()], vec![vec![Value::from("v"); 2]]);
    assert_eq!(
        render_toon(&bad_field).unwrap_err(),
        michi_core::toon::ToonError::InvalidFieldName { name: "a".to_string() }
    );
}

#[test]
fn all_toon_rendering_goes_through_toon_document() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    // Only the free function `render_toon(&ToonOptions) -> Result<String, ToonError>`
    // is in scope here -- its call sites always look like `render_toon(&x)`.
    // This deliberately excludes: its own `fn render_toon(` definitions (in
    // src/toon/mod.rs and src/napi.rs -- the latter a different, NAPI-layer
    // `render_toon` that has always returned `napi::Result<String>`, unaffected
    // by this spec); `AgentResponse::render_toon(&self) -> String`, a distinct,
    // deliberately-infallible method (its own call sites are `.render_toon()`,
    // with no `&`-prefixed argument); and prose in comments/string literals
    // that never happens to contain the literal substring `render_toon(&`.
    // Built via concat() rather than a literal so this test's own source
    // doesn't self-match its own trigger/definition-marker strings.
    let trigger = ["render_toon", "(&"].concat();
    let definition_marker = ["fn ", "render_toon", "("].concat();
    let bound_markers = [
        "?",
        ".map(",
        ".map_err(",
        ".is_err(",
        ".is_ok(",
        ".expect(",
        ".expect_err(",
        ".unwrap(",
        ".unwrap_err(",
        "match ",
        "if let ",
        "assert_eq!",
        "assert_ne!",
    ];
    let mut violations = Vec::new();
    for dir in ["crates", "packages/michi-node/src"] {
        visit_rs_files(&root.join(dir), &mut |path, contents| {
            // tests/ui-fail/*.rs are trybuild compile-fail fixtures: their whole
            // point is to contain code that must NOT compile as written
            // (e.g. AC-004b's render_toon_is_fallible.rs deliberately binds
            // render_toon's Result to a bare String), so they are exempt.
            if path.components().any(|c| c.as_os_str() == "ui-fail") {
                return;
            }
            for (i, line) in contents.lines().enumerate() {
                let trimmed = line.trim_start();
                if trimmed.starts_with("//") {
                    continue;
                }
                if line.contains(trigger.as_str())
                    && !line.contains(definition_marker.as_str())
                    && !bound_markers.iter().any(|m| line.contains(m))
                {
                    violations.push(format!("{}:{}: {}", path.display(), i + 1, line.trim()));
                }
            }
        });
    }
    assert!(violations.is_empty(), "unbound render_toon call site(s) found:\n{}", violations.join("\n"));
}

fn visit_rs_files(dir: &std::path::Path, f: &mut impl FnMut(&std::path::Path, &str)) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().and_then(|n| n.to_str()) == Some("target") {
                continue;
            }
            visit_rs_files(&path, f);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            if let Ok(contents) = std::fs::read_to_string(&path) {
                f(&path, &contents);
            }
        }
    }
}
