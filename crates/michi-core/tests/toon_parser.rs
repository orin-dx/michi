//! Unit tests for the shared test-only TOON parser at
//! `tests/support/toon_parser.rs` (see that file for what it's for and why
//! it lives under `tests/support/`).

mod support;
use support::toon_parser::parse;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_document() {
        let input = "issues[2]{number,title,state}:\n  42,Fix login redirect,open\n  43,Add dark mode,open\ntotalCount: 47\nhelp[1]:\n  Call get_issue\n";
        let parsed = parse(input).unwrap();
        assert_eq!(parsed.type_name, "issues");
        assert_eq!(parsed.fields, vec!["number", "title", "state"]);
        assert_eq!(parsed.rows.len(), 2);
        assert_eq!(parsed.rows[0], vec!["42", "Fix login redirect", "open"]);
        assert_eq!(parsed.total_count, Some(47));
        assert_eq!(parsed.hints, vec!["Call get_issue"]);
    }

    #[test]
    fn parses_quoted_comma_value() {
        let input = "t[1]{a}:\n  \"x,y\"\n";
        let parsed = parse(input).unwrap();
        assert_eq!(parsed.rows[0], vec!["x,y"]);
    }
}
