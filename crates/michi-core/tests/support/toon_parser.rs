//! Test-only strict TOON decoder and encoder-conformance validator — NOT part of the public API.
//!
//! It decodes the subset of TOON 4.4 (https://github.com/toon-format/spec, SPEC.md) that michi emits: a root object
//! whose entries are tabular arrays (§9.3), empty arrays `key: []` (§9.1), inline primitive arrays (§9.1), lists of
//! empty objects (§9.4/§10), and primitive fields (§8). Anything outside that subset is an error, not a guess.
//!
//! Beyond decoding, it rejects output a conforming *encoder* must not produce, so a passing decode proves the input
//! is canonical encoder output, not merely decodable:
//! - §12: LF only, 2-space indentation, no tabs in indentation, no blank lines, no trailing spaces, no trailing
//!   newline, no byte-order mark; exactly one space after `key:` and after an inline-array header.
//! - §7.2: every unquoted string token must be one the quoting rules allow unquoted, and every quoted string must be
//!   one they require quoting ("Otherwise, encoders MUST emit the string without quotes").
//! - §7.1: quoted strings use exactly the encoder escape column (`\\ \" \n \r \t`, lowercase `\u00xx` for the other
//!   C0 controls, everything else literal).
//! - §7.3: keys and field names are quoted exactly when they do not match `^[A-Za-z_][A-Za-z0-9_.]*$`.
//! - §2: numbers are canonical (no leading zeros, no trailing fractional zeros, no `-0`, exponent only outside
//!   `1e-6 <= |n| < 1e21`, lowercase `e` with an explicit sign).
//! - §6/§9: lengths match counts, field lists are non-empty, row widths match, no duplicate keys or field names.
//!
//! The quoting/escaping oracle here is written independently from `michi_core::toon` on purpose: checking michi's
//! output with michi's own escaper would be circular.
//!
//! Lives under `tests/support/` so Cargo does not treat it as its own integration-test binary. Its own unit tests
//! live in `tests/toon_parser.rs`.

#![allow(dead_code)] // each consuming test binary uses a different part of this module

use michi_core::toon::Value;

/// A decoded primitive (§4). Numbers keep their canonical text so tests compare exact output digits.
#[derive(Debug, Clone, PartialEq)]
pub enum Prim {
    /// A string, unquoted and unescaped.
    Str(String),
    /// A number, as its canonical text.
    Num(String),
    /// `true` or `false`.
    Bool(bool),
    /// `null`.
    Null,
}

impl Prim {
    /// `Prim::Str` from a `&str`.
    pub fn str(s: &str) -> Self {
        Self::Str(s.to_string())
    }
}

/// A decoded root-object entry value, limited to the forms michi emits.
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    /// `key: value` (§8).
    Prim(Prim),
    /// `key: []` (§9.1).
    EmptyArray,
    /// `key[N]: v1,v2` (§9.1).
    Inline(Vec<Prim>),
    /// `key[N]{f1,f2}:` followed by N rows (§9.3).
    Table { fields: Vec<String>, rows: Vec<Vec<Prim>> },
    /// `key[N]:` followed by N bare `-` items: an array of empty objects (§9.4, §10).
    EmptyObjects(usize),
}

/// A decoded root object, in document order.
#[derive(Debug, Clone, PartialEq)]
pub struct Doc {
    /// Each root key with its decoded value.
    pub entries: Vec<(String, Node)>,
}

impl Doc {
    /// The value of root key `key`, if present.
    pub fn get(&self, key: &str) -> Option<&Node> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }
}

/// A decoded michi list document: the type entry, then optional `totalCount` and `help` entries, in that order.
#[derive(Debug, PartialEq)]
pub struct ParsedToon {
    /// The list entry's key.
    pub type_name: String,
    /// Field names of the tabular header; empty for `key: []` and for a list of empty objects.
    pub fields: Vec<String>,
    /// Decoded rows; for a list of empty objects, one empty row per item.
    pub rows: Vec<Vec<Prim>>,
    /// The `totalCount` entry, if present.
    pub total_count: Option<usize>,
    /// The `help` entry's strings; empty when there is none.
    pub hints: Vec<String>,
    /// The `truncated` block's rows as `(row, field, removed)`; empty when no cell was cut.
    pub truncated: Vec<(usize, String, usize)>,
}

/// Decode a michi list document strictly. See the module docs for what is rejected.
pub fn parse(input: &str) -> Result<ParsedToon, String> {
    let doc = decode(input)?;
    let mut entries = doc.entries.into_iter();
    let (type_name, node) = entries.next().ok_or("empty document has no type entry")?;
    let (fields, rows) = match node {
        Node::Table { fields, rows } => (fields, rows),
        Node::EmptyArray => (Vec::new(), Vec::new()),
        Node::EmptyObjects(n) => (Vec::new(), vec![Vec::new(); n]),
        other => return Err(format!("first entry must be the list, got {other:?}")),
    };
    let mut total_count = None;
    let mut hints = Vec::new();
    let mut truncated = Vec::new();
    let mut seen_truncated = false;
    let mut seen_help = false;
    for (key, node) in entries {
        match (key.as_str(), node) {
            // The out-of-band cut report comes right after the list, before `totalCount` and `help`.
            ("truncated", Node::Table { fields, rows })
                if !seen_truncated && total_count.is_none() && !seen_help && fields == ["row", "field", "removed"] =>
            {
                seen_truncated = true;
                for cells in rows {
                    let [Prim::Num(row), Prim::Str(field), Prim::Num(removed)] = &cells[..] else {
                        return Err(format!("truncated row has the wrong shape: {cells:?}"));
                    };
                    truncated.push((
                        row.parse().map_err(|e| format!("truncated row {row:?}: {e}"))?,
                        field.clone(),
                        removed.parse().map_err(|e| format!("truncated removed {removed:?}: {e}"))?,
                    ));
                }
            }
            ("totalCount", Node::Prim(Prim::Num(n))) if total_count.is_none() && !seen_help => {
                total_count = Some(n.parse().map_err(|e| format!("totalCount {n:?}: {e}"))?);
            }
            ("help", Node::Inline(items)) if !seen_help => {
                seen_help = true;
                for item in items {
                    match item {
                        Prim::Str(s) => hints.push(s),
                        other => return Err(format!("help item decoded as non-string {other:?}")),
                    }
                }
            }
            (key, node) => return Err(format!("unexpected entry {key:?}: {node:?}")),
        }
    }
    Ok(ParsedToon { type_name, fields, rows, total_count, hints, truncated })
}

/// Decode a root object strictly. See the module docs for what is rejected.
pub fn decode(input: &str) -> Result<Doc, String> {
    if input.is_empty() {
        return Ok(Doc { entries: Vec::new() }); // §5: an empty document is the empty object
    }
    if input.starts_with('\u{feff}') {
        return Err("§12: encoders MUST NOT emit a byte-order mark".into());
    }
    if input.contains('\r') {
        return Err("§1.2/§12: encoders MUST use LF, never CR".into());
    }
    if input.ends_with('\n') {
        return Err("§12: encoders MUST NOT emit a trailing newline".into());
    }
    let lines: Vec<&str> = input.split('\n').collect();
    for (i, line) in lines.iter().enumerate() {
        if line.is_empty() {
            return Err(format!("§12: blank line at {i}"));
        }
        if line.ends_with(' ') || line.ends_with('\t') {
            return Err(format!("§12: trailing whitespace on line {i}: {line:?}"));
        }
        let indent = &line[..line.len() - line.trim_start_matches([' ', '\t']).len()];
        if indent.contains('\t') {
            return Err(format!("§12: tab in indentation on line {i}"));
        }
        if indent.len() % 2 != 0 {
            return Err(format!("§12: indentation not a multiple of 2 on line {i}"));
        }
        if line.trim_start().starts_with('#') {
            return Err(format!("§5.1: encoders MUST NOT emit comment lines (line {i})"));
        }
    }

    let mut entries: Vec<(String, Node)> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if line.starts_with(' ') {
            return Err(format!("§8/§14.2: over-indented line {i}: {line:?}"));
        }
        let (key, rest) = take_key(line)?;
        if entries.iter().any(|(k, _)| *k == key) {
            return Err(format!("§14.3: duplicate key {key:?}"));
        }
        i += 1;
        let node = if let Some(after) = rest.strip_prefix('[') {
            let (len_text, after) = after.split_once(']').ok_or("§6: unclosed bracket segment")?;
            let n = parse_length(len_text)?;
            if let Some(after) = after.strip_prefix('{') {
                let (fields_text, after) = split_fields_seg(after)?;
                if after != ":" {
                    return Err(format!(
                        "§6: fields-bearing header must end with ':' and carry no inline content: {line:?}"
                    ));
                }
                let fields = parse_fields(fields_text)?;
                let mut rows = Vec::with_capacity(n);
                for _ in 0..n {
                    let row = lines.get(i).ok_or("§14.1: fewer rows than declared")?;
                    i += 1;
                    let cells_text = row.strip_prefix("  ").ok_or("§9.3: row must be at depth 1")?;
                    if cells_text.starts_with(' ') {
                        return Err(format!("§9.3: row deeper than depth 1: {row:?}"));
                    }
                    let cells =
                        split_delimited(cells_text)?.into_iter().map(parse_prim).collect::<Result<Vec<_>, _>>()?;
                    if cells.len() != fields.len() {
                        return Err(format!("§14.1: row width {} != {} fields: {row:?}", cells.len(), fields.len()));
                    }
                    rows.push(cells);
                }
                if lines.get(i).is_some_and(|l| l.starts_with(' ')) {
                    return Err("§14.1: more rows than declared".into());
                }
                Node::Table { fields, rows }
            } else if after == ":" {
                if n == 0 {
                    return Err("§9.1: the legacy empty-array header MUST NOT be emitted; use `key: []`".into());
                }
                for _ in 0..n {
                    let item = lines.get(i).ok_or("§14.1: fewer list items than declared")?;
                    i += 1;
                    if *item != "  -" {
                        return Err(format!("only bare `-` (empty object) list items are in michi's subset: {item:?}"));
                    }
                }
                if lines.get(i).is_some_and(|l| l.starts_with(' ')) {
                    return Err("§14.1: more list items than declared".into());
                }
                Node::EmptyObjects(n)
            } else if let Some(values) = after.strip_prefix(": ") {
                if n == 0 {
                    return Err("§9.1: an empty array is `key: []`".into());
                }
                let items = split_delimited(values)?.into_iter().map(parse_prim).collect::<Result<Vec<_>, _>>()?;
                if items.len() != n {
                    return Err(format!("§14.1: inline array has {} values, header declares {n}", items.len()));
                }
                Node::Inline(items)
            } else {
                return Err(format!("§6/§12: malformed header tail {after:?} on {line:?}"));
            }
        } else if let Some(value) = rest.strip_prefix(": ") {
            if value.starts_with(' ') {
                return Err(format!("§12: exactly one space after ':' required: {line:?}"));
            }
            if value == "[]" {
                Node::EmptyArray
            } else {
                Node::Prim(parse_prim(value)?)
            }
        } else {
            return Err(format!("only key-value and array-header lines are in michi's subset: {line:?}"));
        };
        entries.push((key, node));
    }
    Ok(Doc { entries })
}

/// Read a key token (§7.3 encoder form, checked for canonical quoting) and return it with the rest of the line.
fn take_key(line: &str) -> Result<(String, &str), String> {
    if line.starts_with('"') {
        let end = closing_quote(line).ok_or_else(|| format!("§7.1: unterminated quoted key: {line:?}"))?;
        let token = &line[..=end];
        let key = unescape(token)?;
        if key_is_unquoted_form(&key) {
            return Err(format!("§7.3: key {key:?} matches the unquoted pattern and MUST NOT be quoted"));
        }
        check_canonical_escapes(token, &key)?;
        Ok((key, &line[end + 1..]))
    } else {
        let end = line.find(['[', ':']).ok_or_else(|| format!("no key terminator on {line:?}"))?;
        let key = &line[..end];
        if !key_is_unquoted_form(key) {
            return Err(format!("§7.3: key {key:?} does not match the unquoted pattern and MUST be quoted"));
        }
        Ok((key.to_string(), &line[end..]))
    }
}

fn parse_length(text: &str) -> Result<usize, String> {
    let ok = text == "0"
        || (text.starts_with(|c: char| ('1'..='9').contains(&c)) && text.chars().all(|c| c.is_ascii_digit()));
    if !ok {
        return Err(format!("§6: malformed length {text:?} (comma delimiter means no delimiter symbol)"));
    }
    text.parse().map_err(|e| format!("{e}"))
}

/// Split `f1,f2}:...` at the closing brace of the field list, ignoring braces in quoted spans (§6).
fn split_fields_seg(after_open: &str) -> Result<(&str, &str), String> {
    let mut in_quotes = false;
    let mut escaped = false;
    for (idx, c) in after_open.char_indices() {
        if in_quotes {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_quotes = false;
            }
        } else if c == '"' {
            in_quotes = true;
        } else if c == '{' {
            return Err("nested field groups are outside michi's subset".into());
        } else if c == '}' {
            return Ok((&after_open[..idx], &after_open[idx + 1..]));
        }
    }
    Err("§6: unmatched brace in field list".into())
}

fn parse_fields(text: &str) -> Result<Vec<String>, String> {
    if text.is_empty() {
        return Err("§6/§14.2: an empty field list `{}` is a syntax error".into());
    }
    let mut fields = Vec::new();
    for token in split_delimited(text)? {
        let (name, rest) = take_key(&format!("{token}:")).map(|(k, r)| (k, r.to_string()))?;
        if rest != ":" {
            return Err(format!("§6: malformed field entry {token:?}"));
        }
        if fields.contains(&name) {
            return Err(format!("§14.2: field name {name:?} repeated in one field list"));
        }
        fields.push(name);
    }
    Ok(fields)
}

/// Index of the `"` closing the quoted span opened at byte 0.
fn closing_quote(s: &str) -> Option<usize> {
    let mut escaped = false;
    for (idx, c) in s.char_indices().skip(1) {
        if escaped {
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == '"' {
            return Some(idx);
        }
    }
    None
}

/// Split on unquoted commas (the document delimiter, §11). Tokens are not trimmed: a conforming encoder never puts
/// spaces around a delimiter, so any space is either content (and then would have to be quoted) or an error.
fn split_delimited(text: &str) -> Result<Vec<&str>, String> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut in_quotes = false;
    let mut escaped = false;
    for (idx, c) in text.char_indices() {
        if in_quotes {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_quotes = false;
            }
        } else if c == '"' {
            in_quotes = true;
        } else if c == ',' {
            out.push(&text[start..idx]);
            start = idx + 1;
        }
    }
    if in_quotes {
        return Err(format!("§7.1: unterminated string in {text:?}"));
    }
    out.push(&text[start..]);
    Ok(out)
}

/// §4 / Appendix B.4 primitive parsing, plus the §7.2 encoder check in both directions.
pub fn parse_prim(token: &str) -> Result<Prim, String> {
    if token.starts_with('"') {
        if closing_quote(token) != Some(token.len() - 1) {
            return Err(format!("§7.4: characters after the closing quote, or unterminated: {token:?}"));
        }
        let s = unescape(token)?;
        if !string_needs_quotes(&s) {
            return Err(format!("§7.2: {s:?} does not require quoting, so encoders MUST emit it unquoted"));
        }
        check_canonical_escapes(token, &s)?;
        return Ok(Prim::Str(s));
    }
    match token {
        "true" => return Ok(Prim::Bool(true)),
        "false" => return Ok(Prim::Bool(false)),
        "null" => return Ok(Prim::Null),
        _ => {}
    }
    if is_json_number(token) {
        check_canonical_number(token)?;
        return Ok(Prim::Num(token.to_string()));
    }
    if string_needs_quotes(token) {
        return Err(format!("§7.2: unquoted token {token:?} requires quoting"));
    }
    Ok(Prim::Str(token.to_string()))
}

/// §7.1 decoder column.
fn unescape(token: &str) -> Result<String, String> {
    let inner = &token[1..token.len() - 1];
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('\\') => out.push('\\'),
            Some('"') => out.push('"'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('u') => {
                let hex: String = chars.by_ref().take(4).collect();
                if hex.len() != 4 || !hex.chars().all(|h| h.is_ascii_hexdigit()) {
                    return Err(format!("§7.1: \\u needs four hex digits in {token:?}"));
                }
                let code = u32::from_str_radix(&hex, 16).map_err(|e| e.to_string())?;
                out.push(char::from_u32(code).ok_or_else(|| format!("§7.1: surrogate escape in {token:?}"))?);
            }
            other => return Err(format!("§7.1: invalid escape \\{other:?} in {token:?}")),
        }
    }
    Ok(out)
}

/// §7.1 encoder column, written independently of michi.
pub fn oracle_quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn check_canonical_escapes(token: &str, decoded: &str) -> Result<(), String> {
    let canonical = oracle_quote(decoded);
    if token != canonical {
        return Err(format!("§7.1: non-canonical escaping {token:?}, encoder column gives {canonical:?}"));
    }
    Ok(())
}

/// §7.2 quoting conditions for a non-root string value with the comma document delimiter, written independently of
/// michi.
pub fn string_needs_quotes(s: &str) -> bool {
    s.is_empty()
        || s.starts_with([' ', '\t'])
        || s.ends_with([' ', '\t'])
        || matches!(s, "true" | "false" | "null")
        || is_numeric_like(s)
        || s.contains([':', '"', '\\', '[', ']', '{', '}', ','])
        || s.chars().any(|c| (c as u32) < 0x20)
        || s.starts_with(['-', '#'])
}

/// `/^[+-]?[0-9]+(?:\.[0-9]+)?(?:e[+-]?[0-9]+)?$/i`, ASCII digits only.
fn is_numeric_like(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = 0;
    let digits = |i: &mut usize| {
        let start = *i;
        while *i < b.len() && b[*i].is_ascii_digit() {
            *i += 1;
        }
        *i > start
    };
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    if !digits(&mut i) {
        return false;
    }
    if i < b.len() && b[i] == b'.' {
        i += 1;
        if !digits(&mut i) {
            return false;
        }
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            i += 1;
        }
        if !digits(&mut i) {
            return false;
        }
    }
    i == b.len()
}

/// §4 JSON number grammar `/^-?(?:0|[1-9][0-9]*)(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?$/`.
fn is_json_number(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = 0;
    if i < b.len() && b[i] == b'-' {
        i += 1;
    }
    if i < b.len() && b[i] == b'0' {
        i += 1;
    } else if i < b.len() && (b'1'..=b'9').contains(&b[i]) {
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
    } else {
        return false;
    }
    if i < b.len() && b[i] == b'.' {
        i += 1;
        let start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == start {
            return false;
        }
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            i += 1;
        }
        let start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == start {
            return false;
        }
    }
    i == b.len()
}

/// §2 canonical number form.
fn check_canonical_number(token: &str) -> Result<(), String> {
    if token == "-0" || token.starts_with("-0.") && token[3..].chars().all(|c| c == '0') {
        return Err(format!("§2: -0 MUST be normalized to 0, got {token:?}"));
    }
    let (mantissa, exponent) = match token.split_once(['e', 'E']) {
        Some((m, e)) => (m, Some(e)),
        None => (token, None),
    };
    if let Some((_, frac)) = mantissa.split_once('.') {
        if frac.ends_with('0') {
            return Err(format!("§2: trailing fractional zeros in {token:?}"));
        }
    }
    let value: f64 = token.parse().map_err(|e| format!("{e}"))?;
    let magnitude = value.abs();
    let in_canonical_range = value == 0.0 || (1e-6..1e21).contains(&magnitude);
    match exponent {
        Some(_) if in_canonical_range => Err(format!("§2: exponent notation inside the canonical range: {token:?}")),
        Some(e) if token.contains('E') || !(e.starts_with('+') || e.starts_with('-')) => {
            Err(format!("§2: exponent SHOULD use lowercase e and an explicit sign: {token:?}"))
        }
        _ => Ok(()),
    }
}

/// §7.3 unquoted-key pattern `^[A-Za-z_][A-Za-z0-9_.]*$`.
pub fn key_is_unquoted_form(k: &str) -> bool {
    let mut chars = k.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
}

/// The primitive a michi [`Value`] must decode to, for round-trip comparisons of untruncated cells.
pub fn value_to_prim(v: &Value) -> Prim {
    match v {
        Value::Str(s) => Prim::Str(s.to_string()),
        Value::Int(n) => Prim::Num(n.to_string()),
        Value::UInt(n) => Prim::Num(n.to_string()),
        Value::Float(f) if !f.is_finite() => Prim::Null,
        Value::Float(f) if *f == 0.0 => Prim::Num("0".into()),
        Value::Float(f) => Prim::Num(f.to_string()),
        Value::Bool(b) => Prim::Bool(*b),
        Value::Null => Prim::Null,
        other => panic!("value_to_prim: unhandled Value variant {other:?}"),
    }
}
