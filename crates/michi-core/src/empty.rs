use crate::hints::Hint;

/// Render a definitive empty state response: the TOON 4.4 document `type_name: []` followed by `totalCount: 0`, with
/// no trailing newline. `type_name` is quoted when TOON's key rules (§7.3) require it.
#[must_use]
pub fn empty_state(type_name: &str) -> String {
    let key = crate::toon::escape_key(type_name);
    let mut out = String::with_capacity(key.len() + EMPTY_TAIL.len());
    out.push_str(&key);
    out.push_str(EMPTY_TAIL);
    out
}

const EMPTY_TAIL: &str = ": []\ntotalCount: 0";

/// Render a definitive empty state with contextual usage hints: [`empty_state`] followed by the inline
/// `help[N]: …` array, with no trailing newline.
#[must_use]
pub fn empty_state_with_hints(type_name: &str, hints: &[Hint]) -> String {
    let mut out = empty_state(type_name);
    crate::hints::append_hints(&mut out, hints);
    if out.ends_with('\n') {
        out.pop();
    }
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
        assert_eq!(format!("{}\n", &out[base.len() + 1..]), crate::hints::render_hints(&hints));
    }

    #[test]
    fn ac006_with_hints_and_empty_slice_equals_empty_state() {
        let out = empty_state_with_hints("issue", &[]);
        assert_eq!(out, empty_state("issue"));
    }
}
