//! Escaping shared by every renderer that writes caller text into line-oriented output.

use std::borrow::Cow;

/// Escape CR and LF as the two-character sequences `\r` and `\n`, so caller text can never start a new line and fake
/// an extra field. Everything else, including backslashes, passes through unchanged.
#[must_use]
pub(crate) fn escape_line_breaks(s: &str) -> Cow<'_, str> {
    if !s.contains(['\n', '\r']) {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len() + 8);
    for ch in s.chars() {
        match ch {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            _ => out.push(ch),
        }
    }
    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_without_line_breaks_is_borrowed() {
        assert!(matches!(escape_line_breaks("plain, text"), Cow::Borrowed("plain, text")));
    }

    #[test]
    fn cr_and_lf_become_visible_escapes() {
        assert_eq!(escape_line_breaks("a\r\nb\nc"), r"a\r\nb\nc");
    }
}
