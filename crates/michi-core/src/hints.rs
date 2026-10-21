/// A contextual usage hint for an agent.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Hint(pub String);

impl Hint {
    /// Create a hint from any string-like value.
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// The raw hint string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for Hint {
    fn from(s: &str) -> Self {
        Self(s.into())
    }
}

impl From<String> for Hint {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl std::fmt::Display for Hint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Render a `help[N]:` block from a slice of hints.
#[must_use]
pub fn render_hints(hints: &[Hint]) -> String {
    if hints.is_empty() {
        return String::new();
    }
    let mut out = String::with_capacity(12 + hints.len() * 50);
    append_hints(&mut out, hints);
    out
}

/// Append a `help[N]:` block to an existing string in-place.
pub fn append_hints(out: &mut String, hints: &[Hint]) {
    if hints.is_empty() {
        return;
    }
    out.push_str("help[");
    out.push_str(&hints.len().to_string());
    out.push_str("]:\n");
    for hint in hints {
        out.push_str("  ");
        out.push_str(&crate::text::escape_line_breaks(hint.as_str()));
        out.push('\n');
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_hint_renders_correctly() {
        let hints = [Hint::new("Call get_item with id=<id>")];
        assert_eq!(render_hints(&hints), "help[1]:\n  Call get_item with id=<id>\n");
    }

    #[test]
    fn empty_hints_returns_empty_string() {
        assert_eq!(render_hints(&[]), "");
    }

    #[test]
    fn multiple_hints_all_appear_in_order() {
        let hints = [Hint::new("first"), Hint::new("second"), Hint::new("third")];
        let out = render_hints(&hints);
        assert_eq!(out, "help[3]:\n  first\n  second\n  third\n");
    }

    #[test]
    fn append_hints_on_empty_slice_does_nothing() {
        let mut buf = "prefix\n".to_string();
        append_hints(&mut buf, &[]);
        assert_eq!(buf, "prefix\n");
    }

    #[test]
    fn ac010_append_hints_on_empty_slice_leaves_initially_empty_string_empty() {
        let mut buf = String::new();
        append_hints(&mut buf, &[]);
        assert_eq!(buf, "");
    }

    #[test]
    fn ac007_constructors_and_display_mirror_as_str() {
        assert_eq!(Hint::new("text").as_str(), "text");
        assert_eq!(Hint::from("text").as_str(), "text");
        assert_eq!(Hint::from(String::from("text")).as_str(), "text");
        let hint = Hint::new("display me");
        assert_eq!(format!("{hint}"), hint.as_str());
    }

    #[test]
    fn ac011_append_hints_appends_render_hints_output_in_place() {
        let hints = [Hint::new("a"), Hint::new("b")];
        let mut out = "prefix\n".to_string();
        append_hints(&mut out, &hints);
        assert_eq!(out, format!("prefix\n{}", render_hints(&hints)));
    }

    #[test]
    fn ac011a_newline_in_hint_is_escaped() {
        let hints = [Hint::new("a\nb")];
        let out = render_hints(&hints);
        assert_eq!(out, "help[1]:\n  a\\nb\n");
        // The \n renders as a visible escape, so line count stays at help[N]'s N+1.
        assert_eq!(out.lines().count(), 2);
    }

    #[test]
    fn ac004a_render_hints_capacity_is_at_least_12_plus_len_times_50() {
        let hints = [Hint::new("a"), Hint::new("b")];
        let out = render_hints(&hints);
        assert!(out.capacity() >= 12 + hints.len() * 50, "capacity {} too small", out.capacity());
    }

    #[test]
    #[cfg(feature = "serde")]
    fn ac037a_serializes_as_bare_json_string() {
        assert_eq!(serde_json::to_string(&Hint::from("x")).unwrap(), "\"x\"");
    }

    #[test]
    fn carriage_return_in_hint_is_escaped() {
        // \r renders as the visible two-character escape, never raw or dropped.
        let hints = [Hint::new("a\rb")];
        let out = render_hints(&hints);
        assert_eq!(out, "help[1]:\n  a\\rb\n");
        assert!(!out.contains('\r'), "no raw CR may survive, got: {out:?}");
        assert_eq!(out.lines().count(), 2);
    }
}
