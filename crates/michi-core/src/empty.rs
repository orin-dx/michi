use crate::hints::Hint;
use crate::toon::ToonError;

/// Render a definitive empty state response: the TOON 4.4 document `type_name: []` followed by `totalCount: 0`, with
/// no trailing newline. `type_name` is quoted when TOON's key rules (§7.3) require it.
///
/// A reserved root key (`totalCount`, `help` or `recovery`) as `type_name` would repeat that key in the document,
/// which TOON 4.4 §14.3 forbids. This function returns a `String`, so for those names it renders the
/// `toon_validation_failed` error block that `AgentResponse` uses (`error: toon_validation_failed` plus a `message`)
/// instead of invalid TOON. Call [`try_empty_state`] to get the [`ToonError`] instead.
#[must_use]
pub fn empty_state(type_name: &str) -> String {
    try_empty_state(type_name).unwrap_or_else(|e| validation_error_block(&e))
}

/// Render a definitive empty state like [`empty_state`], or reject a reserved `type_name` the way
/// [`crate::toon::render_toon`] does.
///
/// # Errors
///
/// [`ToonError::InvalidTypeName`] when `type_name` is `totalCount`, `help` or `recovery`.
pub fn try_empty_state(type_name: &str) -> Result<String, ToonError> {
    if crate::kv::RESERVED_ROOT_KEYS.contains(&type_name) {
        return Err(ToonError::InvalidTypeName { name: type_name.to_string() });
    }
    let key = crate::toon::escape_key(type_name);
    let mut out = String::with_capacity(key.len() + EMPTY_TAIL.len());
    out.push_str(&key);
    out.push_str(EMPTY_TAIL);
    Ok(out)
}

const EMPTY_TAIL: &str = ": []\ntotalCount: 0";

/// Render a definitive empty state with contextual usage hints: [`empty_state`] followed by the inline
/// `help[N]: …` array, with no trailing newline.
///
/// For a reserved `type_name` it renders the same `toon_validation_failed` error block as [`empty_state`], without
/// the hints. Call [`try_empty_state_with_hints`] to get the [`ToonError`] instead.
#[must_use]
pub fn empty_state_with_hints(type_name: &str, hints: &[Hint]) -> String {
    try_empty_state_with_hints(type_name, hints).unwrap_or_else(|e| validation_error_block(&e))
}

/// Render a definitive empty state with hints like [`empty_state_with_hints`], or reject a reserved `type_name`.
///
/// # Errors
///
/// [`ToonError::InvalidTypeName`] when `type_name` is `totalCount`, `help` or `recovery`.
pub fn try_empty_state_with_hints(type_name: &str, hints: &[Hint]) -> Result<String, ToonError> {
    let mut out = try_empty_state(type_name)?;
    crate::hints::append_hints(&mut out, hints);
    Ok(out)
}

/// The `error: toon_validation_failed` + `message: …` block `AgentResponse` renders for a TOON validation failure.
fn validation_error_block(e: &ToonError) -> String {
    let text = e.to_string();
    let message = crate::toon::escape_value(&text);
    let mut out = String::with_capacity(39 + message.len());
    out.push_str("error: toon_validation_failed\nmessage: ");
    out.push_str(&message);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_empty_state() {
        assert_eq!(empty_state("issue"), "issue: []\ntotalCount: 0");
    }

    #[test]
    fn ac003_empty_type_name_does_not_panic() {
        // The empty key is quoted (TOON 4.4 §7.3).
        assert_eq!(empty_state(""), "\"\": []\ntotalCount: 0");
    }

    #[test]
    fn ac003a_newline_in_type_name_is_escaped() {
        let out = empty_state("a\nb");
        assert_eq!(out, "\"a\\nb\": []\ntotalCount: 0");
        assert_eq!(out.lines().count(), 2, "the escaped \\n must not inject a line");
    }

    #[test]
    fn carriage_return_in_type_name_is_escaped() {
        let out = empty_state("a\rb");
        assert_eq!(out, "\"a\\rb\": []\ntotalCount: 0");
        assert!(!out.contains('\r'), "no raw CR may survive, got: {out:?}");
        assert_eq!(out.lines().count(), 2);
    }

    #[test]
    fn ac004_capacity_is_exact() {
        for (name, key_len) in [("issue", 5), ("my-type", 9)] {
            let out = empty_state(name);
            assert_eq!(out.len(), key_len + EMPTY_TAIL.len(), "{out:?}");
            assert_eq!(out.capacity(), out.len(), "capacity {} not exact -- reallocation occurred", out.capacity());
        }
    }

    #[test]
    fn ac005_with_hints_appends_the_help_line_after_a_newline() {
        use crate::hints::Hint;
        let hints = [Hint::new("first"), Hint::new("second")];
        let out = empty_state_with_hints("issue", &hints);
        assert_eq!(out, "issue: []\ntotalCount: 0\nhelp[2]: first,second");
        let base = empty_state("issue");
        assert_eq!(&out[base.len() + 1..], crate::hints::render_hints(&hints));
    }

    // A reserved root key as the type name would repeat that key in the document (TOON 4.4 §14.3), so the fallible
    // functions reject it with the same error `render_toon` uses.
    #[test]
    fn try_empty_state_rejects_reserved_type_names() {
        for name in ["totalCount", "help", "recovery"] {
            assert_eq!(try_empty_state(name), Err(ToonError::InvalidTypeName { name: name.to_string() }));
            assert_eq!(
                try_empty_state_with_hints(name, &[Hint::new("x")]),
                Err(ToonError::InvalidTypeName { name: name.to_string() })
            );
        }
        assert_eq!(try_empty_state("issue").as_deref(), Ok("issue: []\ntotalCount: 0"));
        assert_eq!(
            try_empty_state_with_hints("issue", &[Hint::new("a")]).as_deref(),
            Ok("issue: []\ntotalCount: 0\nhelp[1]: a")
        );
    }

    // The infallible functions never emit a duplicate key: a reserved name renders the `toon_validation_failed` error
    // block `AgentResponse` uses, so the failure is visible in the text instead of producing invalid TOON.
    #[test]
    fn empty_state_renders_an_error_block_for_reserved_type_names() {
        for name in ["totalCount", "help", "recovery"] {
            let expected = format!(
                "error: toon_validation_failed\nmessage: \"type_name \\\"{name}\\\" is a reserved document key (totalCount, help, recovery)\""
            );
            assert_eq!(empty_state(name), expected, "{name}");
            assert_eq!(empty_state_with_hints(name, &[Hint::new("x")]), expected, "{name}");
        }
    }

    #[test]
    fn ac006_with_hints_and_empty_slice_equals_empty_state() {
        let out = empty_state_with_hints("issue", &[]);
        assert_eq!(out, empty_state("issue"));
    }
}
