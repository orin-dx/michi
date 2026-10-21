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

/// Render a help block from a slice of hints: the one-line TOON 4.4 document `help[N]: hint1,hint2`, with no trailing
/// newline.
///
/// The block is a TOON 4.4 inline primitive array (§9.1), the form §1.4 prescribes for an array of strings. Each hint
/// is quoted only when §7.2 requires it — notably when it contains a comma (the delimiter), a colon, a quote, a
/// backslash, a bracket or brace, or a line break, which is written as the escape `\n`/`\r` so a hint never spans
/// lines. Returns an empty string (the empty document) for an empty slice.
#[must_use]
pub fn render_hints(hints: &[Hint]) -> String {
    if hints.is_empty() {
        return String::new();
    }
    let mut out = String::with_capacity(12 + hints.len() * 50);
    append_hints(&mut out, hints);
    out
}

/// Append the help block of [`render_hints`] to a TOON document in place.
///
/// Every michi renderer returns a finished document without a trailing newline, and the append helpers compose
/// them: when `out` is non-empty and does not already end in a newline, one LF is inserted first, and nothing follows
/// the block. The result is a valid TOON document whenever `out` is one without a `help` key. Does nothing for an
/// empty slice.
pub fn append_hints(out: &mut String, hints: &[Hint]) {
    append_help_block(out, hints.iter().map(Hint::as_str));
}

/// Start a new root-level block in a composed document: one LF unless `out` is empty or already ends in one.
pub(crate) fn start_block(out: &mut String) {
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
}

/// Shared writer for every help block: `help[N]: h1,h2` with no trailing newline, each hint encoded per TOON 4.4
/// §7.2.
pub(crate) fn append_help_block<'a, I>(out: &mut String, hints: I)
where
    I: ExactSizeIterator<Item = &'a str>,
{
    if hints.len() == 0 {
        return;
    }
    start_block(out);
    out.push_str("help[");
    out.push_str(&hints.len().to_string());
    out.push_str("]: ");
    for (i, hint) in hints.enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&crate::toon::escape_value(hint));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_hint_renders_correctly() {
        let hints = [Hint::new("Call get_item with id=<id>")];
        assert_eq!(render_hints(&hints), "help[1]: Call get_item with id=<id>");
    }

    #[test]
    fn hints_are_quoted_per_toon_string_rules() {
        let hints = [Hint::new("a, b"), Hint::new("key: v"), Hint::new(""), Hint::new("-x"), Hint::new("plain")];
        assert_eq!(render_hints(&hints), "help[5]: \"a, b\",\"key: v\",\"\",\"-x\",plain");
    }

    #[test]
    fn append_hints_starts_a_new_line_after_a_finished_document() {
        let mut out = "t: []".to_string();
        append_hints(&mut out, &[Hint::new("h")]);
        assert_eq!(out, "t: []\nhelp[1]: h");
    }

    #[test]
    fn empty_hints_returns_empty_string() {
        assert_eq!(render_hints(&[]), "");
    }

    #[test]
    fn multiple_hints_all_appear_in_order() {
        let hints = [Hint::new("first"), Hint::new("second"), Hint::new("third")];
        let out = render_hints(&hints);
        assert_eq!(out, "help[3]: first,second,third");
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
        assert_eq!(out, "help[1]: \"a\\nb\"");
        // The \n renders as a visible escape inside quotes, so the block stays one line.
        assert_eq!(out.lines().count(), 1);
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
        assert_eq!(out, "help[1]: \"a\\rb\"");
        assert!(!out.contains('\r'), "no raw CR may survive, got: {out:?}");
        assert_eq!(out.lines().count(), 1);
    }
}
