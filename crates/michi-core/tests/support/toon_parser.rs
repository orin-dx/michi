//! Test-only TOON parser — NOT part of the public API. Exists purely to
//! support round-trip property tests (render → parse → compare) per
//! docs/spec/05-scope-and-quality.md's testing strategy. Parses only what render_toon actually
//! produces; not a general-purpose TOON parser for untrusted input.
//!
//! Lives under `tests/support/` rather than directly in `tests/` so Cargo's
//! target auto-discovery does not treat it as its own integration-test
//! binary — files in subdirectories of `tests/` aren't auto-discovered,
//! which lets multiple top-level test binaries `mod` it in without
//! recompiling (and rerunning) the same tests once per binary. Its own unit
//! tests live in `tests/toon_parser.rs`, the sole consumer that runs them.

use michi_core::toon::Value;

#[derive(Debug, PartialEq)]
pub struct ParsedToon {
    pub type_name: String,
    pub fields: Vec<String>,
    pub rows: Vec<Vec<String>>, // parsed as raw strings; comparing rendered text, not typed values
    pub total_count: Option<usize>,
    pub hints: Vec<String>,
}

pub fn parse(input: &str) -> Option<ParsedToon> {
    let mut lines = input.lines();
    let header = lines.next()?;
    let (type_name, rest) = header.split_once('[')?;
    let (count_str, rest) = rest.split_once(']')?;
    let row_count: usize = count_str.parse().ok()?;
    let fields_str = rest.strip_prefix('{')?.strip_suffix(":")?.strip_suffix('}')?;
    let fields: Vec<String> =
        if fields_str.is_empty() { Vec::new() } else { fields_str.split(',').map(str::to_string).collect() };

    let mut rows = Vec::with_capacity(row_count);
    for _ in 0..row_count {
        let line = lines.next()?.strip_prefix("  ")?;
        rows.push(split_toon_row(line));
    }

    let mut total_count = None;
    let mut hints = Vec::new();
    for line in lines {
        if let Some(n) = line.strip_prefix("totalCount: ") {
            total_count = n.parse().ok();
        } else if line.starts_with("help[") {
            // hint lines follow, each prefixed "  "
        } else if let Some(h) = line.strip_prefix("  ") {
            hints.push(h.to_string());
        }
    }

    Some(ParsedToon { type_name: type_name.to_string(), fields, rows, total_count, hints })
}

fn split_toon_row(line: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut chars = line.chars().peekable();
    let mut current = String::new();
    let mut in_quotes = false;
    while let Some(c) = chars.next() {
        match c {
            '"' if !in_quotes => in_quotes = true,
            '"' if in_quotes => in_quotes = false,
            '\\' if in_quotes => match chars.next() {
                Some('n') => current.push('\n'),
                Some('r') => current.push('\r'),
                Some(next) => current.push(next),
                None => {}
            },
            ',' if !in_quotes => {
                values.push(std::mem::take(&mut current));
            }
            other => current.push(other),
        }
    }
    values.push(current);
    values
}

// Only `tests/proptest_toon.rs` calls this; `tests/toon_parser.rs` doesn't,
// and since this module is compiled independently into each consuming test
// binary, that binary's dead-code analysis would otherwise warn here.
#[allow(dead_code)]
pub fn value_to_string(v: &Value) -> String {
    match v {
        Value::Str(s) => s.to_string(),
        Value::Int(n) => n.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Bool(b) => (if *b { "true" } else { "false" }).to_string(),
        Value::Null => String::new(),
    }
}
